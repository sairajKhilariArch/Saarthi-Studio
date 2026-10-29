// Pure logic for Saarthi Studio agent runs in the VS Code extension: the
// bounded, permissioned autonomous loop, its persistence, and the outbound
// privacy policy. Nothing here imports `vscode`, so `node --test` exercises
// all of it. Mirrors app/src/agent.rs and app/src/privacy.rs.

import * as fs from "node:fs";
import * as path from "node:path";
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";

/* Constants mirror the desktop's agent.rs. */
export const DEFAULT_MAX_STEPS = 12;
export const HARD_MAX_STEPS = 30;
const STEP_OUTPUT_CAP = 4_000;
const COMMAND_TIMEOUT_MS = 90_000;
const HISTORY_STEPS = 40;
const CHECKPOINT_CAP = 200_000;
const DIFF_CAP = 8_000;

export const RUNNING = "running";
export const AWAITING_APPROVAL = "awaiting_approval";
export const SUCCEEDED = "succeeded";
export const FAILED = "failed";
export const CANCELLED = "cancelled";
export const INTERRUPTED = "interrupted";

/* ------------------------------------------------------------------ */
/* actions                                                             */

export interface AgentAction {
  action: "think" | "list" | "read" | "write" | "command" | "done" | "fail";
  summary?: string;
  reason?: string;
  path?: string;
  content?: string;
  command?: string;
}

export function actionTag(action: AgentAction): string {
  return action.action;
}

export function pendingSummary(action: AgentAction): string {
  switch (action.action) {
    case "write":
      return `write ${action.path} (${(action.content || "").length} bytes)`;
    case "command":
      return `run ${action.command}`;
    case "read":
      return `read ${action.path}`;
    case "list":
      return `list ${action.path || "."}`;
    default:
      return "";
  }
}

export function parseAction(reply: string): { ok: true; action: AgentAction } | { ok: false; error: string } {
  const trimmed = String(reply || "").trim();
  if (!trimmed.startsWith("{")) {
    if (trimmed) return { ok: true, action: { action: "think", summary: trimmed.slice(0, 300) } };
    return { ok: false, error: "empty reply" };
  }
  try {
    const value = JSON.parse(trimmed) as Record<string, unknown>;
    const tag = value.action;
    if (typeof tag !== "string") return { ok: false, error: 'missing "action"' };
    const field = (name: string): string => {
      const got = value[name];
      if (typeof got !== "string") throw new Error(`missing "${name}"`);
      return got;
    };
    switch (tag) {
      case "think": return { ok: true, action: { action: "think", summary: field("summary") } };
      case "list": return { ok: true, action: { action: "list", path: field("path") } };
      case "read": return { ok: true, action: { action: "read", path: field("path") } };
      case "write": return { ok: true, action: { action: "write", path: field("path"), content: field("content") } };
      case "command": return { ok: true, action: { action: "command", command: field("command") } };
      case "done": return { ok: true, action: { action: "done", summary: field("summary") } };
      case "fail": return { ok: true, action: { action: "fail", reason: field("reason") } };
      default: return { ok: false, error: `unknown action "${tag}"` };
    }
  } catch (error) {
    return { ok: false, error: `invalid action: ${(error as Error).message}` };
  }
}

/* ------------------------------------------------------------------ */
/* privacy (app/src/privacy.rs port)                                   */

const KEY_NAMES = ["api_key", "apikey", "api-key", "secret", "token", "password", "passwd", "access_key", "private_key"];

/** Scan text and mask any value that looks like a credential. */
export function sanitize(text: string): { text: string; findings: string[] } {
  const findings = new Set<string>();
  let insideKeyBlock = false;
  const masked = String(text || "").split("\n").map((line) => {
    const trimmed = line.trim();
    if (trimmed.startsWith("-----BEGIN")) {
      insideKeyBlock = true;
      findings.add("pem");
      return line; // headers are boilerplate, not secret
    }
    if (trimmed.startsWith("-----END")) {
      insideKeyBlock = false;
      return line;
    }
    if (insideKeyBlock) {
      findings.add("pem");
      return "[masked: pem]";
    }
    return scanPlainLine(line, findings);
  });
  return { text: masked.join("\n"), findings: [...findings] };
}

