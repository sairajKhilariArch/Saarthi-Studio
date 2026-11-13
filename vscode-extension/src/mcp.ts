// MCP for the VS Code extension's built-in server: the desktop's stdio subset.
//
// The desktop app owns the full MCP manager (`app/src/mcp.rs`): three
// transports, pooled sessions, OAuth, the permission store and a supervisor.
// This module deliberately implements the smallest honest subset that the
// shared Settings → MCP panel can drive while the desktop app is closed:
//
//   * stdio servers only, started from a command the user registered;
//   * one warm session per server, opened on Test / health check / a tool call
//     and closed by Disconnect, Remove or the process exiting;
//   * JSON-RPC 2.0 over newline-delimited JSON, the MCP stdio framing;
//   * no server-to-client requests (they are answered with -32601) and no
//     OAuth or header transport.
//
// Nothing here invents an observation: a report is only "healthy" after a real
// handshake, and a failure keeps the server's own error text.

import * as fs from "node:fs";
import * as path from "node:path";
import { spawn } from "node:child_process";
import type { ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";

export const MCP_PROTOCOL_VERSION = "2024-11-05";
export const MCP_REQUEST_TIMEOUT_MS = 20_000;
const MAX_SERVERS = 32;
const MAX_ARGS = 32;
const MAX_ARG_CHARS = 500;
const MAX_ENV_PAIRS = 32;
const MAX_ENV_CHARS = 2000;
const MAX_TOOLS = 200;
const MAX_DESCRIPTION = 400;
const MAX_TEXT = 64 * 1024;
const MAX_LOG_ENTRIES = 200;
const MAX_LOG_LINE = 300;

export type McpSource = "client" | "server";

export interface McpLogEntry {
  at: number;
  source: McpSource;
  text: string;
}

export interface McpServer {
  name: string;
  transport: "stdio";
  command: string;
  args: string[];
  env: Record<string, string>;
  enabled: boolean;
}

export interface McpTool {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
}

export interface McpReport {
  name: string;
  health: "unknown" | "healthy" | "unhealthy";
  tools: number;
  error?: string;
  version?: string;
  protocol?: string;
  latency_ms?: number;
  checked_at?: number;
}

/** A request the caller got wrong, with the status the route should answer. */
export class McpError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

function noControl(value: string): boolean {
  // Control characters are refused rather than escaped: a command line or an
  // environment value with a newline is a bug in the caller, not a value.
  return !/[\u0000-\u001f\u007f]/.test(value);
}

function stringList(value: unknown, limit: number, chars: number, label: string): string[] {
  if (value === undefined || value === null) return [];
  if (!Array.isArray(value)) throw new McpError(400, `${label} must be a list.`);
  if (value.length > limit) throw new McpError(400, `${label} allows at most ${limit} entries.`);
  return value.map((entry) => {
    const text = String(entry ?? "").trim();
    if (!text) throw new McpError(400, `${label} entries cannot be empty.`);
    if (text.length > chars) throw new McpError(400, `${label} entries are at most ${chars} characters.`);
    if (!noControl(text)) throw new McpError(400, `${label} entries cannot contain control characters.`);
    return text;
  });
}

function pairs(value: unknown, label: string, keyPattern: RegExp): Record<string, string> {
  if (value === undefined || value === null) return {};
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new McpError(400, `${label} must be an object of name/value pairs.`);
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length > MAX_ENV_PAIRS) {
    throw new McpError(400, `${label} allows at most ${MAX_ENV_PAIRS} entries.`);
  }
  const result: Record<string, string> = {};
  for (const [key, raw] of entries) {
    if (!keyPattern.test(key)) throw new McpError(400, `${label} name \"${key}\" is not valid.`);
    const text = String(raw ?? "");
    if (text.length > MAX_ENV_CHARS) {
      throw new McpError(400, `${label} value for ${key} is over ${MAX_ENV_CHARS} characters.`);
    }
    if (!noControl(text)) throw new McpError(400, `${label} value for ${key} has a control character.`);
    result[key] = text;
  }
  return result;
}

