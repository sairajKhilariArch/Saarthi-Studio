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

test("the workflow canvas lays nodes out by dependency depth", () => {
  const graph = `[
    { id: "a", kind: "task" },
    { id: "b", kind: "task", dependsOn: ["a"] },
    { id: "c", kind: "task", dependsOn: [{ id: "b", when: "failed" }] },
    { id: "d", kind: "task" }
  ]`;
  assert.equal(call(`(() => { const p = graphLayout(${graph}); return p.get("a").x === p.get("d").x; })()`), true);
  assert.equal(call(`(() => { const p = graphLayout(${graph}); return p.get("b").x > p.get("a").x; })()`), true);
  assert.equal(call(`(() => { const p = graphLayout(${graph}); return p.get("c").x > p.get("b").x; })()`), true);
  assert.equal(call(`graphLayout(${graph}).get("a").y`), 24);
});

test("a cyclic graph still lays out and terminates", () => {
  const size = call(`(() => {
    const layout = graphLayout([
      { id: "a", kind: "task", dependsOn: ["b"] },
      { id: "b", kind: "task", dependsOn: ["a"] }
    ]);
    return layout.size;
  })()`);
  assert.equal(size, 2);
});

test("renaming a node rewrites the dependencies that point at it", () => {
  call(`workflowGraph = [
    { id: "plan", kind: "task" },
    { id: "ship", kind: "task", dependsOn: [{ id: "plan", when: "failed" }] }
  ]`);
  call(`renameWorkflowNode("plan", "draft")`);
  const graph = JSON.parse(call("JSON.stringify(workflowGraph)"));
  assert.equal(graph[0].id, "draft");
  assert.deepEqual(graph[1].dependsOn, [{ id: "draft", when: "failed" }]);
});

test("deleting a node removes the edges that pointed at it", () => {
  call(`workflowGraph = [
    { id: "a", kind: "task" },
    { id: "b", kind: "task", dependsOn: ["a"] }
  ]`);
  call(`selectedWorkflowNode = "a"`);
  call("deleteWorkflowNode()");
  const graph = JSON.parse(call("JSON.stringify(workflowGraph)"));
  assert.equal(graph.length, 1);
  assert.deepEqual(graph[0].dependsOn, []);
});

test("adding a node gives it a unique id and the kind's defaults", () => {
  call(`workflowGraph = [{ id: "task-2", kind: "task" }]`);
  call(`addWorkflowNode("task")`);
  const graph = JSON.parse(call("JSON.stringify(workflowGraph)"));
  assert.equal(graph.length, 2);
  assert.notEqual(graph[0].id, graph[1].id);
  assert.equal(graph[1].kind, "task");
  assert.equal(graph[1].note, "");
  call(`addWorkflowNode("agent")`);
  const agent = JSON.parse(call("JSON.stringify(workflowGraph[2])"));
  assert.equal(agent.prompt, "");
  assert.equal(agent.retries, 1);
});

test("a dependency keeps its failure condition when it is edited", () => {
  call(`workflowGraph = [
    { id: "a", kind: "task" },
    { id: "b", kind: "task", dependsOn: ["a"] }
  ]`);
  call(`setWorkflowDependency("b", "a", true, "failed")`);
  let graph = JSON.parse(call("JSON.stringify(workflowGraph)"));
  assert.deepEqual(graph[1].dependsOn, [{ id: "a", when: "failed" }]);
  call(`setWorkflowDependency("b", "a", false)`);
  graph = JSON.parse(call("JSON.stringify(workflowGraph)"));
  assert.deepEqual(graph[1].dependsOn, []);
});

test("icon falls back to the raw name rather than rendering an undefined id", () => {
  const known = call(`icon("check")`);
  assert.ok(known.includes("href=\"#"), known);
  assert.ok(!known.includes("undefined"), known);

  const unknown = call(`icon("not-a-mapped-icon")`);
  assert.ok(unknown.includes("not-a-mapped-icon"), unknown);
  assert.ok(!unknown.includes("undefined"), unknown);
});

