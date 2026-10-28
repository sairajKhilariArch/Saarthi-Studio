// Pure logic for the Free AI Studio VS Code extension: token-budgeted context
// packing, a small conversation store, a workspace API, and the built-in HTTP
// server used when the desktop app is not running. Nothing here imports
// `vscode`, so `node --test` can exercise all of it.

import * as fs from "node:fs";
import * as http from "node:http";
import * as path from "node:path";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";

/* ------------------------------------------------------------------ */
/* types + constants                                                   */
/* ------------------------------------------------------------------ */

export interface StudioConfig {
  routerBaseUrl: string;
  apiKey: string;
  model: string;
  contextBudgetTokens: number;
  historyMessages: number;
  sendWorkspaceOutline: boolean;
}

export const CHARS_PER_TOKEN = 4;
export const DEFAULT_BUDGET_TOKENS = 6000;
export const MIN_BUDGET_TOKENS = 1000;
export const MAX_BUDGET_TOKENS = 32000;
export const DEFAULT_HISTORY_MESSAGES = 6;
export const MIN_HISTORY_MESSAGES = 2;
export const MAX_HISTORY_MESSAGES = 24;
const CURRENT_MESSAGE_CAP_TOKENS = 3000;
const HISTORY_MESSAGE_CAP_TOKENS = 1200;
const MAX_FILE_BYTES = 1024 * 1024;
const MAX_ENTRIES = 400;

export const SYSTEM_PROMPT =
  "You are a concise, helpful AI coding assistant. Be accurate and safe; never claim to run code unless you did.";

export function normalizeBaseUrl(value: string): string {
  const trimmed = String(value || "").trim().replace(/\/+$/, "");
  if (!trimmed) return "http://127.0.0.1:3001/v1";
  return trimmed.endsWith("/v1") ? trimmed : `${trimmed}/v1`;
}

/* ------------------------------------------------------------------ */
/* token budget                                                        */
/* ------------------------------------------------------------------ */

export function estimateTokens(text: string): number {
  return Math.ceil(String(text || "").length / CHARS_PER_TOKEN);
}

/** Keep both ends of a long text inside `maxTokens`. */
export function cropText(text: string, maxTokens: number): string {
  const maxChars = Math.max(0, maxTokens) * CHARS_PER_TOKEN;
  const value = String(text ?? "");
  if (value.length <= maxChars) return value;
  const head = Math.floor((maxChars * 2) / 3);
  const tail = Math.max(0, maxChars - head - 48);
  const omitted = value.length - head - tail;
  return `${value.slice(0, head)}\n… [${omitted} characters omitted] …\n${tail > 0 ? value.slice(value.length - tail) : ""}`;
}

