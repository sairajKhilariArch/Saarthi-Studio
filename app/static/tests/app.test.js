"use strict";

/* Frontend unit tests for the static UI.
 *
 * The UI has no build step and no package manager, so this uses Node's own test
 * runner against the real `app/static/app.js` loaded through `harness.js`. Only
 * pure behaviour is asserted here — escaping, formatting, path handling — because
 * anything that needs a layout cannot be verified without a browser, and a test
 * that pretends otherwise is worse than none.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

test("the UI script evaluates outside a browser", () => {
  // If this fails, every other test is meaningless: the harness no longer loads
  // the real file.
  assert.equal(call("typeof escapeHtml"), "function");
  assert.equal(call("typeof renderMarkdown"), "function");
});

test("escapeHtml neutralises all five characters and stringifies empties", () => {
  assert.equal(
    call(`escapeHtml('<a href="x">&\\'y\\'</a>')`),
    "&lt;a href=&quot;x&quot;&gt;&amp;&#39;y&#39;&lt;/a&gt;",
  );
  assert.equal(call("escapeHtml(null)"), "");
  assert.equal(call("escapeHtml(undefined)"), "");
  assert.equal(call("escapeHtml(0)"), "0");
});

test("inlineMarkdown escapes HTML before it applies any formatting", () => {
  const escaped = call(`inlineMarkdown("<script>alert('x')</script>")`);
  assert.ok(!escaped.includes("<script>"), escaped);
  assert.ok(escaped.includes("&lt;script&gt;"), escaped);

  // A tag typed inside bold is still inert.
  const bold = call(`inlineMarkdown("**<img src=x onerror=alert(1)>**")`);
  assert.ok(!bold.includes("<img"), bold);
  assert.ok(bold.includes("<strong>"), bold);
});

test("inlineMarkdown renders the supported subset and keeps inline code literal", () => {
  assert.equal(call(`inlineMarkdown("**bold**")`), "<strong>bold</strong>");
  assert.equal(call(`inlineMarkdown("*em*")`), "<em>em</em>");
  assert.equal(call(`inlineMarkdown("~~gone~~")`), "<del>gone</del>");
  assert.equal(call("inlineMarkdown('a\\nb')"), "a<br>b");

  // Code is extracted before the emphasis rules, so stars inside it survive.
  const code = call("inlineMarkdown('`a*b*c`')");
  assert.equal(code, "<code>a*b*c</code>");
});

test("inlineMarkdown only links http(s), never a javascript: URL", () => {
  const link = call(`inlineMarkdown("[docs](https://example.com/x)")`);
  assert.equal(
    link,
    '<a href="https://example.com/x" target="_blank" rel="noopener noreferrer">docs</a>',
  );

  const hostile = call(`inlineMarkdown("[x](javascript:alert(1))")`);
  assert.ok(!hostile.includes("<a "), hostile);
  assert.ok(hostile.includes("javascript:alert(1)"), "it stays visible text");
});

test("renderMarkdown builds paragraphs, headings, lists and fences", () => {
  assert.equal(call(`renderMarkdown("hello")`), "<p>hello</p>");
  assert.equal(call(`renderMarkdown("## Title")`), "<h3>Title</h3>");
  assert.equal(
    call(`renderMarkdown("- one\\n- two")`),
    "<ul><li>one</li><li>two</li></ul>",
  );
  const fenced = call("renderMarkdown('```js\\nconst a = 1;\\n```')");
  assert.ok(fenced.includes('<span>js</span>'), fenced);
  assert.ok(fenced.includes("const a = 1;"), fenced);
  // A blank line closes a paragraph rather than adding an empty one.
  assert.equal(call(`renderMarkdown("a\\n\\nb")`), "<p>a</p><p>b</p>");
});

test("formatBytes reports the unit it actually measured", () => {
  assert.equal(call("formatBytes(0)"), "0 B");
  assert.equal(call("formatBytes(1023)"), "1023 B");
  assert.equal(call("formatBytes(1024)"), "1.0 KB");
  assert.equal(call("formatBytes(1536)"), "1.5 KB");
  assert.equal(call("formatBytes(1048576)"), "1.0 MB");
  assert.equal(call("formatBytes(-1)"), "—");
  assert.equal(call("formatBytes(NaN)"), "—");
});

test("basename and parentPath handle trailing slashes and roots", () => {
  assert.equal(call(`basename("/a/b/c.js")`), "c.js");
  assert.equal(call(`basename("/a/b/")`), "b");
  assert.equal(call(`basename("")`), "");
  assert.equal(call(`parentPath("/a/b/c.js")`), "/a/b");
  assert.equal(call(`parentPath("/a")`), "");
  assert.equal(call(`parentPath("/a/")`), "");
  assert.equal(call(`parentPath("plain")`), "");
});

test("relativeTime uses the bucket boundaries it claims", () => {
  const now = 1_800_000_000;
  assert.equal(call(`relativeTime(${now - 30}, ${now})`), "just now");
  assert.equal(call(`relativeTime(${now - 59}, ${now})`), "just now");
  assert.equal(call(`relativeTime(${now - 60}, ${now})`), "1m ago");
  assert.equal(call(`relativeTime(${now - 3600}, ${now})`), "1h ago");
  assert.equal(call(`relativeTime(${now - 86400}, ${now})`), "1d ago");
  assert.equal(call(`relativeTime(0, ${now})`), "");
  assert.equal(call(`relativeTime("nope", ${now})`), "");
});

test("keyPath maps only the tab keys it knows", () => {
  assert.equal(call(`keyPath("chat")`), null);
  assert.equal(call(`keyPath("file:src/a.js")`), "src/a.js");
  assert.equal(call(`keyPath("scratch")`), "scratch");
  assert.equal(call(`keyPath("something-else")`), null);
});

test("contextMetaTitle says what was actually attached, and counts correctly", () => {
  const one = call(`contextMetaTitle({ auto_skills: ["deploy"] })`);
  assert.equal(one, "matched skill: deploy");

  const two = call(`contextMetaTitle({ auto_skills: ["deploy", "test"] })`);
  assert.equal(two, "matched skills: deploy, test");

  const memory = call(`contextMetaTitle({ memory_used: ["use pnpm"] })`);
  assert.equal(memory, "memory given to the model: use pnpm");

  const dropped = call("contextMetaTitle({ context_dropped: 3 })");
  assert.equal(dropped, "3 message(s) not sent at all");

  assert.equal(call("contextMetaTitle({})"), "");
});

test("icon falls back to the raw name rather than rendering an undefined id", () => {
  const known = call(`icon("check")`);
  assert.ok(known.includes("href=\"#"), known);
  assert.ok(!known.includes("undefined"), known);

  const unknown = call(`icon("not-a-mapped-icon")`);
  assert.ok(unknown.includes("not-a-mapped-icon"), unknown);
  assert.ok(!unknown.includes("undefined"), unknown);
});
