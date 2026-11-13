"use strict";

/* The editor's completion helpers: which word is being typed, and where its
 * replacement lands. The ranking lives in Rust behind `GET /api/index/complete`
 * and is tested there; the popup's position is layout, which this harness
 * deliberately cannot see.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

test("completionPrefix reads the identifier immediately before the caret", () => {
  assert.equal(call(`completionPrefix("let token = 1", 9)`), "token");
  assert.equal(call(`completionPrefix("let token = 1", 7)`), "tok");
  // One letter matches most of the index, so it is not a completion request.
  assert.equal(call(`completionPrefix("let t = 1", 5)`), "");
  // A member expression completes only its own name.
  assert.equal(call(`completionPrefix("self.token", 10)`), "token");
  // Digits, `_` and `$` belong to a name; punctuation and spaces do not.
  assert.equal(call(`completionPrefix("x = foo_bar$9", 13)`), "foo_bar$9");
  assert.equal(call(`completionPrefix("call(na", 7)`), "na");
  // An empty buffer, a caret at the start and a caret past the end are safe.
  assert.equal(call(`completionPrefix("", 0)`), "");
  assert.equal(call(`completionPrefix("token", 0)`), "");
  assert.equal(call(`completionPrefix("token", 99)`), "token");
});

test("completionEdit replaces only the word at the caret", () => {
  const first = call(`completionEdit("let tok = tok + 1", 7, "token")`);
  assert.equal(first.value, "let token = tok + 1");
  assert.equal(first.caret, 9);

  // The same word later in the line is not the one being completed.
  const second = call(`completionEdit("let tok = tok", 13, "token")`);
  assert.equal(second.value, "let tok = token");
  assert.equal(second.caret, 15);

  const selected = call(`completionEdit("let token = 1", 9, "tokenized")`);
  assert.equal(selected.value, "let tokenized = 1");
  assert.equal(selected.caret, 13);

  // Nothing typed: the name is inserted at the caret, not appended to the line.
  const inserted = call(`completionEdit("x = ", 4, "value")`);
  assert.equal(inserted.value, "x = value");
  assert.equal(inserted.caret, 9);
});

test("the completion popup is gated on the contract's editor_complete id", () => {
  const fs = require("node:fs");
  const path = require("node:path");
  const html = fs.readFileSync(path.join(__dirname, "..", "index.html"), "utf8");
  const box = html.indexOf('id="completions"');
  assert.ok(box >= 0, "the popup exists in the editor markup");
  const tag = html.slice(box, html.indexOf(">", box));
  assert.ok(tag.includes('data-feature="editor_complete"'), tag);
  assert.ok(tag.includes('role="listbox"'), tag);
  assert.ok(tag.includes("hidden"), "the popup starts hidden");
});