/** Replace an old attached code block with a marker so it is not re-sent. */
export function stripAttachedCode(content: string): string {
  const marker = "Here is the code from my editor";
  const index = String(content || "").indexOf(marker);
  if (index < 0) return String(content || "");
  const head = String(content).slice(0, index).trimEnd();
  if (!head) return "[the attachment from this message was omitted to save context]";
  return `${head}\n[attachment omitted from context; ask to re-attach it]`;
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface PackedRequest {
  messages: ChatMessage[];
  estimatedTokens: number;
  trimmedMessages: number;
}

/**
 * Assemble the messages sent to the router under `budgetTokens`.
 * `history` must end with the message being sent right now.
 */
export function packMessages(options: {
  system?: string;
  outline?: string | null;
  history: ChatMessage[];
  budgetTokens?: number;
  historyMessages?: number;
}): PackedRequest {
  const budget = Math.min(
    MAX_BUDGET_TOKENS,
    Math.max(MIN_BUDGET_TOKENS, Number(options.budgetTokens) || DEFAULT_BUDGET_TOKENS),
  );
  const keep = Math.min(
    MAX_HISTORY_MESSAGES,
    Math.max(MIN_HISTORY_MESSAGES, Number(options.historyMessages) || DEFAULT_HISTORY_MESSAGES),
  );
  const base = options.system || SYSTEM_PROMPT;
  const systemText = options.outline
    ? `${base}\n\nWorkspace files (names only, ask before assuming contents):\n${options.outline}`
    : base;

  const messages: ChatMessage[] = [{ role: "system", content: systemText }];
  let used = estimateTokens(systemText);
  let trimmed = 0;

  const history = Array.isArray(options.history) ? options.history : [];
  const window = Math.min(history.length, Math.max(1, keep));
  const start = history.length - window;
  for (let index = start; index < history.length; index += 1) {
    const entry = history[index];
    const isCurrent = index === history.length - 1;
    const role = entry?.role === "assistant" ? "assistant" : "user";
    const raw = String(entry?.content ?? "");
    if (raw === "" && !isCurrent) continue;
    const content = isCurrent
      ? cropText(raw, CURRENT_MESSAGE_CAP_TOKENS)
      : cropText(stripAttachedCode(raw), HISTORY_MESSAGE_CAP_TOKENS);
    const cost = estimateTokens(content);
    if (!isCurrent && used + cost > budget) {
      trimmed += 1;
      continue;
    }
    used += cost;
    messages.push({ role, content });
  }

  return { messages, estimatedTokens: used, trimmedMessages: trimmed };
}

/** Compact names-only outline of the workspace for the system prompt. */
export function outlineFromEntries(rootName: string, entries: string[]): string {
  const lines = [`${rootName}/`];
  for (const entry of entries.slice(0, 200)) lines.push(entry);
  if (entries.length > 200) lines.push(`… and ${entries.length - 200} more entries`);
  return lines.join("\n");
}

/* ------------------------------------------------------------------ */
/* conversation store                                                  */
/* ------------------------------------------------------------------ */

export interface Conversation {
  id: string;
  title: string;
  created_at: number;
  updated_at: number;
  /** Owning project (desktop parity); absent on threads stored before projects existed. */
  project_id?: string;
}

export interface StoredMessage {
  conversation_id: string;
  role: string;
  content: string;
  created_at: number;
}

export interface RequestLogEntry {
  created_at: number;
  conversation_id: string;
  model: string;
  provider: string;
  status: "ok" | "error";
  latency_ms: number;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
  error: string;
}

interface StoreShape {
  conversations: Conversation[];
  messages: StoredMessage[];
  requests: RequestLogEntry[];
}

/** JSON-backed history store. Small, portable, and easy to back up. */
export class ConversationStore {
  private data: StoreShape = { conversations: [], messages: [], requests: [] };

  constructor(private readonly file: string) {
    this.load();
  }

  private load(): void {
    try {
      const raw = fs.readFileSync(this.file, "utf8");
      const parsed = JSON.parse(raw) as Partial<StoreShape>;
      this.data = {
        conversations: Array.isArray(parsed.conversations) ? parsed.conversations : [],
        messages: Array.isArray(parsed.messages) ? parsed.messages : [],
        requests: Array.isArray(parsed.requests) ? parsed.requests : [],
      };
    } catch {
      this.data = { conversations: [], messages: [], requests: [] };
    }
  }

  private save(): void {
    const dir = path.dirname(this.file);
    fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
    const temp = `${this.file}.tmp`;
    fs.writeFileSync(temp, JSON.stringify(this.data), { mode: 0o600 });
    fs.renameSync(temp, this.file);
  }

  list(projectId?: string | null): Conversation[] {
    return [...this.data.conversations]
      .filter((entry) => !projectId || entry.project_id === projectId)
      .sort((a, b) => b.updated_at - a.updated_at);
  }

  create(title: string, projectId?: string | null): Conversation {
    const now = Date.now() / 1000;
    const conversation: Conversation = {
      id: randomUUID(),
      title: cleanTitle(title) || "New thread",
      created_at: now,
      updated_at: now,
      ...(projectId ? { project_id: projectId } : {}),
    };
    this.data.conversations.push(conversation);
    this.save();
    return conversation;
  }

  has(id: string): boolean {
    return this.data.conversations.some((entry) => entry.id === id);
  }

  messages(id: string): StoredMessage[] {
    return this.data.messages
      .filter((entry) => entry.conversation_id === id)
      .sort((a, b) => a.created_at - b.created_at);
  }

  append(id: string, role: "user" | "assistant", content: string): void {
    const now = Date.now() / 1000;
    this.data.messages.push({ conversation_id: id, role, content, created_at: now });
    const conversation = this.data.conversations.find((entry) => entry.id === id);
    if (!conversation) return;
    conversation.updated_at = now;
    const count = this.data.messages.filter((entry) => entry.conversation_id === id && entry.role === "user").length;
    if (count === 1) conversation.title = cleanTitle(content) || conversation.title;
    this.save();
  }

  remove(id: string): boolean {
    const before = this.data.conversations.length;
    this.data.conversations = this.data.conversations.filter((entry) => entry.id !== id);
    this.data.messages = this.data.messages.filter((entry) => entry.conversation_id !== id);
    if (this.data.conversations.length === before) return false;
    this.save();
    return true;
  }

  /** Delete every thread owned by a project (mirrors the desktop's cascade). */
  removeProject(projectId: string): number {
    const before = this.data.conversations.length;
    const doomed = new Set(
      this.data.conversations.filter((entry) => entry.project_id === projectId).map((entry) => entry.id),
    );
    this.data.conversations = this.data.conversations.filter((entry) => entry.project_id !== projectId);
    this.data.messages = this.data.messages.filter((entry) => !doomed.has(entry.conversation_id));
    const removed = before - this.data.conversations.length;
    if (removed) this.save();
    return removed;
  }

  logRequest(entry: RequestLogEntry): void {
    this.data.requests.push(entry);
    if (this.data.requests.length > 500) this.data.requests = this.data.requests.slice(-500);
    this.save();
  }

  /** Paginated, sorted, filtered view of the request log (desktop parity). */
  usage(params: URLSearchParams = new URLSearchParams()): {
    summary: Record<string, number>;
    total: number;
    page: number;
    page_size: number;
    pages: number;
    requests: RequestLogEntry[];
    series: Array<Record<string, number>>;
    by_model: Array<Record<string, unknown>>;
  } {
    const pageSize = Math.min(200, Math.max(5, Number(params.get("page_size")) || 10));
    const sort = params.get("sort") || "created_at";
    const dir = params.get("dir") === "asc" ? 1 : -1;
    const modelFilter = (params.get("model") || "").toLowerCase();
    const providerFilter = (params.get("provider") || "").toLowerCase();
    const from = Number(params.get("from")) || 0;
    const to = Number(params.get("to")) || 0;

    let rows = [...this.data.requests].reverse();
    if (modelFilter) rows = rows.filter((row) => (row.model || "").toLowerCase().includes(modelFilter));
    if (providerFilter) rows = rows.filter((row) => (row.provider || "").toLowerCase().includes(providerFilter));
    if (from) rows = rows.filter((row) => row.created_at >= from);
    if (to) rows = rows.filter((row) => row.created_at <= to);

    const value = (row: RequestLogEntry): string | number => {
      switch (sort) {
        case "latency": return row.latency_ms || -1;
        case "tokens": return row.total_tokens ?? -1;
        case "status": return row.status;
        case "detail": return row.error || "";
        case "model": return row.model || "";
        case "provider": return row.provider || "";
        default: return row.created_at;
      }
    };
    rows.sort((a, b) => {
      const left = value(a);
      const right = value(b);
      if (left === right) return 0;
      return (left > right ? 1 : -1) * dir;
    });

    const total = rows.length;
    const pages = Math.max(1, Math.ceil(total / pageSize));
    const page = Math.min(Math.max(1, Number(params.get("page")) || 1), pages);

    const okCount = rows.filter((row) => row.status === "ok").length;
    const tokens = rows.reduce((sum, row) => sum + (row.total_tokens || 0), 0);
    const latencies = rows.filter((row) => row.latency_ms > 0).map((row) => row.latency_ms);
    const summary = {
      requests: total,
      successful: okCount,
      failed: total - okCount,
      total_tokens: tokens,
      avg_latency_ms: latencies.length
        ? Math.round(latencies.reduce((sum, value_) => sum + value_, 0) / latencies.length)
        : 0,
    };

    // Daily buckets (UTC day, oldest first) for the requests-per-day chart.
    const buckets = new Map<number, { requests: number; ok: number; tokens: number; latencySum: number; latencyCount: number }>();
    for (const row of rows) {
      const day = Math.floor(row.created_at / 86400) * 86400;
      const bucket = buckets.get(day) || { requests: 0, ok: 0, tokens: 0, latencySum: 0, latencyCount: 0 };
      bucket.requests += 1;
      if (row.status === "ok") bucket.ok += 1;
      bucket.tokens += row.total_tokens || 0;
      if (row.latency_ms > 0) { bucket.latencySum += row.latency_ms; bucket.latencyCount += 1; }
      buckets.set(day, bucket);
    }
    const series = [...buckets.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([day, bucket]) => ({
        day,
        requests: bucket.requests,
        ok: bucket.ok,
        tokens: bucket.tokens,
        avg_latency_ms: bucket.latencyCount ? Math.round(bucket.latencySum / bucket.latencyCount) : 0,
      }));

    const groups = new Map<string, { provider: string; requests: number; ok: number; tokens: number; latencySum: number; latencyCount: number }>();
    for (const row of rows) {
      const key = row.model || "(unknown)";
      const group = groups.get(key) || { provider: row.provider || "(unknown)", requests: 0, ok: 0, tokens: 0, latencySum: 0, latencyCount: 0 };
      group.requests += 1;
      if (row.status === "ok") group.ok += 1;
      group.tokens += row.total_tokens || 0;
      if (row.latency_ms > 0) { group.latencySum += row.latency_ms; group.latencyCount += 1; }
      groups.set(key, group);
    }
    const by_model = [...groups.entries()]
      .map(([model, group]) => ({
        model,
        provider: group.provider,
        requests: group.requests,
        ok: group.ok,
        tokens: group.tokens,
        avg_latency_ms: group.latencyCount ? Math.round(group.latencySum / group.latencyCount) : 0,
      }))
      .sort((a, b) => b.requests - a.requests)
      .slice(0, 20);

    return {
      summary,
      total,
      page,
      page_size: pageSize,
      pages,
      requests: rows.slice((page - 1) * pageSize, page * pageSize),
      series,
      by_model,
    };
  }
}

export function cleanTitle(value: string): string {
  return String(value || "").replace(/\s+/g, " ").trim().slice(0, 80);
}

/* ------------------------------------------------------------------ */
/* small JSON file stores (projects, workflows, skill state)          */
/* ------------------------------------------------------------------ */

class JsonFileStore<T> {
  private value: T;
  constructor(private readonly file: string, fallback: T) {
    this.value = fallback;
    try {
      this.value = JSON.parse(fs.readFileSync(file, "utf8")) as T;
    } catch {
      this.value = fallback;
    }
  }
  read(): T {
    return this.value;
  }
  write(next: T): void {
    this.value = next;
    try {
      fs.mkdirSync(path.dirname(this.file), { recursive: true, mode: 0o700 });
      const temp = `${this.file}.tmp`;
      fs.writeFileSync(temp, JSON.stringify(next, null, 2), { mode: 0o600 });
      fs.renameSync(temp, this.file);
    } catch {
      // A failed write only means the next start starts fresh again.
    }
  }
}

/* ------------------------------------------------------------------ */
/* skills discovery (desktop skills.rs parity, smaller parser)         */
/* ------------------------------------------------------------------ */

export interface DiscoveredSkill {
  id: string;
  name: string;
  description: string;
  body: string;
  path: string;
  source: string;
  error: string | null;
}

function homeDir(): string {
  return process.env.HOME || process.env.USERPROFILE || "";
}

/** YAML-ish frontmatter: `key: value` lines, folded and literal scalars kept readable. */
function parseFrontmatter(raw: string): { meta: Record<string, string>; body: string } {
  const meta: Record<string, string> = {};
  let body = raw;
  if (raw.startsWith("---")) {
    const end = raw.indexOf("\n---", 3);
    if (end > 0) {
      const block = raw.slice(3, end).replace(/^\r?\n/, "");
      body = raw.slice(raw.indexOf("\n", end + 4) + 1);
      let key = "";
      for (const line of block.split("\n")) {
        const match = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
        if (match) {
          key = match[1];
          meta[key] = match[2].replace(/^['\"]|['\"]$/g, "").trim();
        } else if (key && /^\s+/.test(line)) {
          meta[key] = `${meta[key]} ${line.trim()}`.trim();
        }
      }
    }
  }
  return { meta, body };
}

function skillIdFor(dirName: string): string {
  return dirName.toLowerCase().replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "");
}

export function discoverSkills(roots: Array<{ source: string; dir: string }>): DiscoveredSkill[] {
  const seen = new Set<string>();
  const skills: DiscoveredSkill[] = [];
  for (const root of roots) {
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(root.dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const skillFile = path.join(root.dir, entry.name, "SKILL.md");
      if (!fs.existsSync(skillFile)) continue;
      const id = skillIdFor(entry.name);
      if (!id || seen.has(id)) continue;
      seen.add(id);
      try {
        const raw = fs.readFileSync(skillFile, "utf8");
        const { meta, body } = parseFrontmatter(raw);
        skills.push({
          id,
          name: meta.name || entry.name,
          description: meta.description || "",
          body,
          path: skillFile,
          source: root.source,
          error: null,
        });
      } catch (error) {
        skills.push({
          id,
          name: entry.name,
          description: "",
          body: "",
          path: skillFile,
          source: root.source,
          error: (error as Error).message,
        });
      }
    }
  }
  return skills;
}

/** Bound walk of every file under `root` for the Artifacts view. */
export function listArtifacts(root: string): {
  files: Array<{ name: string; path: string; size: number; ext: string }>;
  truncated: boolean;
  count: number;
} {
  const files: Array<{ name: string; path: string; size: number; ext: string }> = [];
  let truncated = false;
  const skip = new Set(["node_modules", ".git", "target", "dist", "build", ".next"]);
  const walk = (dir: string, depth: number) => {
    if (files.length >= 2000 || depth > 8) { truncated = true; return; }
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (files.length >= 2000) { truncated = true; return; }
      if (skip.has(entry.name)) continue;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(full, depth + 1);
        continue;
      }
      if (!entry.isFile()) continue;
      try {
        const stat = fs.statSync(full);
        const name = entry.name;
        const dot = name.lastIndexOf(".");
        files.push({
          name,
          path: path.relative(root, full).split(path.sep).join("/"),
          size: stat.size,
          ext: dot > 0 ? name.slice(dot + 1).toLowerCase() : "",
        });
      } catch {
        // Unreadable file: skip it rather than failing the whole listing.
      }
    }
  };
  walk(root, 0);
  files.sort((a, b) => a.ext.localeCompare(b.ext) || a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  return { files, truncated, count: files.length };
}

/** Open a URL in the platform default browser (desktop browser.rs parity). */
export function openInSystemBrowser(url: string): void {
  const [command, args] =
    process.platform === "darwin"
      ? ["open", [url]]
      : process.platform === "win32"
        ? ["cmd", ["/c", "start", "", url]]
        : ["xdg-open", [url]];
  try {
    spawn(command, args, { stdio: "ignore", detached: true }).unref();
  } catch {
    // The caller reports the failure to the UI.
    throw new WorkspaceError(500, "Could not launch the default browser.");
  }
}

/* ------------------------------------------------------------------ */
/* router call                                                         */
/* ------------------------------------------------------------------ */

export interface RouterReply {
  answer: string;
  meta: Record<string, unknown>;
}

export async function callRouter(
  config: StudioConfig,
  messages: ChatMessage[],
  fetchImpl: typeof fetch,
  modelOverride?: string,
): Promise<RouterReply> {
  const base = normalizeBaseUrl(config.routerBaseUrl);
  const model = modelOverride || config.model || "auto";
  const started = Date.now();
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (config.apiKey) headers.Authorization = `Bearer ${config.apiKey}`;
  let response: Response;
  try {
    response = await fetchImpl(`${base}/chat/completions`, {
      method: "POST",
      headers,
      body: JSON.stringify({ model, messages, stream: false }),
      signal: AbortSignal.timeout(120_000),
    });
  } catch (error) {
    throw new Error(`Could not reach the router at ${base}: ${(error as Error).message}`);
  }
  const latency = Date.now() - started;
  const body = (await response.json().catch(() => ({}))) as Record<string, unknown>;
  if (!response.ok) {
    const message =
      (body.error as { message?: string } | undefined)?.message ||
      (typeof body.error === "string" ? body.error : "") ||
      `Router returned HTTP ${response.status}`;
    const error = new Error(message) as Error & { latency?: number };
    error.latency = latency;
    throw error;
  }
  const choices = (body.choices as Array<{ message?: { content?: string } }>) || [];
  const answer = choices[0]?.message?.content || "";
  const usage = (body.usage as Record<string, number>) || {};
  return {
    answer,
    meta: {
      model: (body.model as string) || model,
      provider: (body.provider as string) || "router",
      latency_ms: latency,
      prompt_tokens: usage.prompt_tokens ?? null,
      completion_tokens: usage.completion_tokens ?? null,
      total_tokens: usage.total_tokens ?? null,
      fallback_attempts: (body.fallback_attempts as string) || "",
    },
  };
}

/* ------------------------------------------------------------------ */
/* desktop detection                                                   */
/* ------------------------------------------------------------------ */

export interface DesktopProbe {
  running: boolean;
  embeddable: boolean;
  health: Record<string, unknown> | null;
}

/** Is a Free AI Studio desktop app answering on this loopback address? */
export async function probeDesktop(
  desktopUrl: string,
  fetchImpl: typeof fetch = fetch,
  timeoutMs = 900,
): Promise<DesktopProbe> {
  const base = String(desktopUrl || "").replace(/\/+$/, "");
  if (!isLoopbackUrl(base)) return { running: false, embeddable: false, health: null };
  try {
    const response = await fetchImpl(`${base}/api/health`, { signal: AbortSignal.timeout(timeoutMs) });
    if (!response.ok) return { running: false, embeddable: false, health: null };
    const health = (await response.json().catch(() => ({}))) as Record<string, unknown>;
    return { running: true, embeddable: health.embeddable === true, health };
  } catch {
    return { running: false, embeddable: false, health: null };
  }
}

export function isLoopbackUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return ["127.0.0.1", "localhost", "[::1]", "::1"].includes(url.hostname);
  } catch {
    return false;
  }
}

