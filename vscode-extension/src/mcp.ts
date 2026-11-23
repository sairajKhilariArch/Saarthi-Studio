// MCP for the VS Code extension's built-in server.
//
// The desktop app owns the full MCP manager (`app/src/mcp.rs`): three
// transports, pooled sessions, OAuth, the permission store and a supervisor.
// This module implements the subset that the shared Settings → MCP panel can
// drive while the desktop app is closed:
//
//   * stdio servers started from a command the user registered;
//   * streamable HTTP servers (POST JSON-RPC, JSON or SSE answers, an
//     `Mcp-Session-Id` kept for the session, custom headers for a bearer
//     token the user supplies);
//   * one warm session per server, opened on Test / health check / a tool call
//     and closed by Disconnect, Remove or the process exiting;
//   * OAuth sign-in for an http/sse server that asks for it (`mcpoauth.ts`),
//     with the stored token attached and refreshed before each request;
//   * no server-to-client requests (they are answered with -32601).
//
// Nothing here invents an observation: a report is only "healthy" after a real
// handshake, and a failure keeps the server's own error text.

import * as fs from "node:fs";
import * as path from "node:path";
import { spawn } from "node:child_process";
import type { ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { validateArguments } from "./mcpschema";
import type { McpOAuth } from "./mcpoauth";

export const MCP_PROTOCOL_VERSION = "2024-11-05";
export const MCP_REQUEST_TIMEOUT_MS = 20_000;
const MAX_SERVERS = 32;
const MAX_URL_CHARS = 2000;
const MAX_BODY_CHARS = 1024 * 1024;
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
  transport: "stdio" | "http" | "sse";
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
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
  const rawTransport = String(input.transport ?? "stdio").trim().toLowerCase() || "stdio";
  const transport = rawTransport === "streamable-http" || rawTransport === "streamable_http"
    ? "http"
    : rawTransport;
  const headers = pairs(input.headers, "Headers", /^[A-Za-z0-9-]{1,64}$/);
  if (transport === "stdio") {
    const command = String(input.command ?? "").trim();
    if (!command) throw new McpError(400, "A stdio server needs a command to run.");
    if (command.length > MAX_ARG_CHARS) throw new McpError(400, "That command is too long.");
    if (!noControl(command)) throw new McpError(400, "The command cannot contain control characters.");
    const args = stringList(input.args, MAX_ARGS, MAX_ARG_CHARS, "Arguments");
    const env = pairs(input.env, "Environment", /^[A-Za-z_][A-Za-z0-9_]*$/);
    if (Object.keys(headers).length) {
      throw new McpError(400, "A stdio server takes no headers; use an HTTP server for those.");
    }
    return {
      name,
      transport: "stdio",
      command,
      args,
      env,
      url: "",
      headers: {},
      enabled: input.enabled === undefined ? true : input.enabled === true,
    };
  }
  if (transport === "http" || transport === "sse") {
    const url = String(input.url ?? "").trim();
    if (!url) throw new McpError(400, "An HTTP server needs a URL.");
    if (url.length > MAX_URL_CHARS) throw new McpError(400, "That URL is too long.");
    if (!noControl(url)) throw new McpError(400, "The URL cannot contain control characters.");
    let parsed: URL;
    try {
      parsed = new URL(url);
    } catch {
      throw new McpError(400, "That URL is not a valid http(s) address.");
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      throw new McpError(400, "An MCP URL must start with http:// or https://.");
    }
    // The desktop's rule: plain http only to this machine; anything remote is
    // TLS, so a token is never sent in the clear across a network.
    if (parsed.protocol === "http:" && !isLoopbackHost(parsed.hostname)) {
      throw new McpError(400, "Use https:// for a remote MCP server; plain http is only for this machine.");
    }
    const command = String(input.command ?? "").trim();
    if (command) throw new McpError(400, "An HTTP server takes a URL, not a command.");
    return {
      name,
      transport,
      command: "",
      args: [],
      env: {},
      url: parsed.toString(),
      headers,
      enabled: input.enabled === undefined ? true : input.enabled === true,
    };
  }
  // Anything else: the panel offers transports because the desktop serves them.
  // Saying so is better than saving a server this build cannot use.
  throw new McpError(
    400,
    `The VS Code built-in server speaks stdio, streamable HTTP and SSE. ${rawTransport.toUpperCase()} servers need the SAARTHI-STUDIO desktop app.`,
  );
}