/** Turn a panel payload into a stored server, or refuse with the reason. */
export function normalizeMcpServer(input: Record<string, unknown>): McpServer {
  const name = String(input.name ?? "").trim();
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(name)) {
    throw new McpError(400, "A server needs a name of letters, digits, `-` or `_` (up to 64).");
  }
  const transport = String(input.transport ?? "stdio").trim().toLowerCase() || "stdio";
  if (transport !== "stdio") {
    // The panel offers the other transports because the desktop serves them;
    // saying so is better than saving a server this process cannot start.
    throw new McpError(
      400,
      `The VS Code built-in server starts stdio servers only. ${transport.toUpperCase()} servers, headers and OAuth sign-in need the Sarthi Studio desktop app.`,
    );
  }
  const command = String(input.command ?? "").trim();
  if (!command) throw new McpError(400, "A stdio server needs a command to run.");
  if (command.length > MAX_ARG_CHARS) throw new McpError(400, "That command is too long.");
  if (!noControl(command)) throw new McpError(400, "The command cannot contain control characters.");
  const args = stringList(input.args, MAX_ARGS, MAX_ARG_CHARS, "Arguments");
  const env = pairs(input.env, "Environment", /^[A-Za-z_][A-Za-z0-9_]*$/);
  const headers = pairs(input.headers, "Headers", /^[A-Za-z0-9-]{1,64}$/);
  if (Object.keys(headers).length) {
    throw new McpError(400, "A stdio server takes no headers; a remote server needs the desktop app.");
  }
  return {
    name,
    transport: "stdio",
    command,
    args,
    env,
    enabled: input.enabled === undefined ? true : input.enabled === true,
  };
}

function readRegistry(file: string): McpServer[] {
  let raw: unknown;
  try {
    raw = JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return [];
  }
  const servers = (raw as { servers?: unknown })?.servers;
  if (!Array.isArray(servers)) return [];
  const valid: McpServer[] = [];
  for (const entry of servers) {
    try {
      const server = normalizeMcpServer((entry || {}) as Record<string, unknown>);
      if (!valid.some((seen) => seen.name === server.name)) valid.push(server);
    } catch {
      // A row this build cannot run is skipped, not guessed at.
    }
  }
  return valid.slice(0, MAX_SERVERS);
}

function writeRegistry(file: string, servers: McpServer[]): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const staging = `${file}.tmp-${process.pid}`;
  fs.writeFileSync(staging, `${JSON.stringify({ servers }, null, 2)}\n`, "utf8");
  fs.renameSync(staging, file);
}

function cap(value: string, limit: number): string {
  return value.length <= limit ? value : value.slice(0, limit);
}

function toolRows(tools: unknown[]): McpTool[] {
  return tools
    .filter((tool): tool is Record<string, unknown> => Boolean(tool && typeof tool === "object"))
    .map((tool) => ({
      name: String(tool.name ?? ""),
      description: cap(String(tool.description ?? ""), MAX_DESCRIPTION),
      input_schema:
        tool.inputSchema && typeof tool.inputSchema === "object"
          ? (tool.inputSchema as Record<string, unknown>)
          : {},
    }))
    .filter((tool) => tool.name)
    .slice(0, MAX_TOOLS);
}

interface Pending {
  resolve: (value: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  timer: NodeJS.Timeout;
}

/** One live MCP stdio process and its JSON-RPC conversation. */
class McpSession {
  private child?: ChildProcessWithoutNullStreams;
  private pending = new Map<number, Pending>();
  private nextId = 1;
  private closed = false;
  private protocol = MCP_PROTOCOL_VERSION;
  private version = "";
  readonly logs: McpLogEntry[] = [];

  constructor(private readonly server: McpServer) {}

  private record(source: McpSource, text: string): void {
    this.logs.push({ at: Date.now() / 1000, source, text: cap(text, MAX_LOG_LINE) });
    if (this.logs.length > MAX_LOG_ENTRIES) this.logs.shift();
  }

  private write(message: Record<string, unknown>): void {
    if (!this.child || this.closed) return;
    try {
      this.child.stdin.write(`${JSON.stringify(message)}\n`);
    } catch (error) {
      this.failAll(`could not write to the server (${(error as Error).message})`);
    }
  }

  private failAll(reason: string): void {
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
      this.pending.delete(id);
    }
  }

  private stop(reason: string): void {
    this.closed = true;
    this.failAll(reason);
    try {
      this.child?.kill();
    } catch {
      // The process is already gone; the report keeps the reason.
    }
    this.child = undefined;
  }

  private onLine(line: string): void {
    const text = line.trim();
    if (!text) return;
    let message: Record<string, unknown>;
    try {
      message = JSON.parse(text) as Record<string, unknown>;
    } catch {
      this.record("server", text);
      return;
    }
    const id = typeof message.id === "number" ? message.id : undefined;
    if (id !== undefined && (message.result !== undefined || message.error !== undefined)) {
      const pending = this.pending.get(id);
      if (!pending) return;
      clearTimeout(pending.timer);
      this.pending.delete(id);
      if (message.error) {
        const detail = (message.error as { message?: unknown }).message;
        pending.reject(new Error(String(detail || "the server returned an error")));
      } else {
        pending.resolve((message.result || {}) as Record<string, unknown>);
      }
      return;
    }
    if (typeof message.method === "string" && id !== undefined) {
      this.record("client", `refused ${message.method}: no server-to-client requests in this subset`);
      this.write({
        jsonrpc: "2.0",
        id,
        error: { code: -32601, message: "Not supported by Sarthi Studio's built-in server." },
      });
      return;
    }
    if (typeof message.method === "string") this.record("server", `notified ${message.method}`);
  }

