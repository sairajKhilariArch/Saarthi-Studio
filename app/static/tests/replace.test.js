"use strict";

/* Replace in file. The matching helper is pure, so it runs in a bare
 * `node:vm` context with no DOM; the find bar's row and the buffer write are
 * checked by reading the scripts that build them.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const read = (name) => fs.readFileSync(path.join(__dirname, "..", name), "utf8");
const context = vm.createContext({});
vm.runInContext(read("replace.js"), context, { filename: "replace.js" });

const run = (text, needle, replacement, limit) =>
  vm.runInContext(
    `replaceLiteral(${JSON.stringify(text)}, ${JSON.stringify(needle)}, ${JSON.stringify(replacement)}, ${
      limit === undefined ? "null" : JSON.stringify(limit)
    })`,
    context,
  );

test("every literal occurrence is replaced, ignoring case, and the end offset is past the last one", () => {
  const result = run("Foo bar foo", "foo", "baz");
  assert.equal(result.text, "baz bar baz");
  assert.equal(result.count, 2);
  assert.equal(result.end, 11);
});

test("the replacement is text, never a pattern", () => {
  // A RegExp-based replace would substitute $& and $1 here.
  const result = run("one", "one", "$& $1 $'");
  assert.equal(result.text, "$& $1 $'");
  assert.equal(result.count, 1);
});

test("an empty needle, no match or a zero limit changes nothing", () => {
  const empty = run("text", "", "x");
  assert.equal(empty.text, "text");
  assert.equal(empty.count, 0);
  assert.equal(empty.end, 0);
  const none = run("text", "missing", "x");
  assert.equal(none.text, "text");
  assert.equal(none.count, 0);
  assert.equal(none.end, 0);
  const capped = run("aaa", "a", "b", 0);
  assert.equal(capped.text, "aaa");
  assert.equal(capped.count, 0);
});

test("a limit replaces that many, and matches do not overlap", () => {
  const one = run("aa", "a", "b", 1);
  assert.equal(one.text, "ba");
  assert.equal(one.count, 1);
  assert.equal(one.end, 1);
  const pair = run("aaa", "aa", "b");
  assert.equal(pair.text, "ba");
  assert.equal(pair.count, 1);
});

test("an empty replacement deletes the matches", () => {
  const result = run("a-b-a", "a", "");
  assert.equal(result.text, "-b-");
  assert.equal(result.count, 2);
  assert.equal(result.end, 4);
});

test("the find bar has the replace controls and writes through a real input event", () => {
  const html = read("index.html");
  const scripts = [...html.matchAll(/<script src="([^"]+)"><\/script>/g)].map((match) => match[1]);
  const at = (name) => scripts.indexOf(name);
  assert.ok(at("/static/replace.js") !== -1, "replace.js is loaded");
  assert.ok(at("/static/replace.js") < at("/static/ide.js"), "before ide.js");
  assert.ok(at("/static/highlight.js") < at("/static/replace.js"), "after highlight.js");

  const ide = read("ide.js");
  assert.match(ide, /id="replaceInput"/);
  assert.match(ide, /id="replaceOne"/);
  assert.match(ide, /id="replaceAll"/);
  assert.match(ide, /replaceLiteral\(area\.value, needle, \$\("replaceInput"\)\.value, null\)/);
  assert.match(ide, /if \(mod && key === "h" && activeFile\(\)\)/);
  assert.match(ide, /area\.dispatchEvent\(new Event\("input", \{ bubbles: true \}\)\)/);
  assert.match(ide, /function openFind\(replace = false\)/);
  assert.match(read("ide.css"), /\.find-sep/);
});