/** The desktop's loopback rule for a plain-http URL. */
function isLoopbackHost(hostname: string): boolean {
  const host = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  return host === "localhost" || host === "::1" || host === "0.0.0.0" || /^127\./.test(host);
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

/** What the service needs from either transport. */
interface McpWire {
  readonly logs: McpLogEntry[];
  readonly protocolVersion: string;
  readonly serverVersion: string;
  handshake(): Promise<{ protocol: string; version: string; latencyMs: number }>;
  tools(): Promise<McpTool[]>;
  call(tool: string, args: Record<string, unknown>): Promise<{ text: string; is_error: boolean }>;
  close(): Promise<void>;
}

/** JSON-RPC answers carried in an SSE body (one message per data event). */
function sseAnswers(body: string): Record<string, unknown>[] {
  const messages: Record<string, unknown>[] = [];
  for (const block of body.split(/\r?\n\r?\n/)) {
    const data = block
      .split(/\r?\n/)
      .filter((line) => line.startsWith("data:"))
      .map((line) => line.slice(5).trimStart())
      .join("\n")
      .trim();
    if (!data || data === "[DONE]") continue;
    try {
      const parsed = JSON.parse(data) as Record<string, unknown>;
      if (parsed && typeof parsed === "object") messages.push(parsed);
    } catch {
      // A non-JSON event is a keep-alive or a comment, not an answer.
    }
  }
  return messages;
}

/** One live MCP stdio process and its JSON-RPC conversation. */
class McpSession implements McpWire {
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
        error: { code: -32601, message: "Not supported by SAARTHI-STUDIO's built-in server." },
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

/** One streamable-HTTP MCP server: one POST per message, JSON or SSE answer. */
class HttpMcpSession implements McpWire {
  private nextId = 1;
  private closed = false;
  private protocol = MCP_PROTOCOL_VERSION;
  private version = "";
  private sessionId = "";
  readonly logs: McpLogEntry[] = [];

  constructor(private readonly server: McpServer, private readonly oauth?: McpOAuth) {}

  get protocolVersion(): string {
    return this.protocol;
  }

  get serverVersion(): string {
    return this.version;
  }

  private record(source: McpSource, text: string): void {
    this.logs.push({ at: Date.now() / 1000, source, text: cap(text, MAX_LOG_LINE) });
    if (this.logs.length > MAX_LOG_ENTRIES) this.logs.shift();
  }

  /** The server's own headers plus a stored OAuth token, when one applies. */
  private async authHeaders(): Promise<Record<string, string>> {
    const extra = this.oauth ? await this.oauth.header(this.server) : {};
    return { ...this.server.headers, ...extra };
  }

  /** Send one JSON-RPC message; header values never reach the log. */
  private async post(
    message: Record<string, unknown>,
    expectAnswer: boolean,
  ): Promise<Record<string, unknown> | null> {
    if (this.closed) throw new Error("the MCP session is closed");
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      Accept: "application/json, text/event-stream",
      ...(await this.authHeaders()),
    };
    if (this.sessionId) headers["Mcp-Session-Id"] = this.sessionId;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), MCP_REQUEST_TIMEOUT_MS);
    let response: Response;
    try {
      response = await fetch(this.server.url, {
        method: "POST",
        headers,
        body: JSON.stringify(message),
        signal: controller.signal,
      });
    } catch (error) {
      const reason = (error as Error).name === "AbortError"
        ? "no answer within 20 s"
        : (error as Error).message;
      throw new Error(`could not reach ${this.server.url}: ${reason}`);
    } finally {
      clearTimeout(timer);
    }
    // The server assigns the session on initialize; every later call carries it.
    const session = response.headers.get("mcp-session-id");
    if (session) this.sessionId = cap(session, 200);
    if (!response.ok) {
      const detail = cap(await response.text().catch(() => ""), 400);
      throw new Error(`the server answered ${response.status}${detail ? `: ${detail}` : ""}`);
    }
    if (!expectAnswer) {
      await response.text().catch(() => "");
      return null;
    }
    const body = cap(await response.text(), MAX_BODY_CHARS);
    const type = (response.headers.get("content-type") || "").toLowerCase();
    let answers: Record<string, unknown>[];
    if (type.includes("text/event-stream")) {
      answers = sseAnswers(body);
    } else {
      try {
        const parsed = JSON.parse(body) as Record<string, unknown>;
        answers = parsed && typeof parsed === "object" ? [parsed] : [];
      } catch {
        throw new Error(`the server answered ${String(message.method)} with something that is not JSON`);
      }
    }
    const answer = answers.find(
      (entry) => entry.id === message.id && (entry.result !== undefined || entry.error !== undefined),
    );
    if (!answer) {
      throw new Error(`the server answered ${String(message.method)} without a matching JSON-RPC result`);
    }
    return answer;
  }

  private async request(
    method: string,
    params: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    this.record("client", `${method} over HTTP`);
    const answer = await this.post({ jsonrpc: "2.0", id: this.nextId++, method, params }, true);
    if (answer?.error) {
      const detail = (answer.error as { message?: unknown }).message;
      throw new Error(String(detail || "the server returned an error"));
    }
    return (answer?.result || {}) as Record<string, unknown>;
  }

  async handshake(): Promise<{ protocol: string; version: string; latencyMs: number }> {
    const started = Date.now();
    this.record("client", `POST ${this.server.url}`);
    const result = await this.request("initialize", {
      protocolVersion: MCP_PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: "sarthi-studio-vscode", version: "2.6.0" },
    });
    await this.post({ jsonrpc: "2.0", method: "notifications/initialized", params: {} }, false);
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
    if (this.closed) return;
    this.closed = true;
    this.record("client", "closed");
    if (!this.sessionId) return;
    try {
      // Best effort: a server may not implement DELETE, and that is fine.
      await fetch(this.server.url, {
        method: "DELETE",
        headers: { ...(await this.authHeaders()), "Mcp-Session-Id": this.sessionId },
        signal: AbortSignal.timeout(5000),
      });
    } catch {
      // Closing is local either way.
    }
  }
}