/* ------------------------------------------------------------------ */
/* workspace helpers                                                   */
/* ------------------------------------------------------------------ */

export class WorkspaceError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
  }
}

/** Resolve a UI path inside the workspace root, refusing escapes. */
export function resolveInside(root: string, relative: string): string {
  const base = fs.realpathSync(root);
  const candidate = String(relative || "").replace(/\\/g, "/").trim();
  if (!candidate) return base;
  if (candidate.startsWith("/") || /^[a-zA-Z]:/.test(candidate)) {
    throw new WorkspaceError(400, "Use a path relative to the open folder.");
  }
  const segments = candidate.split("/").filter((segment) => segment && segment !== ".");
  if (segments.some((segment) => segment === "..")) {
    throw new WorkspaceError(400, "That path leaves the open folder.");
  }
  const joined = path.join(base, segments.join(path.sep));
  let resolved: string;
  try {
    resolved = fs.realpathSync(joined);
  } catch {
    // Creating a new entry (possibly in folders that do not exist yet):
    // validate the nearest existing ancestor instead.
    let ancestor = path.dirname(joined);
    while (ancestor !== base && !fs.existsSync(ancestor)) {
      const parent = path.dirname(ancestor);
      if (parent === ancestor) break;
      ancestor = parent;
    }
    let realAncestor: string;
    try {
      realAncestor = fs.realpathSync(ancestor);
    } catch {
      throw new WorkspaceError(404, "That folder does not exist.");
    }
    if (realAncestor !== base && !realAncestor.startsWith(base + path.sep)) {
      throw new WorkspaceError(400, "That path leaves the open folder.");
    }
    return joined;
  }
  if (resolved !== base && !resolved.startsWith(base + path.sep)) {
    throw new WorkspaceError(400, "That path leaves the open folder.");
  }
  return resolved;
}

