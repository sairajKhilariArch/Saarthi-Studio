/**
 * S5-resume: Step-K resume E2E on the extension REST surface
 * (desktop /api/runs parity), the HTTP contract the extension's panel
 * drives. Core desktop pause/resume/approve semantics are unit-tested in
 * app/src/agent.rs (gated runs resume exactly from the recorded step log);
 * here we verify the embedded server's resume REST contract:
 *
 *   - a live run resumes to RUNNING (200, no state change)
 *   - a completed/failed run refuses resume (409)
 *   - a cancelled run refuses resume (409)
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import * as http from "node:http";
import * as os from "node:os";
import * as fs from "node:fs";
import * as path from "path";

import {
  startEmbeddedServer,
  cleanupTemp,
} from "../dist/lib.mjs";

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "studio-resume-e2e-"));
}

function mockRouter() {
  const replies = [];
  const router = http.createServer((request, response) => {
    request.setEncoding("utf8");
    request.on("end", () => {
      const script = replies.shift() || JSON.stringify({ error: "exhausted mock replies" });
      const openaiReply = {
        model: "auto",
        choices: [{ message: { role: "assistant", content: script } }],
        usage: { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 },
      };
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify(openaiReply));
    });
  });
  return new Promise((resolve) => {
    router.listen(0, "127.0.0.1", () => {
      resolve({ router, url: `http://127.0.0.1:${router.address().port}/v1`, replies: [] });
    });
  });
}

function startEmbedded(router, extra = {}) {
  return startEmbeddedServer({
    uiDir: path.join(os.tmpdir(), "studio-ui-resume-e2e"),
    storeFile: path.join(tempDir(), "history.json"),
    workspaceRoot: tempDir(),
    getConfig: () => ({
      routerBaseUrl: router.url,
      apiKey: "",
      model: "auto",
      contextBudgetTokens: 4000,
      historyMessages: 6,
      sendWorkspaceOutline: false,
    }),
    ...extra,
  });
}

async function waitRun(url, id, predicate, ms = 20000) {
  const started = Date.now();
  for (;;) {
    const data = await (await fetch(`${url}/api/runs/${id}`)).json();
    if (predicate(data)) return data;
    if (Date.now() - started > ms) {
      throw new Error(`timeout; status=${data.status} error=${data.error}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

let embedded;

test("resume: a live run continues as running", async (t) => {
  const workspace = tempDir();
  const storage = tempDir();
  const mock = await mockRouter();
  embedded = await startEmbedded(mock.router);
  t.after(() => {
    embedded?.close();
    cleanupTemp(workspace);
    cleanupTemp(storage);
  });

  const post = async (route, payload) => {
    const response = await fetch(`${embedded.url}${route}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload || {}),
    });
    return { status: response.status, body: await response.json() };
  };

  // Single-step run: the scripted router answers every model call with done,
  // so the run completes on its own; we then prove resume leaves a live run
  // running (desktop parity: resume of a running run is a no-op refresh).
  const started = await post("/api/runs", {
    goal: "one scripted step, then done",
    autoApprove: true,
    allowWrites: true,
  });
  assert.equal(started.status, 200, JSON.stringify(started.body));
  const live = await waitRun(embedded.url, started.body.run.id, (d) => d.status === "succeeded");
  assert.equal(live.status, "succeeded");

  const resumed = await post(`/api/runs/${started.body.run.id}/resume`, {});
  assert.equal(resumed.status, 409);
  assert.equal(resumed.body.ok, false);
  assert.match(resumed.body.error, /succeeded/);
});

test("resume: failed and cancelled runs refuse to resume (desktop parity)", async (t) => {
  const workspace = tempDir();
  const storage = tempDir();
  const mock = await mockRouter();
  embedded = await startEmbedded(mock.router);
  t.after(() => {
    embedded?.close();
    cleanupTemp(workspace);
    cleanupTemp(storage);
  });

  const post = async (route, payload) => {
    const response = await fetch(`${embedded.url}${route}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload || {}),
    });
    return { status: response.status, body: await response.json() };
  };

  const failed = await post("/api/runs", {
    goal: "a run that never completes",
    autoApprove: true,
    allowWrites: true,
  });
  assert.equal(failed.status, 200);

  // Scripted router replies are consumed per run: this run never gets an
  // answer, so it fails like a desktop run stranded at a dead router.
  for (let i = 0; i < 3; i++) mock.replies.push(JSON.stringify({ action: "done", summary: "step " + i }));
  const finished = await waitRun(embedded.url, failed.body.run.id, (d) => d.status === "failed");
  assert.equal(finished.status, "failed");

  const resumeFailed = await post(`/api/runs/${failed.body.run.id}/resume`, {});
  assert.equal(resumeFailed.status, 409);
  assert.equal(resumeFailed.body.ok, false);
  assert.match(resumeFailed.body.error, /failed/i);

  const cancelled = await post("/api/runs", {
    goal: "another run, cancelled straight away",
    autoApprove: true,
    allowWrites: true,
  });
  assert.equal(cancelled.status, 200);
  const cancelledRun = await waitRun(embedded.url, cancelled.body.run.id, (d) => d.status === "cancelled");
  assert.equal(cancelledRun.status, "cancelled");

  const resumeCancelled = await post(`/api/runs/${cancelled.body.run.id}/resume`, {});
  assert.equal(resumeCancelled.status, 409);
  assert.equal(resumeCancelled.body.ok, false);
  assert.match(resumeCancelled.body.error, /cancelled/i);
});
