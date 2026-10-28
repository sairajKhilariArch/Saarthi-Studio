import { test } from "node:test";
import assert from "node:assert/strict";
import * as http from "node:http";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import {
  ConversationStore,
  MAX_BUDGET_TOKENS,
  cleanupTemp,
  cropText,
  estimateTokens,
  packMessages,
  probeDesktop,
  resolveInside,
  sanitizeConsolePath,
  startEmbeddedServer,
  stripAttachedCode,
  listDir,
  readTextFile,
  writeTextFile,
  createEntry,
  renameEntry,
  deleteEntry,
  workspaceOutline,
} from "../dist/lib.mjs";

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "studio-test-"));
}

function listen(handler) {
  const server = http.createServer(handler);
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      resolve({ server, url: `http://127.0.0.1:${port}` });
    });
  });
}

function readRequest(request) {
  return new Promise((resolve) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      resolve(raw ? JSON.parse(raw) : {});
    });
  });
}

/* ------------------------------------------------------------------ */
/* context packer                                                      */
/* ------------------------------------------------------------------ */

test("estimates tokens at four characters each", () => {
  assert.equal(estimateTokens(""), 0);
  assert.equal(estimateTokens("abcd"), 1);
  assert.equal(estimateTokens("abcde"), 2);
  assert.equal(estimateTokens("x".repeat(400)), 100);
});

test("crops long text but keeps both ends", () => {
  const text = `HEAD${"x".repeat(10000)}TAIL`;
  const cropped = cropText(text, 100);
  assert.ok(cropped.length <= 100 * 4 + 60);
  assert.ok(cropped.startsWith("HEAD"));
  assert.ok(cropped.endsWith("TAIL"));
  assert.match(cropped, /characters omitted/);
  assert.equal(cropText("short", 100), "short");
});

test("strips old attachments and keeps the surrounding text", () => {
  const raw = "Review this\n\nHere is the code from my editor (language: rust):\n```rust\nfn big() {}\n```";
  const stripped = stripAttachedCode(raw);
  assert.ok(stripped.startsWith("Review this"));
  assert.ok(!stripped.includes("fn big()"));
  assert.match(stripped, /attachment omitted/);
  assert.equal(stripAttachedCode("plain question"), "plain question");
});

test("packer keeps the current message and trims old ones to the budget", () => {
  const history = [
    { role: "user", content: "old question ".repeat(300) },
    { role: "assistant", content: "long answer ".repeat(600) },
    { role: "user", content: "newer question ".repeat(200) },
    { role: "user", content: "current question" },
  ];
  const packed = packMessages({ system: "Be terse.", history, budgetTokens: 1000, historyMessages: 6 });
  assert.equal(packed.messages[0].role, "system");
  assert.equal(packed.messages.at(-1).content, "current question");
  // The oldest two messages fit the budget; the two larger ones are trimmed.
  assert.equal(packed.trimmedMessages, 2);
});

test("packer honours the history window and never exceeds the ceiling", () => {
  const history = Array.from({ length: 12 }, (_, index) => ({ role: "user", content: `message ${index} ` + "y".repeat(4000) }));
  const packed = packMessages({ history, budgetTokens: MAX_BUDGET_TOKENS * 10, historyMessages: 4 });
  assert.equal(packed.messages.length, 5);
  assert.equal(packed.messages.at(-1).content.startsWith("message 11"), true);
});

test("packer includes the workspace outline when given", () => {
  const packed = packMessages({
    system: "System.",
    outline: workspaceOutlineFrom(["src/", "main.rs"]),
    history: [{ role: "user", content: "hi" }],
  });
  assert.match(packed.messages[0].content, /Workspace files/);
  assert.match(packed.messages[0].content, /main.rs/);
});

function workspaceOutlineFrom(entries) {
  return ["proj/", ...entries].join("\n");
}

/* ------------------------------------------------------------------ */
/* conversation store                                                  */
/* ------------------------------------------------------------------ */

