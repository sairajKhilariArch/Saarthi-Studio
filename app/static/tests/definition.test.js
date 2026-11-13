"use strict";

/* Go to definition: the word under the caret, where its name sits on the
 * declaration line, and the wiring that keeps the chooser behind the
 * `editor_definition` capability. The ranking is Rust's and is tested there;
 * the jump itself is layout, which this harness deliberately cannot see.
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
// Values made inside the vm belong to its realm; a plain copy compares cleanly.
const shape = (expression) => ({ ...call(expression) });

test("wordAt reads the name the caret is on, not only the one before it", () => {
  assert.equal(call(`wordAt("let login = 1", 8)`), "login");
  // The middle of a name is still that name.
  assert.equal(call(`wordAt("let login = 1", 6)`), "login");
  // Immediately after a name, before the space.
  assert.equal(call(`wordAt("let login = 1", 9)`), "login");
  // A caret in whitespace is on no name at all.
  assert.equal(call(`wordAt("let  x", 4)`), "");
  assert.equal(call(`wordAt("", 0)`), "");
  // The caret past the end of the buffer is clamped.
  assert.equal(call(`wordAt("login", 99)`), "login");
  // `$`, `_` and digits belong to a name; `::` is not part of one.
  assert.equal(call(`wordAt("self::foo_bar$9()", 13)`), "foo_bar$9");
});

test("definitionCaret lands on the declared name, not at column one", () => {
  const text = "use crate::auth;\npub fn login(user: &str) -> bool {\n    true\n}";
  assert.deepEqual(shape(`definitionCaret(${JSON.stringify(text)}, 2, "login")`), {
    line: 2,
    column: 8,
  });
  // A name that only appears inside a longer identifier is not the match.
  const shadow = "fn login_token() {}\nfn login() {}";
  assert.deepEqual(shape(`definitionCaret(${JSON.stringify(shadow)}, 1, "login")`), {
    line: 1,
    column: 1,
  });
  assert.deepEqual(shape(`definitionCaret(${JSON.stringify(shadow)}, 2, "login")`), {
    line: 2,
    column: 4,
  });
  // A declaration whose line does not repeat the name falls back to its start.
  const split = ["struct", "user;"].join(String.fromCharCode(10));
  assert.deepEqual(shape(`definitionCaret(${JSON.stringify(split)}, 2, "User")`), {
    line: 2,
    column: 1,
  });
  // Lines outside the buffer are clamped to it.
  assert.deepEqual(shape(`definitionCaret("one", 99, "one")`), { line: 1, column: 1 });
});

test("the chooser is gated on the contract's editor_definition id", () => {
  const html = fs.readFileSync(path.join(__dirname, "..", "index.html"), "utf8");
  const box = html.indexOf('id="definitions"');
  assert.ok(box >= 0, "the chooser exists in the editor markup");
  const tag = html.slice(box, html.indexOf(">", box));
  assert.match(tag, /data-feature="editor_definition"/);
  assert.match(tag, /role="listbox"/);

  const app = fs.readFileSync(path.join(__dirname, "..", "app.js"), "utf8");
  assert.match(app, /capabilitiesAllow\(state\.capabilities, "editor_definition"\)/);
  assert.match(app, /\/api\/index\/definition\?/);
  assert.match(app, /event\.key === "F12"/);
  assert.match(app, /acceptDefinition/);

  const rust = fs.readFileSync(path.join(__dirname, "..", "..", "src", "index.rs"), "utf8");
  assert.match(rust, /\.route\("\/api\/index\/definition", get\(definition_symbols\)\)/);

  const contract = fs.readFileSync(path.join(__dirname, "..", "..", "src", "contract.rs"), "utf8");
  assert.match(contract, /"editor_definition"/);
});
