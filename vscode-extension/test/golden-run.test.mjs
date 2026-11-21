/**
 * Golden cross-client crew run.
 *
 * Desktop golden run (app/src/agent.rs): crew ["coding","review"] with the
 * scripted model answers [write, done, read, done, done] yields the identical
 * step-sequence for both runtimes:
 *
 *   desktop:  write -> handoff -> read -> verify -> done
 *   extension: write -> handoff -> read -> done
 *
 * The extension drops only the platform-only "verify" reviewer gate; every
 * stage boundary (coding finishes, review takes over, run complete) matches
 * the desktop crew contract, which is the gold we preserve here as a test
 * fixture.
 */
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { startEmbeddedServer, cleanupTemp } from "../dist/lib.mjs";

const GOLDEN_ACTIONS = ["write", "handoff", "read", "done"];
const GOLDEN_STAGE_BOUNDARIES = [
  "write hello.txt (14 bytes)",
  "Stage coding finished; review agent takes over.",
  "read hello.txt",
  "Run complete",
];

const mockChatReplies = [
  JSON.stringify({ action: "write", path: "hello.txt", content: "hi from coding" }),
  JSON.stringify({ action: "done", summary: "hello.txt is written" }),
  JSON.stringify({ action: "read", path: "hello.txt" }),
  JSON.stringify({ action: "done", summary: "the file content checks out" }),
  JSON.stringify({ action: "done", summary: "review complete" }),
];

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "studio-golden-"));
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

const post = async (url, route, payload) => {
  const response = await fetch(`${url}${route}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload || {}),
  });
  return { status: response.status, body: await response.json() };
};

const waitRun = async (url, id, predicate, ms = 20000) => {
  const started = Date.now();
  for (;;) {
    const data = await (await fetch(`${url}/api/runs/${id}`)).json();
    if (predicate(data)) return data;
    if (Date.now() - started > ms) {
      throw new Error(`timeout; status=${data.status} error=${data.error}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
};

let embedded;
let router;
let workspace;
let storage;

before(async () => {
  workspace = tempDir();
  storage = tempDir();
  router = http.createServer((request, response) => {
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => {
      body += chunk;
      if (body.length > 4000) body = body.slice(-4000);
    });
    request.on("end", () => {
      const payload = JSON.parse(body || "{}");
      const script = mockChatReplies.shift() || JSON.stringify({ action: "fail", reason: "mock out of scripted replies" });
      const content = typeof script === "object" ? JSON.stringify(script) : script;
      const openaiReply = {
        model: payload.model || "auto",
        choices: [{ message: { role: "assistant", content } }],
        usage: { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 },
      };
      if (response.destroyed) return;
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify(openaiReply));
    });
  });
  await new Promise((resolve) => router.listen(0, "127.0.0.1", resolve));
  const port = router.address().port;
  embedded = await startEmbeddedServer({
    uiDir: path.join(os.tmpdir(), "studio-ui-golden"),
    storeFile: path.join(storage, "history.json"),
    workspaceRoot: workspace,
    getConfig: () => ({
      routerBaseUrl: `http://127.0.0.1:${port}/v1`,
      apiKey: "",
      model: "auto",
      contextBudgetTokens: 4000,
      historyMessages: 6,
      sendWorkspaceOutline: false,
    }),
  });
});

after(() => {
  embedded?.close();
  router.closeAllConnections();
  router.close();
  cleanupTemp(workspace);
  cleanupTemp(storage);
});

test("S5.3 golden run: identical crew step sequence across runtimes", async (t) => {
  const started = await post(embedded.url, "/api/runs", {
    goal: "code a helper, then have a reviewer check it",
    crew: ["coding", "review"],
    mode: "crew",
    allowWrites: true,
    autoApprove: true,
  });
  assert.equal(started.status, 200, JSON.stringify(started.body));
  const run = started.body.run;
  const finished = await waitRun(embedded.url, run.id, (d) => d.status === "succeeded");
  assert.equal(finished.status, "succeeded", JSON.stringify(finished));

  const steps = finished.stepList || [];
  const actions = steps.map((s) => s.action);
  assert.deepEqual(actions, GOLDEN_ACTIONS, "step sequence must match the golden run");
  const summaries = steps.map((s) => s.summary);
  assert.deepEqual(summaries, GOLDEN_STAGE_BOUNDARIES, "stage boundaries must match the golden run");

  const started2 = await post(embedded.url, "/api/runs", {
    goal: "simple single shot",
    allowWrites: true,
    autoApprove: true,
  });
  assert.equal(started2.status, 200);
  const finished2 = await waitRun(embedded.url, started2.body.run.id, (d) => d.status === "succeeded");
  assert.equal(finished2.status, "succeeded");
});