test("store creates, appends, lists and deletes conversations", () => {
  const dir = tempDir();
  const store = new ConversationStore(path.join(dir, "history.json"));
  const conversation = store.create("  first   thread  ");
  assert.equal(conversation.title, "first thread");
  store.append(conversation.id, "user", "Explain this file");
  store.append(conversation.id, "assistant", "It does a thing.");
  assert.equal(store.messages(conversation.id).length, 2);
  assert.equal(store.has(conversation.id), true);

  store.logRequest({
    created_at: 1,
    conversation_id: conversation.id,
    model: "auto",
    provider: "mock",
    status: "ok",
    latency_ms: 120,
    prompt_tokens: 10,
    completion_tokens: 20,
    total_tokens: 30,
    error: "",
  });
  const usage = store.usage();
  assert.equal(usage.summary.requests, 1);
  assert.equal(usage.summary.total_tokens, 30);

  // Reloading from disk keeps the history (it survives a server restart).
  const reloaded = new ConversationStore(path.join(dir, "history.json"));
  assert.equal(reloaded.messages(conversation.id).length, 2);
  assert.equal(reloaded.remove(conversation.id), true);
  assert.equal(reloaded.has(conversation.id), false);
  cleanupTemp(dir);
});

/* ------------------------------------------------------------------ */
/* workspace rules                                                     */
/* ------------------------------------------------------------------ */

test("workspace refuses traversal and absolute paths", () => {
  const root = tempDir();
  fs.writeFileSync(path.join(root, "a.txt"), "hello");
  assert.equal(resolveInside(root, "a.txt"), fs.realpathSync(path.join(root, "a.txt")));
  assert.throws(() => resolveInside(root, "../etc/passwd"), /leaves the open folder/);
  assert.throws(() => resolveInside(root, "/etc/passwd"), /relative to the open folder/);
  assert.throws(() => resolveInside(root, "a/../../b"), /leaves the open folder/);
  cleanupTemp(root);
});

test("workspace read, write, create, rename and delete stay inside the root", () => {
  const root = tempDir();
  writeTextFile(root, "notes/todo.txt", "first");
  assert.equal(readTextFile(root, "notes/todo.txt").content, "first");
  createEntry(root, "notes/sub", true);
  const listing = listDir(root, "notes");
  assert.deepEqual(listing.entries.map((entry) => entry.name).sort(), ["sub", "todo.txt"]);
  renameEntry(root, "notes/todo.txt", "notes/done.txt");
  assert.equal(fs.existsSync(path.join(root, "notes/done.txt")), true);
  deleteEntry(root, "notes/done.txt");
  assert.equal(fs.existsSync(path.join(root, "notes/done.txt")), false);
  assert.throws(() => deleteEntry(root, ""), /Refusing to delete/);
  cleanupTemp(root);
});

/* ------------------------------------------------------------------ */
/* console path guard                                                  */
/* ------------------------------------------------------------------ */

test("console paths are limited to admin API families", () => {
  assert.equal(sanitizeConsolePath("/api/keys"), "keys");
  assert.equal(sanitizeConsolePath("keys/12"), "keys/12");
  assert.equal(sanitizeConsolePath("fallback/routing/"), "fallback/routing");
  for (const bad of ["", "v1/chat/completions", "keys/../../auth", "keys//1", "auth/login", "keys?x=1"]) {
    assert.throws(() => sanitizeConsolePath(bad), /admin|path/i, `${bad} should be refused`);
  }
});

/* ------------------------------------------------------------------ */
/* desktop probe                                                       */
/* ------------------------------------------------------------------ */

test("probe reports a running embeddable desktop app", async () => {
  const { server, url } = await listen((request, response) => {
    if (request.url === "/api/health") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ ok: true, embeddable: true }));
      return;
    }
    response.writeHead(404);
    response.end();
  });
  const probe = await probeDesktop(url, fetch, 5000);
  assert.equal(probe.running, true);
  assert.equal(probe.embeddable, true);
  server.closeAllConnections();
  server.close();

  const offline = await probeDesktop("http://127.0.0.1:1");
  assert.equal(offline.running, false);
  // Non-loopback addresses are never probed.
  assert.equal((await probeDesktop("http://example.com:8765")).running, false);
});

/* ------------------------------------------------------------------ */
/* embedded server                                                     */
/* ------------------------------------------------------------------ */

