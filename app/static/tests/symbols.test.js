"use strict";

/* The file outline: Ctrl+Shift+O lists the open file's indexed declarations.
 * The route and its source ordering are tested in Rust; here the wiring — the
 * listbox, the contract id, the shortcut and the palette entry — is checked.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { loadApp } = require("./harness");

loadApp();

const read = (name) => fs.readFileSync(path.join(__dirname, "..", name), "utf8");

test("the outline listbox is gated on the contract's editor_symbols id", () => {
  const html = read("index.html");
  const box = html.indexOf('id="symbols"');
  assert.ok(box >= 0, "the outline exists in the editor markup");
  const tag = html.slice(box, html.indexOf(">", box));
  assert.match(tag, /data-feature="editor_symbols"/);
  assert.match(tag, /role="listbox"/);
});

test("the editor asks the index for one file and jumps to the symbol", () => {
  const app = read("app.js");
  assert.match(app, /capabilitiesAllow\(state\.capabilities, "editor_symbols"\)/);
  assert.match(app, /\/api\/index\/symbols\?/);
  assert.match(app, /event\.code === "KeyO" && event\.shiftKey/);
  assert.match(app, /revealSymbol/);
  // The jump reuses the declaration placement rather than a second copy.
  assert.match(app, /definitionCaret\(area\.value, item\.line, item\.name\)/);
});

test("the palette and the server expose the outline", () => {
  const ide = read("ide.js");
  assert.match(ide, /Editor: Go to symbol in file/);
  const rust = read("../src/index.rs");
  assert.match(rust, /\.route\("\/api\/index\/symbols", get\(file_symbols\)\)/);
  const contract = read("../src/contract.rs");
  assert.match(contract, /"editor_symbols"/);
  const extension = fs.readFileSync(
    path.join(__dirname, "..", "..", "..", "vscode-extension", "src", "lib.ts"),
    "utf8",
  );
  assert.match(extension, /editor_symbols: false/);
});