/* ---- update check ------------------------------------------------- */

test("updateStatusText reports an up-to-date build with its own version", () => {
  assert.equal(
    call(`updateStatusText({ ok: true, current: "2.35.0", latest: "2.35.0", update_available: false })`),
    "You are up to date (2.35.0).",
  );
});

test("updateStatusText names the newer release, its asset and its size", () => {
  const text = call(`updateStatusText({
    ok: true, current: "2.35.0", latest: "2.36.0", update_available: true,
    asset: { name: "free-ai-suite_2.36.0_windows-x86_64.zip", size: 2048 },
    platform: "windows/x86_64"
  })`);
  assert.ok(text.includes("Version 2.36.0 is available"), text);
  assert.ok(text.includes("this build is 2.35.0"), text);
  assert.ok(text.includes("Download free-ai-suite_2.36.0_windows-x86_64.zip"), text);
  assert.ok(/\(.*(KB|kB|kb).*\)/.test(text), `size was not formatted: ${text}`);
});

test("updateStatusText says so when no build exists for this platform", () => {
  const text = call(`updateStatusText({
    ok: true, current: "2.35.0", latest: "2.36.0", update_available: true,
    asset: null, platform: "freebsd/x86_64"
  })`);
  assert.ok(text.includes("No freebsd/x86_64 build"), text);
  assert.ok(!text.includes("Download"), text);
});

test("updateStatusText passes a failed check through as a failure", () => {
  assert.equal(
    call(`updateStatusText({ error: "The release API answered 503." })`),
    "Update check failed: The release API answered 503.",
  );
  assert.equal(call("updateStatusText(null)"), "The update check returned no answer.");
});

test("stagedStatusText describes a verified file that is not installed", () => {
  const text = call(`stagedStatusText({
    asset: "free-ai-suite_2.37.0_windows-x86_64.zip", bytes: 2048,
    latest: "2.37.0", sha256: "abcdef0123456789abcdef"
  })`);
  assert.ok(text.includes("free-ai-suite_2.37.0_windows-x86_64.zip"), text);
  assert.ok(text.includes("2.37.0"), text);
  assert.ok(text.includes("abcdef012345"), text);
  assert.ok(text.includes("not installed"), text);
  assert.ok(/KB/.test(text), `size was not formatted: ${text}`);
});

test("stagedStatusText stays empty when nothing is staged", () => {
  assert.equal(call("stagedStatusText(null)"), "");
  assert.equal(call("stagedStatusText({})"), "");
});

test("preparedStatusText names the folder and says the running build is unchanged", () => {
  const text = call(`preparedStatusText({
    latest: "2.38.0", root: "C:/data/updates/prepared/2.38.0", binary: "free-ai-studio.exe"
  })`);
  assert.ok(text.includes("2.38.0"), text);
  assert.ok(text.includes("C:/data/updates/prepared/2.38.0"), text);
  assert.ok(text.includes("free-ai-studio.exe"), text);
  assert.ok(text.includes("unchanged"), text);
  assert.ok(text.includes("Start that copy"), text);
});

test("preparedStatusText stays empty when nothing is prepared", () => {
  assert.equal(call("preparedStatusText(null)"), "");
  assert.equal(call("preparedStatusText({})"), "");
  assert.equal(call("preparedStatusText({ latest: \"\" })"), "");
});

test("appliedStatusText says restart, what it replaced and where the copy is", () => {
  const text = call(`appliedStatusText({
    latest: "2.39.0", previous: "2.38.0", backup: "C:/app/free-ai-studio.old-2.38.0.exe"
  })`);
  assert.ok(text.includes("2.39.0"), text);
  assert.ok(text.includes("restart"), text);
  assert.ok(text.includes("2.38.0"), text);
  assert.ok(text.includes("free-ai-studio.old-2.38.0.exe"), text);
});

test("appliedStatusText stays empty before any swap", () => {
  assert.equal(call("appliedStatusText(null)"), "");
  assert.equal(call("appliedStatusText({})"), "");
});
