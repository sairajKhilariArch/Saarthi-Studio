"use strict";

/* Unsaved-buffer recovery: what a stored draft is worth restoring, and the
 * points where the editor writes and clears its mirror. The store itself is
 * Rust's and is tested there; the debounce is a timer this harness cannot run.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);
const read = (name) => fs.readFileSync(path.join(__dirname, "..", name), "utf8");

test("draftRestore offers only text the file on disk does not have", () => {
  assert.equal(call(`draftRestore({ content: "fn edited() {}" }, "fn saved() {}")`), "fn edited() {}");
  // A draft equal to the file is the leftover of a failed discard.
  assert.equal(call(`draftRestore({ content: "same" }, "same")`), "");
  // An empty draft is real: the reader had deleted every line.
  assert.equal(call(`draftRestore({ content: "" }, "text")`), "");
  // No draft, a null draft and a malformed one all mean nothing to restore.
  assert.equal(call(`draftRestore(null, "text")`), "");
  assert.equal(call(`draftRestore({}, "text")`), "");
  assert.equal(call(`draftRestore({ content: 7 }, "text")`), "");
});

test("the editor mirrors a dirty buffer and clears it on save, revert and close", () => {
  const app = read("app.js");
  assert.match(app, /const DRAFT_DELAY = 1200;/);
  assert.match(app, /scheduleDraft\(\);/);
  assert.match(app, /api\("\/api\/workspace\/draft", \{\n    method: "PUT"/);
  // A closed or saved buffer is not a draft.
  const discardCalls = app.match(/void discardDraft\(/g) || [];
  assert.ok(discardCalls.length >= 4, `discardDraft call sites: ${discardCalls.length}`);
  // Opening a file offers its draft back before the caret is placed.
  assert.match(app, /await restoreDraft\(file\);/);
});

test("a silent project switch keeps drafts; an explicit close-all discards them", () => {
  const app = read("app.js");
  const body = app.slice(app.indexOf("function closeAllFileTabs("), app.indexOf("function closeAllFileTabs(") + 1200);
  assert.match(body, /if \(!silent\) \{\n    \[\.\.\.state\.files\.values\(\)\]\.forEach\(\(file\) => \{ void discardDraft\(file\.path\); \}\);/);
});

test("a rename carries a dirty buffer's draft to its new path", () => {
  const app = read("app.js");
  assert.match(app, /async function moveDraft\(from, to\)/);
  // The draft is re-saved under the new key before the old one is discarded.
  const body = app.slice(app.indexOf("async function moveDraft"));
  const put = body.indexOf('method: "PUT"');
  // The first `discardDraft(from)` is the no-draft branch; the normal path
  // clears the old key only after the new one has been written.
  assert.ok(put !== -1, "moveDraft writes the new key");
  assert.match(body.slice(put), /await discardDraft\(from\);/, "moveDraft saves then clears");
  // Both the file branch and the folder branch of remapOpenPaths carry drafts,
  // and only a dirty buffer has one to carry.
  assert.equal((app.match(/if \(file\.content !== file\.original\) void moveDraft\(key, /g) || []).length, 2);
});

test("the server stores drafts in the database, not in the workspace", () => {
  const rust = read("../src/drafts.rs");
  assert.match(rust, /CREATE TABLE IF NOT EXISTS editor_drafts/);
  assert.match(rust, /\.route\("\/api\/workspace\/drafts", get\(list\)\)/);
  assert.match(rust, /get\(read\)\.put\(save\)\.delete\(remove\)/);
  // A deleted folder must not make its draft unreadable.
  assert.match(rust, /fn draft_key\(path: &str\)/);
  assert.match(rust, /Draft paths must stay inside the open folder/);
  const main = read("../src/main.rs");
  assert.match(main, /mod drafts;/);
  assert.match(main, /drafts::initialize\(conn\)/);
  assert.match(main, /\.merge\(drafts::router\(\)/);
});
