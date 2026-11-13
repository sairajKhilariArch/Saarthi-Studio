"use strict";

/* Find references: Shift+F12 lists every indexed file that mentions the name
 * at the caret. The scan itself is Rust's and is tested there; here the wiring
 * is checked — the capability gate, the route, the shortcut, and the fact that
 * the list keeps showing each mention's own line instead of a symbol name.
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

test("the editor asks the index for references and shows each mention", () => {
  const app = read("app.js");
  assert.match(app, /capabilitiesAllow\(state\.capabilities, "editor_references"\)/);
  assert.match(app, /\/api\/index\/references\?/);
  assert.match(app, /event\.key === "F12" && event\.shiftKey/);
  // A single mention jumps; several fill the shared places list.
  assert.match(app, /revealLocation\(items\[0\]\)/);
  assert.match(app, /label: "References"/);
  // The row shows the line the name was found on, not its kind.
  assert.match(app, /item\.snippet \|\| item\.name/);
});

test("the server scans whole words and the palette offers the command", () => {
  const rust = read("../src/index.rs");
  assert.match(rust, /\.route\("\/api\/index\/references", get\(find_references\)\)/);
  assert.match(rust, /fn mention_in\(/);
  assert.match(rust, /fn is_name_char\(/);
  const ide = read("ide.js");
  assert.match(ide, /Editor: Find references/);
  const contract = read("../src/contract.rs");
  assert.match(contract, /"editor_references"/);
  const extension = fs.readFileSync(
    path.join(__dirname, "..", "..", "..", "vscode-extension", "src", "lib.ts"),
    "utf8",
  );
  assert.match(extension, /editor_references: false/);
});