function scanPlainLine(line: string, findings: Set<string>): string {
  const lower = line.toLowerCase();
  let out = line;
  if (lower.includes("private key")) findings.add("pem");
  if ((lower.includes("bearer") || lower.includes("authorization")) && hasTokenRun(lower)) {
    out = maskRuns(out);
    findings.add("bearer");
  }
  if (lower.includes("akia") && hasTokenRun(lower.slice(lower.indexOf("akia")))) {
    out = maskRuns(out);
    findings.add("aws");
  }
  if (lower.startsWith("eyj")) {
    let count = 0;
    let index = lower.indexOf("eyj");
    while (index !== -1) {
      count += 1;
      index = lower.indexOf("eyj", index + 1);
    }
    if (count >= 2) {
      out = maskRuns(out);
      findings.add("jwt");
    }
  }
  // The finding must follow the mask: a line only reports key-assignment
  // when a value was actually removed from it.
  const afterAssign = maskAssignments(out);
  if (afterAssign !== out) {
    out = afterAssign;
    findings.add("key-assignment");
  }
  return out;
}

function hasTokenRun(text: string): boolean {
  let run = 0;
  for (const ch of text) {
    if (/[a-zA-Z0-9\-_./+=#]/.test(ch)) {
      run += 1;
      if (run >= 24) return true;
    } else {
      run = 0;
    }
  }
  return false;
}

function maskRuns(text: string): string {
  return text.replace(/[a-zA-Z0-9\-_./+=#]{24,}/g, "[masked]");
}

function maskAssignments(text: string): string {
  // Mask the value side of every KEY_NAMES assignment whose value is long.
  // Line oriented: values never span lines, unlike the masked runs above.
  for (;;) {
    const result = maskOneAssignment(text);
    if (result === null) return text;
    text = result;
  }
}

function maskOneAssignment(text: string): string | null {
  // Mirrors privacy.rs mask_assignments_in: find the key name anywhere
  // (case-insensitive), require the next char not to extend the word, then
  // skip whitespace/quotes to '=' or ':', skip again, and mask the value
  // when it holds at least 16 non-whitespace characters.
  const lower = text.toLowerCase();
  for (const name of KEY_NAMES) {
    let from = 0;
    for (;;) {
      const pos = lower.indexOf(name, from);
      if (pos === -1) break;
      const after = pos + name.length;
      const next = after < text.length ? text.charAt(after) : "";
      if (next && /[a-zA-Z0-9_]/.test(next)) {
        from = after; // "keyhole" is not a key named "key"
        continue;
      }
      let cursor = after;
      while (cursor < text.length && /[\s"]/.test(text.charAt(cursor))) cursor += 1;
      if (cursor >= text.length || (text.charAt(cursor) !== "=" && text.charAt(cursor) !== ":")) {
        from = after;
        continue;
      }
      cursor += 1;
      while (cursor < text.length && /[\s"]/.test(text.charAt(cursor))) cursor += 1;
      const newline = text.indexOf("\n", cursor);
      const lineEnd = newline === -1 ? text.length : newline;
      const value = text.slice(cursor, lineEnd);
      if (value.replace(/\s/g, "").length < 16) {
        from = after;
        continue;
      }
      return `${text.slice(0, cursor)}[masked]${text.slice(lineEnd)}`;
    }
  }
  return null;
}

/** True when a workspace-relative path is on the user's exclusion list. */
export function isExcluded(pathTarget: string, exclusions: string[]): boolean {
  return exclusions.some((rule) => {
    const clean = String(rule || "").trim().replace(/^\.\//, "").replace(/\/+$/, "");
    if (!clean || clean === ".") return false;
    const target = String(pathTarget || "").replace(/^\.\//, "");
    return target === clean || target.startsWith(`${clean}/`);
  });
}

/** Human-readable summary of what the outbound policy changed, or null. */
export function noticeText(findings: string[], excludedPaths: string[]): string | null {
  const parts = [];
  if (findings.length) parts.push(`possible credentials masked before sending (${findings.length}: ${findings.join(", ")})`);
  if (excludedPaths.length) parts.push(`excluded files were not sent: ${excludedPaths.join(", ")}`);
  return parts.length ? parts.join("; ") : null;
}

/* ------------------------------------------------------------------ */
/* diff + prompt building                                              */

export function capChars(text: string, limit: number): string {
  const value = String(text || "");
  const chars = Array.from(value);
  if (chars.length <= limit) return value;
  return `${chars.slice(0, limit).join("")}… (output truncated)`;
}

export function unifiedDiff(oldText: string | null, next: string, filePath: string): string {
  const CONTEXT = 3;
  const oldLines = oldText === null ? [] : String(oldText || "").split("\n");
  const newLines = String(next || "").split("\n");
  if (oldText !== null && oldText === next) return "(no changes — the new content is identical)";
  let start = 0;
  while (start < oldLines.length && start < newLines.length && oldLines[start] === newLines[start]) start += 1;
  let endOld = oldLines.length;
  let endNew = newLines.length;
  while (endOld > start && endNew > start && oldLines[endOld - 1] === newLines[endNew - 1]) {
    endOld -= 1;
    endNew -= 1;
  }
  const ctxStart = Math.max(0, start - CONTEXT);
  const ctxEndOld = Math.min(endOld + CONTEXT, oldLines.length);
  const ctxEndNew = Math.min(endNew + CONTEXT, newLines.length);
  const head = `--- ${filePath} (${oldText === null ? "new file" : "before"})\n+++ ${filePath} (after)\n@@ -${oldText === null ? 0 : ctxStart + 1},${ctxEndOld - ctxStart} +${ctxStart + 1},${ctxEndNew - ctxStart} @@\n`;
  const lines = [
    ...oldLines.slice(ctxStart, start).map((line) => ` ${line}`),
    ...oldLines.slice(start, endOld).map((line) => `-${line}`),
    ...newLines.slice(start, endNew).map((line) => `+${line}`),
    ...oldLines.slice(endOld, ctxEndOld).map((line) => ` ${line}`),
  ];
  return capChars(`${head}${lines.join("\n")}\n`, DIFF_CAP);
}

/** The standing instruction plus the goal and capped history. */
export function buildMessages(run: { goal: string; allowWrites: boolean; allowCommands: boolean; autoApprove: boolean }, history: string): Array<Record<string, string>> {
  const writeRule = run.allowWrites ? "write, list" : "list (write FORBIDDEN — never emit it)";
  const commandRule = !run.allowCommands
    ? "command FORBIDDEN — never emit it"
    : run.autoApprove ? "command" : "command (each command first waits for user approval)";
  const system = `You are the Saarthi Studio autonomous coding agent working inside an open ` +
    `workspace folder. You complete the task one step at a time: inspect, plan, ` +
    `edit, verify, and keep going until the goal is actually met. Reply with ` +
    `EXACTLY ONE JSON object and nothing else — no markdown fences, no prose. Choose one:\n` +
    `{"action":"think","summary":"..."} next thought, open question or plan\n` +
    `{"action":"list","path":"relative/path"} inspect the folder\n` +
    `{"action":"read","path":"relative/path"} read a file (always read before editing)\n` +
    `{"action":"write","path":"relative/path","content":"full file text"} create or overwrite a file\n` +
    `{"action":"command","command":"build/test command"} run a shell command in the workspace\n` +
    `{"action":"done","summary":"..."} the goal is met and you verified it\n` +
    `{"action":"fail","reason":"..."} you cannot meet the goal\n\n` +
    `Permitted tools for this run: read: always; write: ${writeRule}; command: ${commandRule}.`;
  const user = `Workspace goal:\n${run.goal}\n\nSteps so far:\n${history}\n\n` +
    `Choose the next single step as one JSON action. If a previous step errored, ` +
    `diagnose it and repair your approach; if you already verified success, emit done.`;
  return [{ role: "system", content: system }, { role: "user", content: user }];
}

/** Logged steps, newest first with a cap; outputs are privacy-masked. */
export function buildHistory(steps: Array<{ seq: number; action: string; summary: string; output: string; }>): string {
  const recent = steps
    .slice()
    .sort((a, b) => b.seq - a.seq)
    .slice(0, HISTORY_STEPS)
    .reverse();
  const lines = recent.map((step) => {
    const head = step.summary ? `${step.seq}. [${step.action}] ${step.summary}` : `${step.seq}. [${step.action}]`;
    if (!step.output) return head;
    // Anything a previous step saw is masked before it rides the next prompt.
    return `${head}\n${sanitize(capChars(step.output, 300)).text}`;
  });
  return lines.length ? lines.join("\n") : "(no steps yet)";
}

/* ------------------------------------------------------------------ */
/* run store (beside the conversation store, plain JSON)               */

export interface AgentRun {
  id: string;
  projectId: string;
  goal: string;
  status: string;
  allowWrites: boolean;
  allowCommands: boolean;
  autoApprove: boolean;
  maxSteps: number;
  pendingAction: string; // serialized JSON action or ""
  error: string;
  createdAt: number;
  updatedAt: number;
}

export interface AgentStep {
  id: number;
  runId: string;
  seq: number;
  action: string;
  summary: string;
  output: string;
  checkpoint: string;
  at: number;
}

interface RunsShape {
  runs: AgentRun[];
  steps: AgentStep[];
}

/** JSON-file persistence: same shape as the desktop's SQLite rows. */
export class RunStore {
  private data: RunsShape;

  constructor(private readonly file: string) {
    try {
      const parsed = JSON.parse(fs.readFileSync(file, "utf8")) as Partial<RunsShape>;
      this.data = {
        runs: Array.isArray(parsed.runs) ? parsed.runs : [],
        steps: Array.isArray(parsed.steps) ? parsed.steps : [],
      };
    } catch {
      this.data = { runs: [], steps: [] };
    }
  }

  private save(): void {
    try {
      fs.mkdirSync(path.dirname(this.file), { recursive: true, mode: 0o700 });
      const temp = `${this.file}.tmp`;
      fs.writeFileSync(temp, JSON.stringify(this.data), { mode: 0o600 });
      fs.renameSync(temp, this.file);
    } catch {
      // A failed write only means the next read starts fresh again.
    }
  }

  /** Crash recovery: a run still marked live after a restart never resumed. */
  markInterrupted(): void {
    const stamp = Date.now() / 1000;
    let changed = false;
    for (const run of this.data.runs) {
      if (run.status === RUNNING || run.status === AWAITING_APPROVAL) {
        run.status = INTERRUPTED;
        run.error = "App restarted mid-run.";
        run.updatedAt = stamp;
        changed = true;
      }
    }
    if (changed) this.save();
  }

  create(input: {
    projectId: string; goal: string; allowWrites: boolean; allowCommands: boolean;
    autoApprove: boolean; maxSteps: number;
  }): AgentRun {
    const now = Date.now() / 1000;
    const run: AgentRun = {
      id: randomUUID(),
      projectId: input.projectId,
      goal: input.goal,
      status: RUNNING,
      allowWrites: input.allowWrites,
      allowCommands: input.allowCommands,
      autoApprove: input.autoApprove,
      maxSteps: Math.min(HARD_MAX_STEPS, Math.max(1, Math.round(input.maxSteps || DEFAULT_MAX_STEPS))),
      pendingAction: "",
      error: "",
      createdAt: now,
      updatedAt: now,
    };
    this.data.runs.push(run);
    this.save();
    return run;
  }

  list(limit = 50): AgentRun[] {
    return this.data.runs
      .slice()
      .sort((a, b) => b.createdAt - a.createdAt)
      .slice(0, limit);
  }

  load(id: string): AgentRun | null {
    return this.data.runs.find((run) => run.id === id) || null;
  }

  private saveRun(run: AgentRun): void {
    run.updatedAt = Date.now() / 1000;
    this.save();
  }

  setStatus(id: string, status: string, error: string | null, pending: string | null): AgentRun | null {
    const run = this.load(id);
    if (!run) return null;
    run.status = status;
    run.error = error || "";
    run.pendingAction = pending || "";
    this.saveRun(run);
    return run;
  }

  finish(id: string, status: string, error: string): AgentRun | null {
    return this.setStatus(id, status, error, "");
  }

  stepCount(id: string): number {
    return this.data.steps.filter((step) => step.runId === id).length;
  }

  steps(id: string): AgentStep[] {
    return this.data.steps
      .filter((step) => step.runId === id)
      .sort((a, b) => a.seq - b.seq);
  }

  appendStep(id: string, action: string, summary: string, output: string, checkpoint = ""): AgentStep | null {
    const seq = this.stepCount(id) + 1;
    const before = this.load(id);
    if (!before) return null;
    const step: AgentStep = {
      id: this.nextStepId(),
      runId: id,
      seq,
      action,
      summary,
      output,
      checkpoint,
      at: Date.now() / 1000,
    };
    this.data.steps.push(step);
    this.saveRun(before);
    return step;
  }

  private nextStepId(): number {
    return this.data.steps.reduce((max, step) => Math.max(max, step.id), 0) + 1;
  }

  lastExecuted(runId: string): { action: string; summary: string } | null {
    const steps = this.steps(runId).filter((step) => step.action === "command" || step.action === "write");
    const last = steps[steps.length - 1];
    return last ? { action: last.action, summary: last.summary } : null;
  }

  unparsedCount(runId: string): number {
    return this.data.steps.filter((step) => step.runId === runId && step.action === "unparsed").length;
  }

  loadCheckpoint(runId: string, seq: number): string {
    const step = this.data.steps.find((entry) => entry.runId === runId && entry.seq === seq && entry.action === "write");
    return step ? step.checkpoint : "";
  }
}

/* ------------------------------------------------------------------ */
/* runtime                                                             */

export interface AgentRuntimeDeps {
  /** Open workspace root; throws WorkspaceError when none is open. */
  root: () => string;
  /** Shared path-confined helpers from lib.ts (resolveInside, etc.). */
  callRouter: (messages: Array<Record<string, string>>) => Promise<string>;
  exclusions?: () => string[];
  notify: (runId: string | null) => void;
}

interface ToolDeps {
  resolveInside: (root: string, relative: string) => string;
}

let toolDeps: ToolDeps | null = null;
/** lib.ts wires its path helpers in once at startup. */
export function bindTools(deps: ToolDeps): void {
  toolDeps = deps;
}

function tools(): ToolDeps {
  if (!toolDeps) throw new Error("agent tools not bound");
  return toolDeps;
}

function readAnyFile(root: string, relative: string): { text: string } | null {
  const target = tools().resolveInside(root, relative);
  let stat: fs.Stats;
  try {
    stat = fs.statSync(target);
  } catch {
    return null;
  }
  if (stat.isDirectory()) return null;
  if (stat.size > 1024 * 1024) throw new Error(`That file is ${Math.round(stat.size / 1024)} KB. The agent reads files up to 1024 KB.`);
  return { text: fs.readFileSync(target, "utf8") };
}

function runToolCommand(root: string, command: string): Promise<string> {
  if (!command.trim()) throw new Error("Empty command.");
  return new Promise((resolve, reject) => {
    const isWindows = process.platform === "win32";
    const child = spawn(isWindows ? "cmd" : "sh", isWindows ? ["/C", command] : ["-c", command], {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error("Command timed out after 90 seconds and was killed."));
    }, COMMAND_TIMEOUT_MS);
    child.stdout.on("data", (chunk) => { stdout += String(chunk); });
    child.stderr.on("data", (chunk) => { stderr += String(chunk); });
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(new Error(`Failed to spawn: ${error.message}`));
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve(`exit: ${code}\n--- stdout ---\n${capChars(stdout.trim(), STEP_OUTPUT_CAP)}\n--- stderr ---\n${capChars(stderr.trim(), STEP_OUTPUT_CAP)}`);
    });
  });
}

/** Snapshot the file a write is about to replace (per-file rollback). */
function captureCheckpoint(root: string, filePath: string): { checkpoint: string; oldContent: string | null } {
  const file = readAnyFile(root, filePath);
  if (!file) return { checkpoint: "", oldContent: null };
  if ([...file.text].length > CHECKPOINT_CAP) return { checkpoint: "", oldContent: file.text };
  return {
    checkpoint: JSON.stringify({ path: filePath, old: file.text }),
    oldContent: file.text,
  };
}

function runSummaryJson(run: AgentRun, stepCount: number): Record<string, unknown> {
  return {
    id: run.id,
    projectId: run.projectId,
    goal: run.goal,
    status: run.status,
    allowWrites: run.allowWrites,
    allowCommands: run.allowCommands,
    autoApprove: run.autoApprove,
    maxSteps: run.maxSteps,
    pendingAction: pendingSummaryJson(run),
    error: run.error,
    steps: stepCount,
    createdAt: run.createdAt,
    updatedAt: run.updatedAt,
  };
}

function pendingSummaryJson(run: AgentRun): Record<string, unknown> | null {
  if (!run.pendingAction) return null;
  try {
    const parsed = parseAction(run.pendingAction);
    if (parsed.ok) return { tag: actionTag(parsed.action), summary: pendingSummary(parsed.action) };
  } catch {
    // fall through
  }
  return { tag: "unknown", summary: "unreadable pending action" };
}

function allowed(run: AgentRun, action: AgentAction): { kind: "allowed" | "needs-approval" | "denied"; need?: string } {
  if (action.action === "write" && !run.allowWrites) return { kind: "denied", need: "file writes" };
  if (action.action === "command" && !run.allowCommands) return { kind: "denied", need: "command execution" };
  if ((action.action === "write" || action.action === "command") && !run.autoApprove) return { kind: "needs-approval" };
  return { kind: "allowed" };
}

type ToolOutcome = { ok: true; output: string; checkpoint: string; oldContent: string | null } | { ok: false; error: string };

/** Tool execution, gated nothing here — the caller handles the gates. */
async function dispatchTool(root: string, action: AgentAction): Promise<ToolOutcome> {
  try {
    switch (action.action) {
      case "write": {
        const target = tools().resolveInside(root, action.path || "");
        if (fs.existsSync(target) && fs.statSync(target).isDirectory()) {
          throw new Error("That path is a folder, not a file.");
        }
        // Snapshot BEFORE writing: this is what makes the change diffable
        // and reversible from the run log.
        const { checkpoint, oldContent } = captureCheckpoint(root, action.path || "");
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, action.content || "", "utf8");
        return {
          ok: true,
          output: `Wrote ${(action.content || "").length} bytes to ${action.path}.\n--- diff ---\n${unifiedDiff(oldContent, action.content, action.path)}`,
          checkpoint,
          oldContent,
        };
      }
      case "read": {
        const file = readAnyFile(root, action.path || "");
        if (!file) throw new Error(`No such file: ${action.path}`);
        return { ok: true, output: capChars(file.text, STEP_OUTPUT_CAP), checkpoint: "", oldContent: null };
      }
      case "list": {
        const rel = action.path || "";
        const dir = tools().resolveInside(root, rel);
        if (!fs.statSync(dir).isDirectory()) throw new Error("That path is not a folder.");
        const entries = fs.readdirSync(dir, { withFileTypes: true });
        const names = entries.slice(0, 200).map((entry) => (entry.isDirectory() ? `${entry.name}/` : entry.name)).join("\n");
        const truncated = entries.length > 200 ? "\n(list truncated)" : "";
        return { ok: true, output: capChars(names, STEP_OUTPUT_CAP) + truncated, checkpoint: "", oldContent: null };
      }
      case "command": {
        const output = await runToolCommand(root, action.command || "");
        return { ok: true, output, checkpoint: "", oldContent: null };
      }
      default:
        return { ok: false, error: "Only list, read, write and command actions execute." };
    }
  } catch (error) {
    return { ok: false, error: (error as Error).message };
  }
}

/** Drives runs against a store live. Created per embedded server start. */
export class AgentRuntime {
  private readonly stopping = new Set<string>();

  constructor(private readonly store: RunStore, private deps: AgentRuntimeDeps) {}

  create(input: {
    goal?: unknown; allowWrites?: unknown; allowCommands?: unknown;
    autoApprove?: unknown; maxSteps?: unknown; projectId?: string;
  }): { ok: true; id: string } | { ok: false; error: string; status?: number } {
    const goal = String(input.goal || "").trim();
    if (!goal) return { ok: false, error: "Describe what the agent should do.", status: 400 };
    if ([...goal].length > 5_000) return { ok: false, error: "Goal is too long.", status: 400 };
    if (!this.deps.exclusions) this.deps = { ...this.deps, exclusions: () => [] };
    let root: string;
    try {
      root = this.deps.root();
    } catch (error) {
      return { ok: false, error: "Open a workspace folder first — agent runs only work inside an open workspace.", status: 400 };
    }
    void root;
    const run = this.store.create({
      projectId: input.projectId || "",
      goal,
      allowWrites: Boolean(input.allowWrites),
      allowCommands: Boolean(input.allowCommands),
      autoApprove: Boolean(input.autoApprove),
      maxSteps: Number(input.maxSteps) || DEFAULT_MAX_STEPS,
    });
    this.drive(run.id);
    this.deps.notify(run.id);
    return { ok: true, id: run.id };
  }

  summary(run: AgentRun): Record<string, unknown> {
    return runSummaryJson(run, this.store.stepCount(run.id));
  }

  isLive(run: AgentRun): boolean {
    return run.status === RUNNING || run.status === AWAITING_APPROVAL;
  }

  cancel(id: string): { ok: boolean; status?: string; error?: string } {
    const run = this.store.load(id);
    if (!run) return { ok: false, error: "No such run." };
    if (run.status !== RUNNING && run.status !== AWAITING_APPROVAL) return { ok: false, error: "Run is not running." };
    this.stopping.add(id);
    this.store.finish(id, CANCELLED, "Cancelled by user.");
    this.deps.notify(id);
    return { ok: true, status: CANCELLED };
  }

  async approve(runId: string, approve: boolean): Promise<{ ok: boolean; status?: string; error?: string }> {
    const run = this.store.load(runId);
    if (!run) return { ok: false, error: "No such run." };
    if (run.status !== AWAITING_APPROVAL || !run.pendingAction) {
      return { ok: false, error: "This run has no action waiting for approval." };
    }
    const parsed = parseAction(run.pendingAction);
    if (!parsed.ok) {
      this.store.finish(runId, FAILED, "Approval data was unreadable.");
      this.deps.notify(runId);
      return { ok: false, error: "Approval data was unreadable." };
    }
    if (!approve) {
      this.store.appendStep(runId, "rejected", pendingSummary(parsed.action), "Rejected by the user.");
      this.store.finish(runId, FAILED, "You rejected the action the agent needed.");
      this.deps.notify(runId);
      return { ok: true, status: FAILED };
    }
    let root: string;
    try {
      root = this.deps.root();
    } catch (error) {
      this.store.finish(runId, FAILED, `Workspace closed: ${(error as Error).message}`);
      this.deps.notify(runId);
      return { ok: false, error: (error as Error).message };
    }
    const outcome = await dispatchTool(root, parsed.action);
    if (!outcome.ok) {
      this.store.finish(runId, FAILED, outcome.error);
      this.deps.notify(runId);
      return { ok: false, error: outcome.error };
    }
    this.store.appendStep(runId, actionTag(parsed.action), pendingSummary(parsed.action), outcome.output, outcome.checkpoint);
    this.store.setStatus(runId, RUNNING, null, "");
    this.drive(runId);
    this.deps.notify(runId);
    return { ok: true, status: RUNNING };
  }

  restoreStep(runId: string, seq: number): { ok: boolean; error?: string } {
    const run = this.store.load(runId);
    if (!run) return { ok: false, error: "No such run." };
    const checkpoint = this.store.loadCheckpoint(runId, seq);
    if (!checkpoint) return { ok: false, error: "This step has no checkpoint to restore." };
    let record: { path?: unknown; old?: unknown };
    try {
      record = JSON.parse(checkpoint) as { path?: unknown; old?: unknown };
    } catch {
      return { ok: false, error: "Checkpoint is unreadable; nothing was restored." };
    }
    if (typeof record.path !== "string" || typeof record.old !== "string") {
      return { ok: false, error: "Checkpoint is unreadable; nothing was restored." };
    }
    let root: string;
    try {
      root = this.deps.root();
    } catch (error) {
      return { ok: false, error: (error as Error).message };
    }
    try {
      const target = tools().resolveInside(root, record.path);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, record.old, "utf8");
    } catch (error) {
      return { ok: false, error: (error as Error).message };
    }
    this.store.appendStep(runId, "restored", `restored ${record.path}`, "File restored to the checkpoint from before the agent wrote it.");
    this.deps.notify(runId);
    return { ok: true };
  }

  /** The bounded loop: model step, gate, execute, log, repeat. */
  drive(runId: string): void {
    void (async () => {
      for (;;) {
        const run = this.store.load(runId);
        if (!run || run.status !== RUNNING) break;
        if (this.stopping.has(runId)) break;
        if (this.store.stepCount(runId) >= HARD_MAX_STEPS) {
          this.store.appendStep(runId, "error", "", "Run stopped: the step ceiling was reached without completion.");
          this.store.finish(runId, FAILED, "Run exceeded the step ceiling.");
          this.deps.notify(runId);
          break;
        }
        const keepGoing = await this.stepOnce(run);
        this.deps.notify(runId);
        if (!this.isLive(this.store.load(runId) || run)) break;
        if (!keepGoing) break;
      }
      if (this.store.load(runId) && this.store.stepCount(runId) >= HARD_MAX_STEPS && this.isLive(this.store.load(runId) as AgentRun)) {
        this.store.finish(runId, FAILED, "Run exceeded the step ceiling.");
        this.deps.notify(runId);
      }
      this.stopping.delete(runId);
    })().catch(async (error) => {
      this.store.appendStep(runId, "error", "The model call failed:", (error as Error).message);
      this.store.finish(runId, FAILED, (error as Error).message);
      this.deps.notify(runId);
      this.stopping.delete(runId);
    });
  }

  /** One model step. Returns false when the run reached a terminal state. */
  private async stepOnce(run: AgentRun): Promise<boolean> {
    const history = buildHistory(this.store.steps(run.id));
    const reply = await this.deps.callRouter(buildMessages(run, history));
    // A cancel can land while the model is thinking: a stopped run must not
    // act on the reply that arrives afterwards (mirrors agent.rs step_once).
    const live = this.store.load(run.id);
    if (!live || live.status !== RUNNING) return false;
    const parsed = parseAction(reply);
    if (!parsed.ok) {
      this.store.appendStep(run.id, "unparsed", "Model reply was not a usable action; retrying with counts against retries.", `${parsed.error}\nreply:\n${capChars(reply, 600)}`);
      if (this.store.unparsedCount(run.id) >= 3) {
        this.store.finish(run.id, FAILED, "The model kept replying with unparseable text.");
        return false;
      }
      return true;
    }
    switch (parsed.action.action) {
      case "think":
        this.store.appendStep(run.id, "think", parsed.action.summary || "", "");
        return true;
      case "done":
        this.store.appendStep(run.id, "done", "Run complete", parsed.action.summary || "");
        this.store.finish(run.id, SUCCEEDED, "");
        return false;
      case "fail":
        this.store.appendStep(run.id, "fail", "Model gave up", parsed.action.reason || "");
        this.store.finish(run.id, FAILED, parsed.action.reason || "");
        return false;
      default:
        return this.executeStep(run, parsed.action);
    }
  }

  /** Permission gate, approval gate, execute, log — mirrors agent.rs. */
  private async executeStep(run: AgentRun, action: AgentAction): Promise<boolean> {
    const gate = allowed(run, action);
    if (gate.kind === "denied") {
      this.store.appendStep(run.id, actionTag(action), pendingSummary(action), `Attempted ${gate.need}, but this run started without that permission.`);
      this.store.finish(run.id, FAILED, `The model needed ${gate.need}, but the run started without that permission. Start a new run with it allowed.`);
      return false;
    }
    if (gate.kind === "needs-approval") {
      const last = this.store.lastExecuted(run.id);
      if (last && last.action === action.action && last.summary === pendingSummary(action)) {
        this.store.appendStep(run.id, actionTag(action), pendingSummary(action), "This exact action already ran — continuing instead of repeating it.");
        return true;
      }
      this.store.appendStep(run.id, "approval-requested", pendingSummary(action), "Paused. Approve or reject in the Runs panel.");
      this.store.setStatus(run.id, AWAITING_APPROVAL, null, JSON.stringify(action));
      return false;
    }
    let root: string;
    try {
      root = this.deps.root();
    } catch (error) {
      this.store.finish(run.id, FAILED, `Workspace closed: ${(error as Error).message}`);
      return false;
    }
    const summary = pendingSummary(action);
    // Privacy: a read or listing of an excluded path is replaced by a notice.
    const exclusions = this.deps.exclusions?.() || [];
    if (
      (action.action === "read" || action.action === "list") &&
      isExcluded(action.path || "", exclusions)
    ) {
      const output = `[blocked by privacy policy: ${action.path} is on the outbound exclusion list; contents were not sent and are not shown here.]`;
      this.store.appendStep(run.id, actionTag(action), summary, output);
      return true;
    }
    const outcome = await dispatchTool(root, action);
    if (!outcome.ok) {
      // Not fatal: the next model step sees the error and can repair.
      this.store.appendStep(run.id, `${actionTag(action)}-error`, summary, outcome.error);
      return true;
    }
    this.store.appendStep(run.id, actionTag(action), summary, outcome.output, outcome.checkpoint);
    return true;
  }
}
