"use strict";

/* Token rules for the editor's highlight overlay. The module is pure — text in,
 * escaped HTML out — so it runs in a bare `node:vm` context with no DOM at all.
 * The browser's rendering of these spans is layout and is not tested here.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "..", "highlight.js"), "utf8");
const context = vm.createContext({});
vm.runInContext(source, context, { filename: "highlight.js" });

const highlight = (text, language) =>
  vm.runInContext(`highlightCode(${JSON.stringify(text)}, ${JSON.stringify(language)})`, context);
const call = (expression) => vm.runInContext(expression, context);

test("rust colours keywords, types, numbers and comments", () => {
  const html = highlight("let x: i32 = 42; // hi", "rust");
  assert.match(html, /<span class="tok-key">let<\/span>/);
  assert.match(html, /<span class="tok-type">i32<\/span>/);
  assert.match(html, /<span class="tok-num">42<\/span>/);
  assert.match(html, /<span class="tok-com">\/\/ hi<\/span>/);
});

test("a rust lifetime is not a character literal", () => {
  const lifetime = highlight("fn f<'a>(x: &'a str) {}", "rust");
  assert.ok(!lifetime.includes("tok-str"), lifetime);
  const character = highlight("let c = 'x';", "rust");
  assert.match(character, /<span class="tok-str">&#39;x&#39;<\/span>/);
});

test("code is escaped, so a buffer can never inject markup", () => {
  const script = highlight('<script>alert("x")</script>', "javascript");
  assert.ok(!script.includes("<script>"), script);
  assert.ok(script.includes("&lt;script&gt;"), script);
  // Inside a string the angle brackets are still escaped, only the span wraps it.
  const string = highlight('const a = "<b>";', "javascript");
  assert.ok(!string.includes("<b>"), string);
  assert.match(string, /<span class="tok-str">&quot;&lt;b&gt;&quot;<\/span>/);
});

test("a URL inside a string is not mistaken for a comment", () => {
  const url = highlight('const u = "http://x"; // done', "javascript");
  assert.ok(!url.includes('<span class="tok-com">http'), url);
  assert.match(url, /tok-str">&quot;http:\/\/x&quot;/);
  assert.match(url, /<span class="tok-com">\/\/ done<\/span>/);
});

test("an unterminated string stops at the line end", () => {
  const html = highlight('let a = "open\nlet b = 1;', "javascript");
  assert.match(html, /<span class="tok-str">&quot;open<\/span>/);
  assert.match(html, /tok-key">let<\/span> b = <span class="tok-num">1<\/span>/);
});

test("json keys are properties and its literals are keywords", () => {
  const html = highlight('{"a": 1, "b": true}', "json");
  assert.match(html, /<span class="tok-prop">&quot;a&quot;<\/span>/);
  assert.match(html, /<span class="tok-num">1<\/span>/);
  assert.match(html, /<span class="tok-key">true<\/span>/);
});

test("css colours a property and a hex value", () => {
  const html = highlight(".a { color: #ff8a3d; }", "css");
  assert.match(html, /<span class="tok-prop">color<\/span>/);
  assert.match(html, /<span class="tok-num">#ff8a3d<\/span>/);
});

test("markup colours the tag, the attribute and its value", () => {
  const html = highlight('<a href="x">hi</a>', "html");
  assert.match(html, /<span class="tok-tag">a<\/span>/);
  assert.match(html, /<span class="tok-attr">href<\/span>/);
  assert.match(html, /<span class="tok-str">&quot;x&quot;<\/span>/);
  assert.match(html, /<span class="tok-tag">a<\/span>&gt;/, "the closing tag is coloured too");
});

test("markdown colours headings, fences, inline code, links and list markers", () => {
  const html = highlight("# Title\n\n- [x](http://y)\n\n```\ncode\n```", "markdown");
  assert.match(html, /<span class="tok-key"># Title<\/span>/);
  assert.match(html, /<span class="tok-punct">-<\/span>/);
  assert.match(html, /<span class="tok-fn">\[x\]<\/span>/);
  assert.match(html, /<span class="tok-str">\(http:\/\/y\)<\/span>/);
  assert.match(html, /<span class="tok-str">```<\/span>/);
  assert.match(html, /<span class="tok-str">code<\/span>/);
  // A fence that closes ends the block; the following line is plain again.
  const after = highlight("```\na\n```\n# Head", "markdown");
  assert.match(after, /<span class="tok-key"># Head<\/span>/);
});

test("an unknown language or an oversized buffer comes back escaped and plain", () => {
  assert.equal(highlight("<b>hi</b>", "text"), "&lt;b&gt;hi&lt;/b&gt;");
  assert.equal(highlight("<b>hi</b>", "brainfuck"), "&lt;b&gt;hi&lt;/b&gt;");
  const huge = "x".repeat(200001);
  assert.equal(highlight(huge, "rust"), huge);
  // The cap is on the language path, not on escaping.
  assert.equal(highlight("x".repeat(200001), "text").length, 200001);
});

test("support is decided by a known grammar, not by the file name", () => {
  assert.equal(call(`highlightSupported("rust")`), true);
  assert.equal(call(`highlightSupported("yml")`), true);
  assert.equal(call(`highlightSupported("markdown")`), true);
  assert.equal(call(`highlightSupported("text")`), false);
  assert.equal(call(`highlightSupported("")`), false);
});

test("sql keywords match regardless of case, as sql itself does", () => {
  const html = highlight("SELECT id FROM users where id = 1", "sql");
  assert.match(html, /<span class="tok-key">SELECT<\/span>/);
  assert.match(html, /<span class="tok-key">FROM<\/span>/);
  assert.match(html, /<span class="tok-key">where<\/span>/);
});

test("the workbench delegates its highlighting instead of keeping a second engine", () => {
  const ide = fs.readFileSync(path.join(__dirname, "..", "ide.js"), "utf8");
  assert.match(ide, /typeof highlightCode === "function"/, "ide.js calls the module");
  assert.match(ide, /function paintCode\(/, "the workbench still owns the layer");
  assert.ok(!/function patternFor|function familyFor/.test(ide), "the old regex engine is gone");
  const html = fs.readFileSync(path.join(__dirname, "..", "index.html"), "utf8");
  const moduleAt = html.indexOf("/static/highlight.js");
  const ideAt = html.indexOf("/static/ide.js");
  assert.ok(moduleAt > 0 && ideAt > moduleAt, "highlight.js loads before ide.js");
  assert.ok(!html.includes('id="highlight"'), "the markup holds no second overlay");
});