export interface DirEntry {
  name: string;
  path: string;
  dir: boolean;
  symlink: boolean;
  hidden: boolean;
  size: number;
}

export function listDir(root: string, relative: string): { path: string; entries: DirEntry[]; truncated: boolean } {
  const dir = resolveInside(root, relative);
  const stat = fs.statSync(dir);
  if (!stat.isDirectory()) throw new WorkspaceError(400, "That path is not a folder.");
  const base = relativeLabel(root, dir);
  const entries: DirEntry[] = [];
  let truncated = false;
  for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entries.length >= MAX_ENTRIES) {
      truncated = true;
      break;
    }
    let isDir = item.isDirectory();
    let size = 0;
    try {
      const info = fs.statSync(path.join(dir, item.name));
      isDir = info.isDirectory();
      size = isDir ? 0 : info.size;
    } catch {
      // Broken symlink: show it as a file with unknown size.
    }
    entries.push({
      name: item.name,
      path: base ? `${base}/${item.name}` : item.name,
      dir: isDir,
      symlink: item.isSymbolicLink(),
      hidden: item.name.startsWith("."),
      size,
    });
  }
  entries.sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  return { path: base, entries, truncated };
}

export function readTextFile(root: string, relative: string) {
  const file = resolveInside(root, relative);
  const stat = fs.statSync(file);
  if (stat.isDirectory()) throw new WorkspaceError(400, "That path is a folder, not a file.");
  if (stat.size > MAX_FILE_BYTES) {
    throw new WorkspaceError(413, `This file is ${Math.round(stat.size / 1024)} KB. The editor opens files up to 1024 KB.`);
  }
  const bytes = fs.readFileSync(file);
  if (bytes.includes(0)) throw new WorkspaceError(415, "This looks like a binary file, so it cannot be opened in the text editor.");
  const content = bytes.toString("utf8");
  return {
    path: relativeLabel(root, file),
    content,
    language: languageFor(file),
    size: stat.size,
    modified: stat.mtimeMs / 1000,
  };
}

export function writeTextFile(root: string, relative: string, content: string) {
  const file = resolveInside(root, relative);
  if (fs.existsSync(file) && fs.statSync(file).isDirectory()) {
    throw new WorkspaceError(400, "That path is a folder, not a file.");
  }
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, content, "utf8");
  return readTextFile(root, relative);
}

export function createEntry(root: string, relative: string, dir: boolean) {
  const target = resolveInside(root, relative);
  if (fs.existsSync(target)) throw new WorkspaceError(409, "Something with that name already exists.");
  if (dir) fs.mkdirSync(target, { recursive: true });
  else {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "");
  }
  return { ok: true, path: relativeLabel(root, target), dir };
}

export function renameEntry(root: string, from: string, to: string) {
  const source = resolveInside(root, from);
  const target = resolveInside(root, to);
  if (fs.existsSync(target)) throw new WorkspaceError(409, "Something with that name already exists.");
  fs.renameSync(source, target);
  return { ok: true, from: relativeLabel(root, source), to: relativeLabel(root, target) };
}

export function deleteEntry(root: string, relative: string) {
  const target = resolveInside(root, relative);
  if (target === fs.realpathSync(root)) throw new WorkspaceError(400, "Refusing to delete the open folder itself.");
  fs.rmSync(target, { recursive: true, force: false });
  return { ok: true, path: relative };
}

export function relativeLabel(root: string, absolute: string): string {
  const relative = path.relative(fs.realpathSync(root), absolute);
  return relative.split(path.sep).join("/");
}

const LANGUAGES: Record<string, string> = {
  ".ts": "typescript", ".tsx": "typescript", ".js": "javascript", ".jsx": "javascript",
  ".mjs": "javascript", ".cjs": "javascript", ".rs": "rust", ".py": "python",
  ".go": "go", ".java": "java", ".kt": "kotlin", ".c": "c", ".h": "c",
  ".cpp": "cpp", ".hpp": "cpp", ".cs": "csharp", ".rb": "ruby", ".php": "php",
  ".swift": "swift", ".sh": "bash", ".bash": "bash", ".zsh": "bash",
  ".json": "json", ".yaml": "yaml", ".yml": "yaml", ".toml": "toml",
  ".md": "markdown", ".html": "html", ".css": "css", ".scss": "scss",
  ".sql": "sql", ".xml": "xml", ".ini": "ini", ".gradle": "groovy",
};

export function languageFor(file: string): string {
  return LANGUAGES[path.extname(file).toLowerCase()] || "text";
}

/** Remove a temporary directory created by tests or the server. */
export function cleanupTemp(dir: string): void {
  fs.rmSync(dir, { recursive: true, force: true });
}

/* ------------------------------------------------------------------ */
/* embedded server                                                     */
/* ------------------------------------------------------------------ */

export interface EmbeddedServerOptions {
  uiDir: string;
  storeFile: string;
  workspaceRoot?: string;
  getConfig: () => StudioConfig;
  fetchImpl?: typeof fetch;
  /** Router admin port used by the console proxy (FreeLLMAPI: 3001). */
  routerPort?: number;
  log?: (message: string) => void;
}

export interface EmbeddedServer {
  server: http.Server;
  port: number;
  url: string;
  close: () => Promise<void>;
}

const ADMIN_PREFIXES = [
  "analytics", "backups", "cache", "client-profiles", "compression", "conversations",
  "decisions", "embeddings", "fallback", "free-tier", "health", "keys", "logs",
  "media", "models", "premium", "profiles", "search", "settings", "update",
].join(",").split(",");

const CONTENT_TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".json": "application/json",
};

/** Normalise a console path and refuse anything outside the admin API. */
export function sanitizeConsolePath(raw: string): string {
  let value = String(raw || "").trim().replace(/^\/+/, "");
  if (value.startsWith("api/")) value = value.slice(4);
  value = value.replace(/\/+$/, "");
  if (!value) throw new WorkspaceError(400, "Missing router API path.");
  if (value.length > 300) throw new WorkspaceError(400, "Router API path is too long.");
  const segments = value.split("/");
  for (const segment of segments) {
    if (!segment) throw new WorkspaceError(400, "Router API path has an empty segment.");
    if (segment === "." || segment === "..") throw new WorkspaceError(400, "Router API path must stay inside the admin API.");
    if (!/^[A-Za-z0-9._:~-]+$/.test(segment)) throw new WorkspaceError(400, "Router API path contains unsupported characters.");
  }
  if (!ADMIN_PREFIXES.includes(segments[0])) {
    throw new WorkspaceError(400, `\`${segments[0]}\` is not an admin API family this app manages.`);
  }
  return segments.join("/");
}

