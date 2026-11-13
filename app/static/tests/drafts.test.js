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

test("the Files pane lists only drafts whose file is not already open", () => {
  // A draft for an open buffer is the buffer itself, not something to recover.
  const paths = call(
    `unopenedDrafts([{path:"a.rs"},{path:"b.rs"},{path:7},{},null], new Set(["a.rs"])).map((d) => d.path).join(",")`,
  );
  assert.equal(paths, "b.rs");
  // A missing list is an empty list, not a crash.
  assert.equal(call(`unopenedDrafts(null, new Set()).length`), 0);
  assert.equal(call(`unopenedDrafts([{path:"a.rs"}], null).length`), 1);
});

test("a recovered row shows bytes and a readable size", () => {
  assert.equal(call(`draftSize(512)`), "512 B");
  assert.equal(call(`draftSize(2048)`), "2 KB");
  assert.equal(call(`draftSize(0)`), "0 B");
  assert.equal(call(`draftSize(-1)`), "");
  assert.equal(call(`draftSize("wide")`), "");
});

test("the list and its Discard all are wired to the bulk route", () => {
  const html = read("index.html");
  assert.match(html, /id="recovered" class="recovered hidden"/);
  assert.match(html, /id="recoveredList"/);
  assert.match(html, /id="recoveredDiscard"/);
  const app = read("app.js");
  assert.match(app, /async function discardAllDrafts\(\)/);
  assert.match(app, /\$\("recoveredDiscard"\)\.addEventListener/);
  // Deleting a file or folder clears the drafts beneath it.
  assert.match(app, /\/api\/workspace\/drafts\?prefix=\$/);
  const rust = read("../src/drafts.rs");
  assert.match(rust, /get\(list\)\.delete\(clear\)/);
  assert.match(rust, /fn clear_drafts\(db: &FsPath, root: &FsPath, prefix: &str\)/);
  assert.match(rust, /path == key \|\| path\.starts_with\(under\)/);
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
  assert.match(rust, /\.route\("\/api\/workspace\/drafts", get\(list\)\.delete\(clear\)\)/);
  assert.match(rust, /get\(read\)\.put\(save\)\.delete\(remove\)/);
  // A deleted folder must not make its draft unreadable.
  assert.match(rust, /fn draft_key\(path: &str\)/);
  assert.match(rust, /Draft paths must stay inside the open folder/);
  const main = read("../src/main.rs");
  assert.match(main, /mod drafts;/);
  assert.match(main, /drafts::initialize\(conn\)/);
  assert.match(main, /\.merge\(drafts::router\(\)/);
});