/** One legacy SSE MCP server: a GET stream names the POST endpoint, answers
    arrive on the stream (the pre-streamable-HTTP transport). */
class SseMcpSession implements McpWire {
  private pending = new Map<number, { resolve: (value: Record<string, unknown>) => void; reject: (error: Error) => void; timer: NodeJS.Timeout }>();
  private nextId = 1;
  private closed = false;
  private protocol = MCP_PROTOCOL_VERSION;
  private version = "";
  private postUrl = "";
  private controller?: AbortController;
  private ready?: Promise<void>;
  readonly logs: McpLogEntry[] = [];

  constructor(private readonly server: McpServer, private readonly oauth?: McpOAuth) {}

  get protocolVersion(): string {
    return this.protocol;
  }

  get serverVersion(): string {
    return this.version;
  }

  private record(source: McpSource, text: string): void {
    this.logs.push({ at: Date.now() / 1000, source, text: cap(text, MAX_LOG_LINE) });
    if (this.logs.length > MAX_LOG_ENTRIES) this.logs.shift();
  }

  /** The server's own headers plus a stored OAuth token, when one applies. */
  private async authHeaders(): Promise<Record<string, string>> {
    const extra = this.oauth ? await this.oauth.header(this.server) : {};
    return { ...this.server.headers, ...extra };
  }