export async function startEmbeddedServer(options: EmbeddedServerOptions): Promise<EmbeddedServer> {
  const fetchImpl = options.fetchImpl || fetch;
  const store = new ConversationStore(options.storeFile);
  const log = options.log || (() => undefined);
  const sessionFile = path.join(path.dirname(options.storeFile), "router-session.json");
  let consoleToken: string | null = readSessionToken(sessionFile);

  /* Project / skill / workflow state mirrors the desktop's SQLite tables in
     small JSON files beside the conversation store: same routes, same shapes,
     no schema migration. Conversations carry an optional project_id so the
     active project filters threads exactly like the desktop does. */
  interface ProjectRecord {
    id: string;
    name: string;
    folder: string;
    folder_path?: string;
    created_at: number;
    updated_at: number;
    archived: boolean;
  }
  const projectsStore = new JsonFileStore<{ projects: ProjectRecord[]; active: string }>(
    path.join(path.dirname(options.storeFile), "projects.json"),
    { projects: [], active: "" },
  );
  const skillsState = new JsonFileStore<{ disabled: string[] }>(
    path.join(path.dirname(options.storeFile), "skills-state.json"),
    { disabled: [] },
  );
  const workflowsStore = new JsonFileStore<{ workflows: Array<{ id: string; name: string; prompt: string; skill: string }> }>(
    path.join(path.dirname(options.storeFile), "workflows.json"),
    { workflows: [] },
  );
  const activeProject = (): string | null => projectsStore.read().active || null;

  function skillRoots(): Array<{ source: string; dir: string }> {
    const roots = [{ source: "built-in", dir: path.join(options.uiDir, "skills") }];
    if (options.workspaceRoot) roots.push({ source: "project", dir: path.join(options.workspaceRoot, "skills") });
    const home = homeDir();
    if (home) {
      roots.push({ source: "user", dir: path.join(home, ".config", "free-ai-studio", "skills") });
      roots.push({ source: "agents", dir: path.join(home, ".agents", "skills") });
    }
    return roots;
  }

  const routerPort = () => options.routerPort || 3001;
  const routerOrigin = () => `http://127.0.0.1:${routerPort()}`;

  async function desktopSession(): Promise<string> {
    const response = await fetchImpl(`${routerOrigin()}/api/auth/desktop-session`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
      signal: AbortSignal.timeout(5000),
    });
    const body = (await response.json().catch(() => ({}))) as Record<string, unknown>;
    const token = extractToken(body);
    if (!token) throw new WorkspaceError(401, "This router needs a dashboard sign-in.");
    consoleToken = token;
    writeSessionToken(sessionFile, token);
    return token;
  }

  async function consoleFetch(method: string, adminPath: string, payload: unknown): Promise<{ status: number; contentType: string; body: string }> {
    const token = consoleToken || (await desktopSession());
    const send = async (auth: string) => {
      const headers: Record<string, string> = { Authorization: `Bearer ${auth}`, "x-dashboard-token": auth };
      const init: RequestInit = { method, headers, signal: AbortSignal.timeout(20_000) };
      if (payload !== undefined && payload !== null && method !== "GET") {
        headers["Content-Type"] = "application/json";
        init.body = JSON.stringify(payload);
      }
      return fetchImpl(`${routerOrigin()}/api/${adminPath}`, init);
    };
    let response = await send(token);
    if (response.status === 401) {
      consoleToken = null;
      try {
        fs.rmSync(sessionFile, { force: true });
      } catch {
        // ignore
      }
      response = await send(await desktopSession());
    }
    const contentType = response.headers.get("content-type") || "application/json";
    return { status: response.status, contentType, body: await response.text() };
  }

  function sendJson(response: http.ServerResponse, status: number, payload: unknown, extra: Record<string, string> = {}) {
    const body = JSON.stringify(payload);
    response.writeHead(status, {
      "Content-Type": "application/json",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      ...extra,
    });
    response.end(body);
  }

  function serveStatic(response: http.ServerResponse, urlPath: string) {
    const decoded = decodeURIComponent(urlPath.split("?")[0]);
    const relative = decoded === "/" ? "index.html" : decoded.replace(/^\/static\//, "").replace(/^\/+/, "");
    const target = path.join(options.uiDir, relative);
    const root = path.resolve(options.uiDir);
    const resolved = path.resolve(target);
    if (!resolved.startsWith(root + path.sep) && resolved !== root) {
      sendJson(response, 403, { ok: false, error: "Not found." });
      return;
    }
    let bytes: Buffer;
    try {
      bytes = fs.readFileSync(resolved);
    } catch {
      sendJson(response, 404, { ok: false, error: "Not found." });
      return;
    }
    response.writeHead(200, {
      "Content-Type": CONTENT_TYPES[path.extname(resolved).toLowerCase()] || "application/octet-stream",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      // The desktop UI is framed by the editor webview; nothing else may frame it.
      "Content-Security-Policy":
        "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'self' vscode-webview:",
    });
    response.end(bytes);
  }

  async function readBody(request: http.IncomingMessage): Promise<Record<string, unknown>> {
    const chunks: Buffer[] = [];
    let size = 0;
    for await (const chunk of request) {
      size += chunk.length;
      if (size > 5 * 1024 * 1024) throw new WorkspaceError(413, "Request body is too large.");
      chunks.push(chunk as Buffer);
    }
    if (!chunks.length) return {};
    try {
      return JSON.parse(Buffer.concat(chunks).toString("utf8")) as Record<string, unknown>;
    } catch {
      throw new WorkspaceError(400, "Expected a JSON body.");
    }
  }

  /* One chat turn: persist the user message, run the selected skill/mode
     through the packed context, call the router (honouring a per-message
     model override), persist the reply, log usage. Throws with `status` and
     `meta` so both chat routes can render their own response shape. */
  async function runChatTurn(body: Record<string, unknown>): Promise<{ answer: string; meta: Record<string, unknown> }> {
    const conversationId = String(body.conversation_id || "");
    const message = String(body.message || "").trim();
    const code = String(body.code || "").slice(0, 50_000);
    const language = String(body.language || "text").slice(0, 40);
    const fail = (status: number, reason: string): never => {
      const error = new Error(reason) as Error & { status?: number };
      error.status = status;
      throw error;
    };
    if (!conversationId || !message) fail(400, "Choose a conversation and enter a message.");
    if (!store.has(conversationId)) fail(404, "Conversation not found.");

    let prompt = code.trim()
      ? `${message}\n\nHere is the code from my editor (language: ${language}):\n\`\`\`${language}\n${code}\n\`\`\``
      : message;

    // The picked skill rides with this message only (desktop prepare_chat parity).
    const skillId = String(body.skill || "").trim();
    if (skillId) {
      const skill = discoverSkills(skillRoots()).find((entry) => entry.id === skillId);
      if (skill && !skill.error && skill.body.trim()) {
        prompt = `Follow the skill "${skill.name}" for this message:\n\n${skill.body}\n\n---\n\n${prompt}`;
      }
    }
    // Plan mode: reply with a plan, change nothing.
    if (String(body.mode || "") === "plan") {
      prompt = `Plan mode: do not make changes this reply. Answer with a short,\nordered implementation plan (files, steps, risks) the user can approve\nbefore anything is built.\n\n---\n\n${prompt}`;
    }

    store.append(conversationId, "user", prompt);
    const config = options.getConfig();
    const history = store
      .messages(conversationId)
      .slice(-Math.max(config.historyMessages, 2))
      .map(({ role, content }) => ({ role, content }));
    const outline = options.workspaceRoot && config.sendWorkspaceOutline ? workspaceOutline(options.workspaceRoot) : null;
    const packed = packMessages({
      system: SYSTEM_PROMPT,
      outline,
      history,
      budgetTokens: config.contextBudgetTokens,
      historyMessages: config.historyMessages,
    });
    const modelOverride = String(body.model || "").trim();
    try {
      const reply = await callRouter(config, packed.messages, fetchImpl, modelOverride || undefined);
      store.append(conversationId, "assistant", reply.answer);
      store.logRequest({
        created_at: Date.now() / 1000,
        conversation_id: conversationId,
        model: String(reply.meta.model || modelOverride || config.model),
        provider: String(reply.meta.provider || "router"),
        status: "ok",
        latency_ms: Number(reply.meta.latency_ms || 0),
        prompt_tokens: (reply.meta.prompt_tokens as number) ?? null,
        completion_tokens: (reply.meta.completion_tokens as number) ?? null,
        total_tokens: (reply.meta.total_tokens as number) ?? null,
        error: "",
      });
      return {
        answer: reply.answer,
        meta: { ...reply.meta, context_tokens: packed.estimatedTokens, context_trimmed: packed.trimmedMessages },
      };
    } catch (error) {
      const failure = error as Error & { latency?: number };
      store.logRequest({
        created_at: Date.now() / 1000,
        conversation_id: conversationId,
        model: modelOverride || config.model,
        provider: "router / unknown",
        status: "error",
        latency_ms: failure.latency || 0,
        prompt_tokens: null,
        completion_tokens: null,
        total_tokens: null,
        error: failure.message.slice(0, 500),
      });
      const wrapped = new Error(failure.message) as Error & { status?: number; meta?: Record<string, unknown> };
      wrapped.status = 502;
      wrapped.meta = { latency_ms: failure.latency || 0 };
      throw wrapped;
    }
  }

  const handler = async (request: http.IncomingMessage, response: http.ServerResponse) => {
    const url = new URL(request.url || "/", "http://127.0.0.1");
    const method = (request.method || "GET").toUpperCase();
    const routePath = url.pathname;

    try {
      if (!isLoopbackUrl(`http://${request.headers.host || "127.0.0.1"}`) && !(request.headers.host || "").startsWith("127.0.0.1")) {
        const host = (request.headers.host || "").split(":")[0];
        if (!["127.0.0.1", "localhost", "[::1]"].includes(host)) {
          sendJson(response, 403, { ok: false, error: "This server only answers on loopback." });
          return;
        }
      }

      if (method === "GET" && (routePath === "/" || routePath.startsWith("/static/"))) {
        serveStatic(response, routePath);
        return;
      }

      if (routePath === "/api/health" && method === "GET") {
        sendJson(response, 200, {
          ok: true,
          mode: "embedded",
          embeddable: true,
          workspace: options.workspaceRoot || null,
          version: "1.19.0",
        });
        return;
      }

      if (routePath === "/api/config") {
        if (method === "GET") {
          const config = options.getConfig();
          sendJson(response, 200, {
            base_url: config.routerBaseUrl,
            model: config.model,
            has_api_key: Boolean(config.apiKey),
            context_budget_tokens: config.contextBudgetTokens,
            history_messages: config.historyMessages,
            send_workspace_outline: config.sendWorkspaceOutline,
            embedded: true,
          });
          return;
        }
        sendJson(response, 400, { ok: false, error: "Change these values in VS Code settings (Free AI Studio)." });
        return;
      }

      if (routePath === "/api/conversations" && method === "GET") {
        sendJson(response, 200, store.list(activeProject()));
        return;
      }
      if (routePath === "/api/conversations" && method === "POST") {
        const body = await readBody(request);
        sendJson(response, 200, store.create(String(body.title || "New thread"), activeProject()));
        return;
      }

      const messageMatch = routePath.match(/^\/api\/conversations\/([^/]+)\/messages$/);
      if (messageMatch && method === "GET") {
        const id = decodeURIComponent(messageMatch[1]);
        if (!store.has(id)) {
          sendJson(response, 404, { ok: false, error: "Conversation not found." });
          return;
        }
        sendJson(response, 200, store.messages(id).map(({ role, content }) => ({ role, content })));
        return;
      }

      const deleteMatch = routePath.match(/^\/api\/conversations\/([^/]+)\/delete$/);
      if (deleteMatch && method === "POST") {
        const removed = store.remove(decodeURIComponent(deleteMatch[1]));
        sendJson(response, removed ? 200 : 404, removed ? { ok: true } : { ok: false, error: "Conversation not found." });
        return;
      }

      if (routePath === "/api/usage" && method === "GET") {
        sendJson(response, 200, store.usage(url.searchParams));
        return;
      }

      /* skills ---------------------------------------------------- */
      if (routePath === "/api/skills" && method === "GET") {
        const disabled = new Set(skillsState.read().disabled);
        const skills = discoverSkills(skillRoots()).map(({ id, name, description, path: skillPath, source, error }) => ({
          id,
          name,
          description,
          path: skillPath,
          source,
          error,
          enabled: !disabled.has(id),
        }));
        sendJson(response, 200, { ok: true, skills });
        return;
      }

      const skillStateMatch = routePath.match(/^\/api\/skills\/([^/]+)\/state$/);
      if (skillStateMatch && method === "POST") {
        const body = await readBody(request);
        const id = decodeURIComponent(skillStateMatch[1]);
        const enabled = body.enabled !== false;
        const state = skillsState.read();
        const disabled = new Set(state.disabled);
        if (enabled) disabled.delete(id);
        else disabled.add(id);
        skillsState.write({ disabled: [...disabled] });
        sendJson(response, 200, { ok: true, id, enabled });
        return;
      }

      const skillMatch = routePath.match(/^\/api\/skills\/([^/]+)$/);
      if (skillMatch && method === "GET") {
        const id = decodeURIComponent(skillMatch[1]);
        const skill = discoverSkills(skillRoots()).find((entry) => entry.id === id);
        if (!skill) {
          sendJson(response, 404, { ok: false, error: "No such skill." });
          return;
        }
        const enabled = !skillsState.read().disabled.includes(id);
        if (skill.error) {
          sendJson(response, 422, { ok: false, error: skill.error });
          return;
        }
        sendJson(response, 200, {
          ok: true,
          skill: { id, name: skill.name, description: skill.description, body: skill.body, path: skill.path, source: skill.source, enabled },
        });
        return;
      }

      /* projects (desktop parity; JSON file instead of SQLite) ------- */
      if (routePath === "/api/projects" && method === "GET") {
        sendJson(response, 200, { ok: true, active: activeProject(), projects: projectsStore.read().projects });
        return;
      }
      if (routePath === "/api/projects" && method === "POST") {
        const body = await readBody(request);
        const name = cleanTitle(String(body.name || ""));
        if (!name) {
          sendJson(response, 400, { ok: false, error: "A project needs a name." });
          return;
        }
        const folder = String(body.folder ?? body.folder_path ?? "").slice(0, 4000);
        const now = Date.now() / 1000;
        const project: ProjectRecord = {
          id: randomUUID(), name, folder, folder_path: folder,
          created_at: now, updated_at: now, archived: false,
        };
        const data = projectsStore.read();
        data.projects.push(project);
        if (!data.active) data.active = project.id;
        projectsStore.write(data);
        sendJson(response, 201, { ok: true, id: project.id, name, project });
        return;
      }

      const projectActivate = routePath.match(/^\/api\/projects\/([^/]+)\/activate$/);
      if (projectActivate && method === "POST") {
        const id = decodeURIComponent(projectActivate[1]);
        const data = projectsStore.read();
        const project = data.projects.find((entry) => entry.id === id);
        if (!project) {
          sendJson(response, 404, { ok: false, error: "No such project." });
          return;
        }
        data.active = id;
        projectsStore.write(data);
        // The folder still belongs to VS Code's own open folder.
        const workspace = project.folder && options.workspaceRoot && path.resolve(project.folder) === path.resolve(options.workspaceRoot)
          ? { open: true, name: path.basename(options.workspaceRoot), path: options.workspaceRoot }
          : { open: false };
        sendJson(response, 200, { ok: true, id, name: project.name, workspace });
        return;
      }

      const projectUpdate = routePath.match(/^\/api\/projects\/([^/]+)\/update$/);
      if (projectUpdate && method === "POST") {
        const id = decodeURIComponent(projectUpdate[1]);
        const body = await readBody(request);
        const data = projectsStore.read();
        const project = data.projects.find((entry) => entry.id === id);
        if (!project) {
          sendJson(response, 404, { ok: false, error: "No such project." });
          return;
        }
        if (typeof body.name === "string" && cleanTitle(body.name)) project.name = cleanTitle(body.name);
        if (typeof body.folder === "string") { project.folder = body.folder.slice(0, 4000); project.folder_path = project.folder; }
        if (typeof body.folder_path === "string") { project.folder = body.folder_path.slice(0, 4000); project.folder_path = project.folder; }
        if (typeof body.archived === "boolean") {
          project.archived = body.archived;
          // Never leave the UI pointed at an archived project.
          if (body.archived && data.active === id) data.active = "";
        }
        project.updated_at = Date.now() / 1000;
        projectsStore.write(data);
        sendJson(response, 200, { ok: true, id });
        return;
      }

      const projectDelete = routePath.match(/^\/api\/projects\/([^/]+)\/delete$/);
      if (projectDelete && method === "POST") {
        const id = decodeURIComponent(projectDelete[1]);
        const data = projectsStore.read();
        const before = data.projects.length;
        data.projects = data.projects.filter((entry) => entry.id !== id);
        if (data.projects.length === before) {
          sendJson(response, 404, { ok: false, error: "No such project." });
          return;
        }
        if (data.active === id) data.active = "";
        projectsStore.write(data);
        // Threads cascade with their project, like the desktop's foreign key.
        store.removeProject(id);
        sendJson(response, 200, { ok: true, active: activeProject() });
        return;
      }

      /* workflows --------------------------------------------------- */
      if (routePath === "/api/workflows" && method === "GET") {
        sendJson(response, 200, { ok: true, workflows: workflowsStore.read().workflows });
        return;
      }
      if (routePath === "/api/workflows" && method === "POST") {
        const body = await readBody(request);
        const name = cleanTitle(String(body.name || ""));
        const prompt = String(body.prompt || "").slice(0, 4000);
        const skill = String(body.skill || "").slice(0, 160);
        if (!name || !prompt) {
          sendJson(response, 400, { ok: false, error: "A workflow needs a name and a prompt." });
          return;
        }
        const data = workflowsStore.read();
        const requestedId = String(body.id || "");
        const known = requestedId && data.workflows.some((entry) => entry.id === requestedId);
        const id = known ? requestedId : randomUUID();
        data.workflows = data.workflows.filter((entry) => entry.id !== id);
        data.workflows.push({ id, name, prompt, skill });
        data.workflows.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
        workflowsStore.write(data);
        sendJson(response, 200, { ok: true, id });
        return;
      }
      const workflowDelete = routePath.match(/^\/api\/workflows\/([^/]+)\/delete$/);
      if (workflowDelete && method === "POST") {
        const id = decodeURIComponent(workflowDelete[1]);
        const data = workflowsStore.read();
        const before = data.workflows.length;
        data.workflows = data.workflows.filter((entry) => entry.id !== id);
        if (data.workflows.length === before) {
          sendJson(response, 404, { ok: false, error: "No such workflow." });
          return;
        }
        workflowsStore.write(data);
        sendJson(response, 200, { ok: true });
        return;
      }

      /* model catalog ------------------------------------------------ */
      if (routePath === "/api/models" && method === "GET") {
        const config = options.getConfig();
        const base = normalizeBaseUrl(config.routerBaseUrl);
        try {
          const headers: Record<string, string> = {};
          if (config.apiKey) headers.Authorization = `Bearer ${config.apiKey}`;
          const listResponse = await fetchImpl(`${base}/models`, {
            headers,
            signal: AbortSignal.timeout(10_000),
          });
          const text = await listResponse.text();
          if (!listResponse.ok) {
            sendJson(response, 200, {
              ok: false,
              error: `Router returned HTTP ${listResponse.status} for /models. Saving an API key may unlock the catalog.`,
              models: [],
            });
            return;
          }
          const parsed = JSON.parse(text) as { data?: unknown[] } | unknown[];
          const rows = (Array.isArray(parsed) ? parsed : parsed.data) || [];
          const models = rows
            .filter((row): row is Record<string, unknown> => Boolean(row && typeof row === "object"))
            .map((row) => ({
              id: String(row.id || ""),
              owner: String(row.owned_by || ""),
              context_window:
                Number(
                  row.context_length ??
                    row.context_window ??
                    row.max_input_tokens ??
                    (row.top_provider as { context_length?: number } | undefined)?.context_length ??
                    0,
                ) || 0,
            }))
            .filter((row) => row.id);
          sendJson(response, 200, { ok: true, error: null, models });
        } catch (error) {
          sendJson(response, 200, { ok: false, error: `Router not reachable: ${(error as Error).message}`, models: [] });
        }
        return;
      }

      /* router / gateway lifecycle ---------------------------------- */
      if (routePath === "/api/router" && method === "GET") {
        let reachable = false;
        try {
          const ping = await fetchImpl(`${routerOrigin()}/api/ping`, { signal: AbortSignal.timeout(1500) });
          reachable = ping.ok;
        } catch {
          reachable = false;
        }
        sendJson(response, 200, { reachable, running: reachable, port: routerPort(), managed_by: "desktop" });
        return;
      }
      if (
        (routePath === "/api/router/start" || routePath === "/api/router/stop" ||
          routePath === "/api/omniroute" || routePath === "/api/omniroute/start" || routePath === "/api/omniroute/stop") &&
        method === "POST"
      ) {
        // Honest refusal: process supervision lives in the desktop binary.
        sendJson(response, 400, {
          ok: false,
          error: "Start or stop the gateway from the Saarthi Studio desktop app, or run it yourself in a terminal — the VS Code built-in server does not manage background processes.",
        });
        return;
      }

      /* open a URL in the platform browser --------------------------- */
      if (routePath === "/api/open" && method === "POST") {
        const body = await readBody(request);
        const target = String(body.url || "");
        if (!target.startsWith("http://127.0.0.1:") && !target.startsWith("http://localhost:") && !target.startsWith("https://")) {
          sendJson(response, 400, { ok: false, error: "Only http(s) URLs can be opened." });
          return;
        }
        openInSystemBrowser(target);
        sendJson(response, 200, { ok: true });
        return;
      }

      /* artifacts: every readable file in the open folder -------------- */
      if (routePath === "/api/workspace/files" && method === "GET") {
        sendJson(response, 200, listArtifacts(requireRoot(options)));
        return;
      }

      /* terminal: no PTY in the extension host — fail loudly, not silently */
      if (routePath.startsWith("/api/terminal/")) {
        sendJson(response, 501, {
          ok: false,
          error: "The built-in terminal is not available in VS Code's built-in server. Use VS Code's own terminal, or the Saarthi Studio desktop app for the project terminal.",
        });
        return;
      }

      if (routePath === "/api/test" && method === "POST") {
        const config = options.getConfig();
        try {
          const reply = await callRouter(
            config,
            [
              { role: "system", content: "Reply with the single word OK." },
              { role: "user", content: "ping" },
            ],
            fetchImpl,
          );
          sendJson(response, 200, { ok: true, message: `Router answered with ${reply.meta.model}.` });
        } catch (error) {
          sendJson(response, 502, { ok: false, error: (error as Error).message });
        }
        return;
      }

      /* chat: one shared turn runner; /api/chat answers JSON, /api/chat/stream
         (the UI's default) answers SSE frames with the same result. */
      if (routePath === "/api/chat" && method === "POST") {
        const body = await readBody(request);
        try {
          const result = await runChatTurn(body);
          sendJson(response, 200, { ok: true, answer: result.answer, meta: result.meta });
        } catch (error) {
          const failure = error as Error & { status?: number; meta?: Record<string, unknown> };
          sendJson(response, failure.status || 502, {
            ok: false,
            error: failure.message,
            meta: failure.meta || {},
          });
        }
        return;
      }

      if (routePath === "/api/chat/stream" && method === "POST") {
        const body = await readBody(request);
        response.writeHead(200, {
          "Content-Type": "text/event-stream; charset=utf-8",
          "Cache-Control": "no-store",
          "X-Content-Type-Options": "nosniff",
        });
        const frame = (payload: Record<string, unknown>) => {
          response.write(`data: ${JSON.stringify(payload)}\n\n`);
        };
        try {
          const result = await runChatTurn(body);
          if (result.answer) frame({ delta: result.answer });
          frame({ done: true, meta: result.meta });
        } catch (error) {
          const failure = error as Error & { meta?: Record<string, unknown> };
          frame({ error: failure.message, meta: failure.meta || {} });
          frame({ done: true });
        }
        response.end();
        return;
      }

      /* console bridge ------------------------------------------------ */
      if (routePath === "/api/router/console/status" && method === "GET") {
        try {
          const ping = await fetchImpl(`${routerOrigin()}/api/ping`, { signal: AbortSignal.timeout(1500) });
          if (!ping.ok) throw new Error("router offline");
          try {
            await consoleFetch("GET", "keys", undefined);
            sendJson(response, 200, { ok: true, reachable: true, authenticated: true, mode: "ready", message: "Router console is connected." });
          } catch {
            sendJson(response, 200, { ok: true, reachable: true, authenticated: false, mode: "login", message: "Sign in with the router dashboard account." });
          }
        } catch {
          sendJson(response, 200, { ok: true, reachable: false, authenticated: false, mode: "offline", message: "The router is not running." });
        }
        return;
      }

      if (routePath === "/api/router/console/login" && method === "POST") {
        const body = await readBody(request);
        const loginResponse = await fetchImpl(`${routerOrigin()}/api/auth/login`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ email: body.email, password: body.password }),
          signal: AbortSignal.timeout(8000),
        });
        const payload = (await loginResponse.json().catch(() => ({}))) as Record<string, unknown>;
        const token = extractToken(payload);
        if (!token) {
          sendJson(response, 401, { ok: false, error: "The router rejected those credentials." });
          return;
        }
        consoleToken = token;
        writeSessionToken(sessionFile, token);
        sendJson(response, 200, { ok: true });
        return;
      }

      if (routePath.startsWith("/api/router/console/")) {
        const adminPath = sanitizeConsolePath(routePath.slice("/api/router/console/".length));
        let payload: unknown;
        if (method !== "GET" && method !== "HEAD") payload = await readBody(request);
        try {
          const result = await consoleFetch(method, adminPath, payload);
          response.writeHead(result.status, { "Content-Type": result.contentType, "Cache-Control": "no-store" });
          response.end(result.body);
        } catch (error) {
          sendJson(response, 401, { ok: false, error: (error as Error).message });
        }
        return;
      }

      /* workspace ----------------------------------------------------- */
      if (routePath === "/api/workspace" && method === "GET") {
        const root = options.workspaceRoot || null;
        sendJson(response, 200, {
          workspace: root ? { path: root, absolute_path: root, name: path.basename(root) } : null,
          recent: root ? [root] : [],
          home: root || ".",
        });
        return;
      }

      if (routePath === "/api/workspace/open" && method === "POST") {
        const body = await readBody(request);
        const requested = String(body.path || "");
        if (!options.workspaceRoot) {
          sendJson(response, 400, { ok: false, error: "Open a folder in VS Code; the extension server follows the editor workspace." });
          return;
        }
        if (requested && path.resolve(requested) !== path.resolve(options.workspaceRoot)) {
          sendJson(response, 400, { ok: false, error: "This server can only serve the folder open in VS Code." });
          return;
        }
        sendJson(response, 200, {
          ok: true,
          workspace: { path: options.workspaceRoot, absolute_path: options.workspaceRoot, name: path.basename(options.workspaceRoot) },
          recent: [options.workspaceRoot],
        });
        return;
      }

      if (routePath === "/api/workspace/pick" && method === "POST") {
        sendJson(response, 400, { ok: false, picked: false, error: "Use VS Code's own File → Open Folder to switch projects." });
        return;
      }

      if (routePath === "/api/workspace/close" && method === "POST") {
        sendJson(response, 200, { ok: true });
        return;
      }

      if (routePath === "/api/workspace/entries" && method === "GET") {
        const root = requireRoot(options);
        sendJson(response, 200, listDir(root, url.searchParams.get("path") || ""));
        return;
      }

      if (routePath === "/api/workspace/file") {
        const root = requireRoot(options);
        if (method === "GET") {
          sendJson(response, 200, readTextFile(root, url.searchParams.get("path") || ""));
          return;
        }
        if (method === "PUT" || method === "POST") {
          const body = await readBody(request);
          sendJson(response, 200, writeTextFile(root, String(body.path || ""), String(body.content ?? "")));
          return;
        }
      }

      if (routePath === "/api/workspace/create" && method === "POST") {
        const root = requireRoot(options);
        const body = await readBody(request);
        sendJson(response, 200, createEntry(root, String(body.path || ""), body.dir === true));
        return;
      }

      if (routePath === "/api/workspace/rename" && method === "POST") {
        const root = requireRoot(options);
        const body = await readBody(request);
        sendJson(response, 200, renameEntry(root, String(body.from || ""), String(body.to || "")));
        return;
      }

      if (routePath === "/api/workspace/delete" && method === "POST") {
        const root = requireRoot(options);
        const body = await readBody(request);
        sendJson(response, 200, deleteEntry(root, String(body.path || "")));
        return;
      }

      sendJson(response, 404, { ok: false, error: `No route for ${method} ${routePath}` });
    } catch (error) {
      if (error instanceof WorkspaceError) {
        sendJson(response, error.status, { ok: false, error: error.message });
      } else {
        log(`request failed: ${(error as Error).message}`);
        sendJson(response, 500, { ok: false, error: (error as Error).message });
      }
    }
  };

  const server = http.createServer((request, response) => {
    void handler(request, response);
  });

  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  return {
    server,
    port,
    url: `http://127.0.0.1:${port}`,
    close: () =>
      new Promise<void>((resolve) => {
        // Keep-alive sockets would otherwise hold the port until they time out.
        server.closeAllConnections?.();
        server.close(() => resolve());
      }),
  };
}