  async open(): Promise<void> {
    if (this.child && !this.closed) return;
    const child = spawn(this.server.command, this.server.args, {
      cwd: process.cwd(),
      env: { ...process.env, ...this.server.env },
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.child = child;
    this.closed = false;
    this.record("client", `started ${[this.server.command, ...this.server.args].join(" ")}`);
    createInterface({ input: child.stdout }).on("line", (line) => this.onLine(line));
    createInterface({ input: child.stderr }).on("line", (line) => this.record("server", line));
    child.on("error", (error) => {
      this.closed = true;
      this.failAll(`could not start ${this.server.command}: ${error.message}`);
    });
    child.on("exit", (code, signal) => {
      this.closed = true;
      this.failAll(`the server exited (${signal || code})`);
    });
  }

  request(method: string, params: Record<string, unknown>): Promise<Record<string, unknown>> {
    if (!this.child || this.closed) {
      return Promise.reject(new Error("the MCP session is not open"));
    }
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        this.stop(`${this.server.name} did not answer ${method} within 20 s`);
        reject(new Error(`${this.server.name} did not answer ${method} within 20 seconds.`));
      }, MCP_REQUEST_TIMEOUT_MS);
      this.pending.set(id, { resolve, reject, timer });
      this.write({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method: string, params: Record<string, unknown>): void {
    this.write({ jsonrpc: "2.0", method, params });
  }

  /** `initialize` + `notifications/initialized`, as the MCP lifecycle orders. */
  get protocolVersion(): string {
    return this.protocol;
  }

  get serverVersion(): string {
    return this.version;
  }

  async handshake(): Promise<{ protocol: string; version: string; latencyMs: number }> {
    const started = Date.now();
    await this.open();
    const result = await this.request("initialize", {
      protocolVersion: MCP_PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: "sarthi-studio-vscode", version: "2.6.0" },
    });
    this.notify("notifications/initialized", {});
    this.protocol = String(result.protocolVersion || MCP_PROTOCOL_VERSION);
    const info = (result.serverInfo || {}) as { version?: unknown };
    this.version = String(info.version || "");
    return { protocol: this.protocol, version: this.version, latencyMs: Date.now() - started };
  }

  async tools(): Promise<McpTool[]> {
    const result = await this.request("tools/list", {});
    return toolRows(Array.isArray(result.tools) ? result.tools : []);
  }

  async call(tool: string, args: Record<string, unknown>): Promise<{ text: string; is_error: boolean }> {
    const result = await this.request("tools/call", { name: tool, arguments: args });
    const content = Array.isArray(result.content) ? result.content : [];
    const text = content
      .filter((entry): entry is Record<string, unknown> => Boolean(entry && typeof entry === "object"))
      .filter((entry) => entry.type === "text" && typeof entry.text === "string")
      .map((entry) => String(entry.text))
      .join("\n");
    return {
      text: cap(text, MAX_TEXT) || "(the tool returned no text)",
      is_error: result.isError === true,
    };
  }

  async close(): Promise<void> {
    if (!this.child) {
      this.closed = true;
      return;
    }
    this.record("client", "closed");
    try {
      this.child.stdin.end();
    } catch {
      // The pipe is already closed.
    }
    this.stop("the session was closed");
  }
}

/** Registry, warm sessions and the observations the panel reads. */
export class McpService {
  private sessions = new Map<string, McpSession>();
  private reports = new Map<string, McpReport>();
  private logs = new Map<string, McpLogEntry[]>();

  constructor(private readonly file: string) {}

  private servers(): McpServer[] {
    return readRegistry(this.file);
  }

  private find(name: string): McpServer {
    const server = this.servers().find((entry) => entry.name === name);
    if (!server) throw new McpError(404, `No MCP server named "${name}".`);
    return server;
  }

  private record(name: string): void {
    const session = this.sessions.get(name);
    this.logs.set(name, session ? [...session.logs] : this.logs.get(name) || []);
  }

  list(): Array<Record<string, unknown>> {
    return this.servers().map((server) => ({
      name: server.name,
      transport: server.transport,
      enabled: server.enabled,
      command: server.command,
      args: server.args,
      url: "",
      environment: Object.keys(server.env),
      headers: [],
    }));
  }

  healthRows(): McpReport[] {
    return this.servers().map(
      (server) => this.reports.get(server.name) || { name: server.name, health: "unknown", tools: 0 },
    );
  }