test("built-in server chats through the router and serves the workspace", async () => {
  const workspace = tempDir();
  fs.writeFileSync(path.join(workspace, "app.js"), "console.log('hi');");
  const storage = tempDir();

  const router = await listen(async (request, response) => {
    const body = await readRequest(request);
    if (request.url === "/v1/chat/completions") {
      const messages = body.messages || [];
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({
        model: "auto",
        provider: "mock",
        choices: [{ message: { content: `saw ${messages.length} messages` } }],
        usage: { prompt_tokens: 12, completion_tokens: 6, total_tokens: 18 },
      }));
      return;
    }
    response.writeHead(404);
    response.end("{}");
  });

  const embedded = await startEmbeddedServer({
    uiDir: path.join(process.cwd(), "media", "ui"),
    storeFile: path.join(storage, "history.json"),
    workspaceRoot: workspace,
    routerPort: 1,
    getConfig: () => ({
      routerBaseUrl: `${router.url}/v1`,
      apiKey: "test-key",
      model: "auto",
      contextBudgetTokens: 4000,
      historyMessages: 4,
      sendWorkspaceOutline: true,
    }),
  });

  const post = async (route, payload) => {
    const response = await fetch(`${embedded.url}${route}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    return { status: response.status, body: await response.json() };
  };

  const health = await (await fetch(`${embedded.url}/api/health`)).json();
  assert.equal(health.ok, true);
  assert.equal(health.embeddable, true);

  const created = await post("/api/conversations", { title: "Smoke test" });
  assert.equal(created.status, 200);
  const id = created.body.id;

  const chat = await post("/api/chat", { conversation_id: id, message: "hello", code: "print(1)", language: "python" });
  assert.equal(chat.status, 200);
  assert.equal(chat.body.ok, true);
  assert.match(chat.body.answer, /saw \d+ messages/);
  assert.ok(chat.body.meta.context_tokens > 0);

  const messages = await (await fetch(`${embedded.url}/api/conversations/${id}/messages`)).json();
  assert.equal(messages.length, 2);
  assert.equal(messages[0].role, "user");
  assert.match(messages[0].content, /Here is the code from my editor/);

  const usage = await (await fetch(`${embedded.url}/api/usage`)).json();
  assert.equal(usage.summary.requests, 1);
  assert.equal(usage.summary.total_tokens, 18);

  const file = await (await fetch(`${embedded.url}/api/workspace/file?path=app.js`)).json();
  assert.equal(file.content, "console.log('hi');");

  const traversal = await fetch(`${embedded.url}/api/workspace/file?path=../secret`);
  assert.equal(traversal.status, 400);

  const written = await fetch(`${embedded.url}/api/workspace/file`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ path: "new.txt", content: "saved" }),
  });
  assert.equal(written.status, 200);
  assert.equal(fs.readFileSync(path.join(workspace, "new.txt"), "utf8"), "saved");

  const index = await fetch(`${embedded.url}/`);
  assert.equal(index.status, 200);
  assert.match(await index.text(), /Saarthi Studio/);

  await embedded.close();
  router.server.closeAllConnections();
  router.server.close();
  cleanupTemp(workspace);
  cleanupTemp(storage);
});

test("built-in server proxies the console and refuses non-admin paths", async () => {
  const workspace = tempDir();
  const storage = tempDir();
  const calls = [];

  const router = await listen(async (request, response) => {
    calls.push(`${request.method} ${request.url}`);
    if (request.url === "/api/ping") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ status: "ok" }));
      return;
    }
    if (request.url === "/api/auth/desktop-session") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ token: "desktop-token" }));
      return;
    }
    if (request.url === "/api/keys") {
      const authorized = request.headers.authorization === "Bearer desktop-token";
      response.writeHead(authorized ? 200 : 401, { "Content-Type": "application/json" });
      response.end(JSON.stringify(authorized ? { keys: [] } : { error: "unauthorized" }));
      return;
    }
    response.writeHead(404, { "Content-Type": "application/json" });
    response.end("{}");
  });
  const routerPort = Number(new URL(router.url).port);

  const embedded = await startEmbeddedServer({
    uiDir: path.join(process.cwd(), "media", "ui"),
    storeFile: path.join(storage, "history.json"),
    workspaceRoot: workspace,
    routerPort,
    getConfig: () => ({
      routerBaseUrl: `${router.url}/v1`,
      apiKey: "",
      model: "auto",
      contextBudgetTokens: 6000,
      historyMessages: 6,
      sendWorkspaceOutline: true,
    }),
  });

  const status = await (await fetch(`${embedded.url}/api/router/console/status`)).json();
  assert.equal(status.reachable, true);
  assert.equal(status.authenticated, true);

  const keys = await fetch(`${embedded.url}/api/router/console/keys`);
  assert.equal(keys.status, 200);
  assert.deepEqual(await keys.json(), { keys: [] });
  assert.ok(calls.some((call) => call === "GET /api/keys"));

  const refused = await fetch(`${embedded.url}/api/router/console/v1/chat/completions`);
  assert.equal(refused.status, 400);

  await embedded.close();
  router.server.closeAllConnections();
  router.server.close();
  cleanupTemp(workspace);
  cleanupTemp(storage);
});