function requireRoot(options: EmbeddedServerOptions): string {
  if (!options.workspaceRoot) {
    throw new WorkspaceError(400, "Open a folder in VS Code to browse files.");
  }
  return options.workspaceRoot;
}

export function workspaceOutline(root: string): string {
  const names: string[] = [];
  const walk = (dir: string, depth: number) => {
    if (depth > 3 || names.length > 300) return;
    let items: fs.Dirent[];
    try {
      items = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const item of items) {
      if (names.length > 300) return;
      if (item.name === "node_modules" || item.name === ".git" || item.name === "target") continue;
      const relative = path.relative(root, path.join(dir, item.name)).split(path.sep).join("/");
      names.push(item.isDirectory() ? `${relative}/` : relative);
      if (item.isDirectory()) walk(path.join(dir, item.name), depth + 1);
    }
  };
  walk(root, 0);
  return outlineFromEntries(path.basename(root), names);
}

function extractToken(value: unknown): string | null {
  const keys = ["token", "sessionToken", "session_token", "session", "accessToken", "access_token"];
  const walk = (node: unknown, depth: number): string | null => {
    if (depth > 3 || !node || typeof node !== "object") return null;
    if (Array.isArray(node)) {
      for (const item of node) {
        const found = walk(item, depth + 1);
        if (found) return found;
      }
      return null;
    }
    const record = node as Record<string, unknown>;
    for (const key of keys) {
      const candidate = record[key];
      if (typeof candidate === "string" && candidate.trim()) return candidate.trim();
    }
    for (const item of Object.values(record)) {
      const found = walk(item, depth + 1);
      if (found) return found;
    }
    return null;
  };
  return walk(value, 0);
}

function readSessionToken(file: string): string | null {
  try {
    const raw = JSON.parse(fs.readFileSync(file, "utf8")) as { token?: string };
    return raw.token || null;
  } catch {
    return null;
  }
}

function writeSessionToken(file: string, token: string): void {
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify({ token, saved_at: Date.now() / 1000 }), { mode: 0o600 });
  } catch {
    // A failed write only means the next start needs a new session.
  }
}