  entries(name: string): McpLogEntry[] {
    return this.logs.get(name) || [];
  }

  save(input: Record<string, unknown>): McpServer {
    const server = normalizeMcpServer(input);
    const servers = this.servers();
    const index = servers.findIndex((entry) => entry.name === server.name);
    if (index === -1 && servers.length >= MAX_SERVERS) {
      throw new McpError(400, `At most ${MAX_SERVERS} MCP servers can be registered.`);
    }
    // A changed command must not keep talking to the old process.
    void this.closeSession(server.name).catch(() => undefined);
    this.reports.delete(server.name);
    if (index === -1) servers.push(server);
    else servers[index] = server;
    writeRegistry(this.file, servers);
    return server;
  }

  async remove(name: string): Promise<void> {
    const servers = this.servers().filter((entry) => entry.name !== name);
    await this.closeSession(name);
    this.reports.delete(name);
    writeRegistry(this.file, servers);
  }

  private async session(name: string): Promise<McpSession> {
    const server = this.find(name);
    if (!server.enabled) throw new McpError(409, `MCP server "${name}" is disabled.`);
    const warm = this.sessions.get(name);
    if (warm) return warm;
    const session = new McpSession(server);
    this.sessions.set(name, session);
    try {
      await session.handshake();
    } catch (error) {
      this.sessions.delete(name);
      await session.close();
      throw error;
    }
    return session;
  }

  private observe(name: string, report: McpReport): void {
    this.reports.set(name, report);
    this.record(name);
  }

  async test(name: string): Promise<{ tools: McpTool[]; protocol: string; version: string; latency_ms: number }> {
    const started = Date.now();
    try {
      const session = await this.session(name);
      const tools = await session.tools();
      this.observe(name, {
        name,
        health: "healthy",
        tools: tools.length,
        protocol: session.protocolVersion,
        version: session.serverVersion,
        latency_ms: Date.now() - started,
        checked_at: Date.now() / 1000,
      });
      return {
        tools,
        protocol: session.protocolVersion,
        version: session.serverVersion,
        latency_ms: Date.now() - started,
      };
    } catch (error) {
      // A missing name or a disabled server is the caller's mistake, not a
      // failed handshake: keep its status instead of blaming the server.
      if (error instanceof McpError) throw error;
      this.observe(name, {
        name,
        health: "unhealthy",
        tools: 0,
        error: (error as Error).message,
        latency_ms: Date.now() - started,
        checked_at: Date.now() / 1000,
      });
      throw new McpError(502, `MCP server "${name}" did not answer: ${(error as Error).message}`);
    }
  }

  async call(
    name: string,
    tool: string,
    args: Record<string, unknown>,
  ): Promise<{ text: string; is_error: boolean }> {
    if (!tool.trim()) throw new McpError(400, "Name a tool to call.");
    try {
      const session = await this.session(name);
      return await session.call(tool, args);
    } catch (error) {
      if (error instanceof McpError) throw error;
      this.observe(name, {
        name,
        health: "unhealthy",
        tools: 0,
        error: (error as Error).message,
        checked_at: Date.now() / 1000,
      });
      throw new McpError(502, `${name}.${tool} failed: ${(error as Error).message}`);
    }
  }

  async check(name = ""): Promise<{
    checked: McpReport[];
    results: Array<{ name: string; ok: boolean; error?: string }>;
    skipped: Array<{ name: string; reason: string }>;
    message?: string;
  }> {
    const servers = this.servers().filter((server) => server.enabled && (!name || server.name === name));
    if (!servers.length) {
      return {
        checked: this.healthRows(),
        results: [],
        skipped: [],
        message: name ? "No enabled MCP server by that name." : "No enabled MCP servers to check.",
      };
    }
    const results: Array<{ name: string; ok: boolean; error?: string }> = [];
    for (const server of servers) {
      try {
        await this.test(server.name);
        results.push({ name: server.name, ok: true });
      } catch (error) {
        results.push({ name: server.name, ok: false, error: (error as Error).message });
      }
    }
    return { checked: this.healthRows(), results, skipped: [] };
  }

  private async closeSession(name: string): Promise<boolean> {
    const session = this.sessions.get(name);
    if (!session) return false;
    this.sessions.delete(name);
    await session.close();
    this.logs.set(name, [...session.logs]);
    return true;
  }

  async disconnect(name: string): Promise<boolean> {
    this.find(name);
    return this.closeSession(name);
  }

  /** Close every warm process; the server shutdown path calls this. */
  async shutdown(): Promise<void> {
    for (const name of [...this.sessions.keys()]) {
      await this.closeSession(name).catch(() => undefined);
    }
  }
}
