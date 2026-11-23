/**
 * S5-template: crew templates — storage / API / test on the extension REST
 * surface. Desktop parity (app/src/main.rs): crew templates are a JSON file
 * (~/.config/SAARTHI-STUDIO/crew-templates.json) with list/save/delete.
 *
 *   - save:    POST /api/crew-templates  { crew: [role, ...] } -> { ok, name }
 *   - get:     GET  /api/crew-templates  -> { ok: true, templates: [...] }
 *   - delete:  DELETE /api/crew-templates/:name
 *   - validate: crew must be an array of non-empty PROFILES role names
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import * as http from "node:http";
import * as os from "node:os";
import * as fs from "node:fs";
import * as path from "path";

import { startEmbeddedServer, cleanupTemp } from "../dist/lib.mjs";

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "studio-crew-template-"));
}

function mockRouter(extraReplies = []) {
  const replies = [...extraReplies];
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

function startEmbedded(router) {
  return startEmbeddedServer({
    uiDir: path.join(os.tmpdir(), "studio-crew-ui-e2e"),
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
  });
}

function request(embedded, method, route, payload) {
  return fetch(`${embedded.url}${route}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: payload === undefined ? undefined : JSON.stringify(payload),
  });
}

let embedded;

test("crew template persistence: create, list, delete", async (t) => {
  const mock = await mockRouter();
  embedded = await startEmbedded(mock.router);
  t.after(() => {
    embedded?.close();
    mock.router.closeAllConnections();
    mock.router.close();
    cleanupTemp();
  });

  const save = (payload) =>
    request(embedded, "POST", "/api/crew-templates", payload);

  // 1. the first roster becomes template "crew-1"
  const first = await save({ crew: ["coding", "review"] });
  assert.equal(first.status, 200, JSON.stringify(await first.json()));
  const firstBody = await first.json();
  assert.equal(firstBody.ok, true);
  assert.equal(firstBody.name, "crew-1");

  // 2. a second roster becomes "crew-2" and both are retained
  const second = await save({ crew: ["writing", "testing"] });
  assert.equal(second.status, 200, JSON.stringify(await second.json()));
  const secondBody = await second.json();
  assert.equal(secondBody.ok, true);
  assert.equal(secondBody.name, "crew-2");

  // 3. GET lists both rosters verbatim
  const list = await request(embedded, "GET", "/api/crew-templates");
  assert.equal(list.status, 200);
  const listBody = await list.json();
  assert.equal(listBody.ok, true);
  assert.equal(listBody.templates.length, 2);
  assert.ok(listBody.templates.some((t) => t.name === "crew-1"));
  assert.ok(listBody.templates.some((t) => t.name === "crew-2"));
  assert.deepEqual(
    listBody.templates.map((t) => t.crew).sort((a, b) => a[0] < b[0] ? -1 : 1),
    [["coding", "review"], ["writing", "testing"]],
  );

  // 4. a roster that is not an array of role names is refused
  const bad = await save({ crew: ["coding", 3] });
  assert.equal(bad.status, 400, JSON.stringify(await bad.json()));
  const still = await request(embedded, "GET", "/api/crew-templates");
  assert.equal((await still.json()).templates.length, 2);

  // 5. delete one roster; the other survives
  const del = await request(embedded, "DELETE", "/api/crew-templates/crew-1");
  assert.equal(del.status, 200, JSON.stringify(await del.json()));
  const after = await request(embedded, "GET", "/api/crew-templates");
  assert.equal((await after.json()).templates.length, 1);

  // 6. deleting a name that was never stored is refused
  const gone = await request(embedded, "DELETE", "/api/crew-templates/crew-99");
  assert.equal(gone.status, 404, JSON.stringify(await gone.json()));

  // 7. re-adding the deleted roster starts a fresh template id
  const again = await save({ crew: ["coding", "review"] });
  assert.equal(again.status, 200);
  assert.equal((await again.json()).name, "crew-2");
});

test("crew template API refuses a roster that is not role names", async (t) => {
  const mock = await mockRouter([JSON.stringify({ action: "done" })]);
  embedded = await startEmbedded(mock.router);
  t.after(() => {
    embedded?.close();
    mock.router.closeAllConnections();
    mock.router.close();
    cleanupTemp();
  });

  const bad = await request(embedded, "POST", "/api/crew-templates", {
    crew: ["coding", 3],
  });
  assert.equal(bad.status, 400, JSON.stringify(await bad.json()));

  const missing = await request(embedded, "POST", "/api/crew-templates", { crew: [] });
  assert.equal(missing.status, 400, JSON.stringify(await missing.json()));

  const nonString = await request(embedded, "POST", "/api/crew-templates", { crew: 5 });
  assert.equal(nonString.status, 400, JSON.stringify(await nonString.json()));
});