  private failAll(reason: string): void {
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
      this.pending.delete(id);
    }
  }

  private onMessage(text: string): void {
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
    if (typeof message.method === "string") this.record("server", `notified ${message.method}`);
  }

  /** Read one SSE stream, split into events, and resolve the endpoint. */
  private async open(): Promise<void> {
    if (this.ready) return this.ready;
    const controller = new AbortController();
    this.controller = controller;
    this.ready = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        controller.abort();
        reject(new Error(`${this.server.name} did not name an endpoint within 20 s`));
      }, MCP_REQUEST_TIMEOUT_MS);
      const settle = (error?: Error) => {
        clearTimeout(timer);
        if (error) reject(error);
        else resolve();
      };
      (async () => {
        this.record("client", `GET ${this.server.url}`);
        let response: Response;
        try {
          response = await fetch(this.server.url, {
            headers: { Accept: "text/event-stream", ...(await this.authHeaders()) },
            signal: controller.signal,
          });
        } catch (error) {
          settle(new Error(`could not reach ${this.server.url}: ${(error as Error).message}`));
          return;
        }
        if (!response.ok) {
          settle(new Error(`the server answered ${response.status}`));
          return;
        }
        const base = new URL(this.server.url);
        let buffer = "";
        let event = "message";
        const handle = (block: string, eventName: string) => {
          const data = block
            .split(/\r?\n/)
            .filter((line) => line.startsWith("data:"))
            .map((line) => line.slice(5).trimStart())
            .join("\n");
          if (!data) return;
          if (eventName === "endpoint") {
            // The POST endpoint must stay on the stream's own origin.
            let target: URL;
            try {
              target = new URL(data, base);
            } catch {
              settle(new Error("the server named an endpoint that is not a URL"));
              return;
            }
            if (target.origin !== base.origin) {
              settle(new Error("the server named an endpoint on another origin"));
              return;
            }
            this.postUrl = target.toString();
            this.record("client", `endpoint ${this.postUrl}`);
            settle();
            return;
          }
          this.onMessage(data);
        };
        try {
          const reader = response.body?.getReader();
          if (!reader) throw new Error("the server sent no stream body");
          const decoder = new TextDecoder();
          for (;;) {
            const { value, done } = await reader.read();
            if (done) break;
            buffer += decoder.decode(value, { stream: true });
            for (;;) {
              const split = buffer.search(/\r?\n\r?\n/);
              if (split < 0) break;
              const raw = buffer.slice(0, split);
              buffer = buffer.slice(split + (buffer[split] === "\r" ? 4 : 2));
              event = (raw.match(/^event:\s*(.+)$/m)?.[1] || "message").trim();
              handle(raw, event);
            }
          }
          this.closed = true;
          this.failAll("the server closed the stream");
          settle(new Error("the server closed the stream before naming an endpoint"));
        } catch (error) {
          this.closed = true;
          this.failAll("the event stream ended");
          settle(new Error(`the event stream failed: ${(error as Error).message}`));
        }
      })();
    });
    return this.ready;
  }

  private async post(message: Record<string, unknown>): Promise<void> {
    if (!this.postUrl) throw new Error("the MCP session is not open");
    const response = await fetch(this.postUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...(await this.authHeaders()) },
      body: JSON.stringify(message),
      signal: AbortSignal.timeout(MCP_REQUEST_TIMEOUT_MS),
    });
    // Legacy SSE answers on the stream and acknowledges the POST with 202; a
    // server may also answer directly, which is the same message either way.
    if (!response.ok) throw new Error(`the server answered ${response.status}`);
    const type = (response.headers.get("content-type") || "").toLowerCase();
    if (type.includes("application/json") && typeof message.id === "number") {
      const text = await response.text();
      if (text.trim()) this.onMessage(text);
    } else {
      await response.text().catch(() => "");
    }
  }

  private request(method: string, params: Record<string, unknown>): Promise<Record<string, unknown>> {
    if (this.closed) return Promise.reject(new Error("the MCP session is closed"));
    this.record("client", `${method} over SSE`);
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${this.server.name} did not answer ${method} within 20 seconds.`));
      }, MCP_REQUEST_TIMEOUT_MS);
      this.pending.set(id, { resolve, reject, timer });
      void this.post({ jsonrpc: "2.0", id, method, params }).catch((error) => {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(error as Error);
      });
    });
  }

  async handshake(): Promise<{ protocol: string; version: string; latencyMs: number }> {
    const started = Date.now();
    await this.open();
    const result = await this.request("initialize", {
      protocolVersion: MCP_PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: "sarthi-studio-vscode", version: "2.6.0" },
    });
    await this.post({ jsonrpc: "2.0", method: "notifications/initialized", params: {} });
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
    if (this.closed) return;
    this.closed = true;
    this.record("client", "closed");
    this.failAll("the session was closed");
    try {
      this.controller?.abort();
    } catch {
      // Already aborted.
    }
  }
}

/** Registry, warm sessions and the observations the panel reads. */
export class McpService {
  private sessions = new Map<string, McpWire>();
  private reports = new Map<string, McpReport>();
  private logs = new Map<string, McpLogEntry[]>();
  /** Tools of the current session, so a call checks the schema it was shown. */
  private toolCache = new Map<string, McpTool[]>();

  constructor(private readonly file: string, private readonly oauth?: McpOAuth) {}

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
      url: server.url,
      environment: Object.keys(server.env),
      // Names only: a header value is often a bearer token.
      headers: Object.keys(server.headers),
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
    this.toolCache.delete(server.name);
    if (index === -1) servers.push(server);
    else servers[index] = server;
    writeRegistry(this.file, servers);
    return server;
  }

  async remove(name: string): Promise<void> {
    const servers = this.servers().filter((entry) => entry.name !== name);
    await this.closeSession(name);
    this.reports.delete(name);
    this.toolCache.delete(name);
    writeRegistry(this.file, servers);
  }

  private async session(name: string): Promise<McpWire> {
    const server = this.find(name);
    if (!server.enabled) throw new McpError(409, `MCP server "${name}" is disabled.`);
    const warm = this.sessions.get(name);
    if (warm) return warm;
    const session: McpWire = server.transport === "http"
      ? new HttpMcpSession(server, this.oauth)
      : server.transport === "sse"
        ? new SseMcpSession(server, this.oauth)
        : new McpSession(server);
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

  /** The tools of every enabled server, as the agent prompt lists them.
      A server that fails to answer is named in `errors`, never fatal: a run
      still works when one MCP server is down. */
  async catalog(): Promise<{
    tools: Array<{ server: string; name: string; description: string; required: string[] }>;
    errors: string[];
  }> {
    const tools: Array<{ server: string; name: string; description: string; required: string[] }> = [];
    const errors: string[] = [];
    for (const server of this.servers().filter((entry) => entry.enabled)) {
      try {
        const tested = await this.test(server.name);
        for (const tool of tested.tools.slice(0, MAX_TOOLS)) {
          const schema = (tool.input_schema || {}) as Record<string, unknown>;
          const required = Array.isArray(schema.required)
            ? schema.required.filter((name): name is string => typeof name === "string" && Boolean(name))
            : [];
          tools.push({
            server: server.name,
            name: tool.name,
            description: tool.description || "",
            required,
          });
        }
      } catch (error) {
        errors.push(`${server.name}: ${(error as Error).message}`);
      }
    }
    return { tools, errors };
  }

  async test(name: string): Promise<{ tools: McpTool[]; protocol: string; version: string; latency_ms: number }> {
    const started = Date.now();
    try {
      const session = await this.session(name);
      const tools = await session.tools();
      this.toolCache.set(name, tools);
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
      const tools = this.toolCache.get(name) || (await session.tools());
      this.toolCache.set(name, tools);
      const known = tools.find((entry) => entry.name === tool);
      if (!known) throw new McpError(400, `MCP server "${name}" has no tool named "${tool}".`);
      // The server's own schema is the authority; this refuses what it would
      // certainly refuse, with a message the model can repair from.
      const problem = validateArguments(known.input_schema, args);
      if (problem) throw new McpError(400, problem);
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
    this.toolCache.delete(name);
    await session.close();
    this.logs.set(name, [...session.logs]);
    return true;
  }

  async disconnect(name: string): Promise<boolean> {
    this.find(name);
    return this.closeSession(name);
  }

  /** Close every warm session and report which ones were open. */
  async disconnectAll(): Promise<string[]> {
    const names = [...this.sessions.keys()];
    for (const name of names) {
      await this.closeSession(name).catch(() => undefined);
    }
    return names;
  }

  /** Close every warm process; the server shutdown path calls this. */
  async shutdown(): Promise<void> {
    await this.disconnectAll();
  }
}
