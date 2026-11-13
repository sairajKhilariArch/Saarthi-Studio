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

test("a stored position wins over the dependency layout", () => {
  assert.equal(
    call(`(() => {
      const p = graphLayout([
        { id: "a", kind: "task", x: 300, y: 120 },
        { id: "b", kind: "task", dependsOn: ["a"] }
      ]);
      return p.get("a").x === 300 && p.get("a").y === 120 && p.get("b").x === 264;
    })()`),
    true,
  );
  // Half a position is not a position, and neither is a non-numeric one: both
  // nodes lay out by depth instead of landing on an axis they were never dragged along.
  assert.equal(call(`graphLayout([{ id: "a", kind: "task", x: 300 }]).get("a").x`), 24);
  assert.equal(call(`graphLayout([{ id: "a", kind: "task", x: "left", y: 5 }]).get("a").x`), 24);
  assert.equal(call(`graphStoredPosition({ id: "a", x: -8, y: 12.6 }).x`), 0);
});

test("dragging clamps to the canvas and commits whole pixels", () => {
  // Compared as text: an object built inside the sandbox has a different
  // Object.prototype, which assert.deepEqual (strict) treats as a mismatch.
  const drag = (call_) => call(`JSON.stringify(graphDragPosition(${call_}))`);
  assert.equal(drag(`{ x: 40, y: 60 }, 12.4, -100, { width: 500, height: 300 }`), '{"x":52,"y":0}');
  assert.equal(drag(`{ x: 40, y: 60 }, 1000, 1000, { width: 500, height: 300 }`), '{"x":500,"y":300}');
  // A zero or missing bound cannot push a card off the top-left corner.
  assert.equal(drag(`{ x: -5, y: 0 }, -50, 0, { width: 0, height: 0 }`), '{"x":0,"y":0}');
  assert.equal(drag(`{ x: 10, y: 10 }, 5, 5, null`), '{"x":0,"y":0}');
});

test("a dragged node's position is written into the graph", () => {
  call(`workflowGraph = [{ id: "a", kind: "task", note: "keep me" }]`);
  call(`updateWorkflowNode("a", { x: 250, y: 90 })`);
  const node = JSON.parse(call("JSON.stringify(workflowGraph[0])"));
  assert.equal(node.x, 250);
  assert.equal(node.y, 90);
  // Dragging changes where the card is, never what the node does.
  assert.equal(node.note, "keep me");
});

test("a refresh restores the remembered thread, and only while it still exists", () => {
  assert.equal(call(`threadToRestore([{ id: "a" }, { id: "b" }], "b")`), "b");
  // A thread deleted elsewhere (or from another project) is not restored.
  assert.equal(call(`threadToRestore([{ id: "a" }], "gone")`), null);
  assert.equal(call(`threadToRestore([{ id: "a" }], "")`), null);
  assert.equal(call(`threadToRestore(null, "a")`), null);
});

test("the open thread is remembered and forgotten", () => {
  call(`rememberThread("abc")`);
  assert.equal(call(`rememberedThreadId()`), "abc");
  call(`rememberThread(null)`);
  assert.equal(call(`rememberedThreadId()`), "");
  // A remembered id the server no longer lists resolves to nothing, so the page
  // falls back to the newest thread instead of opening a dead one.
  call(`rememberThread("stale")`);
  assert.equal(call(`threadToRestore([{ id: "x" }])`), null);
  call(`rememberThread(null)`);
});

test("the status bar label mirrors the connection text", () => {
  assert.equal(call(`connectionStatusLabel("Router connected")`), "Router connected");
  assert.equal(call(`connectionStatusLabel("Checking…")`), "Checking");
  // An empty label falls back rather than leaving a blank status item.
  assert.equal(call(`connectionStatusLabel("")`), "router");
  assert.equal(call(`connectionStatusLabel(null)`), "router");
});

test("a first run in the IDE shell opens the docked conversation", () => {
  // The P0: the chat lives in the dock column, so the legacy default (closed)
  // left it at 0px wide — a blank workspace with an 8px composer.
  assert.equal(call(`defaultPanelShow({}, true).right`), true);
  assert.equal(call(`defaultPanelShow({}, false).right`), false);
  // An explicit choice wins over both defaults.
  assert.equal(call(`defaultPanelShow({ right: false }, true).right`), false);
  assert.equal(call(`defaultPanelShow({ right: true }, false).right`), true);
  // The other panels keep their own defaults.
  assert.equal(call(`defaultPanelShow(null, true).left`), true);
  assert.equal(call(`defaultPanelShow(null, true).bottom`), false);
  assert.equal(call(`defaultPanelShow({ left: false, bottom: true }, true).left`), false);
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
    asset: { name: "sarthi-studio_2.36.0_windows-x86_64.zip", size: 2048 },
    platform: "windows/x86_64"
  })`);
  assert.ok(text.includes("Version 2.36.0 is available"), text);
  assert.ok(text.includes("this build is 2.35.0"), text);
  assert.ok(text.includes("Download sarthi-studio_2.36.0_windows-x86_64.zip"), text);
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
    asset: "sarthi-studio_2.37.0_windows-x86_64.zip", bytes: 2048,
    latest: "2.37.0", sha256: "abcdef0123456789abcdef"
  })`);
  assert.ok(text.includes("sarthi-studio_2.37.0_windows-x86_64.zip"), text);
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

test("rollbackStatusText names the restored build, the one it replaced and where that is", () => {
  const text = call(`rollbackStatusText({
    restored: "2.40.0", replaced: "2.41.0", superseded: "C:/app/free-ai-studio.old-2.41.0.exe"
  })`);
  assert.ok(text.includes("2.40.0"), text);
  assert.ok(text.includes("2.41.0"), text);
  assert.ok(text.includes("restart"), text);
  assert.ok(text.includes("free-ai-studio.old-2.41.0.exe"), text);
});

test("uninstallSummary names every path and count before anything is offered", () => {
  const text = call(`uninstallSummary({
    confirmation: "C:/Users/me/AppData/Roaming/free-ai-studio",
    targets: {
      install: { path: "C:/apps/studio", exists: true, files: 120, bytes: 4096, removable: true, reason: "" },
      data: { path: "C:/Users/me/AppData/Roaming/free-ai-studio", exists: true, files: 9, bytes: 2048 },
      config: { path: "C:/Users/me/AppData/Roaming/free-ai-studio", exists: false, files: 0, bytes: 0 }
    }
  })`);
  assert.ok(text.includes("program folder: C:/apps/studio"), text);
  assert.ok(text.includes("120 files"), text);
  assert.ok(text.includes("data: C:/Users/me/AppData/Roaming/free-ai-studio"), text);
  assert.ok(text.includes("settings:"), text);
  assert.ok(text.includes("nothing there"), text);
});

test("uninstallSummary says why the program folder cannot be removed", () => {
  const text = call(`uninstallSummary({
    targets: { install: { path: "C:/repo", exists: true, files: 3, bytes: 10, removable: false, reason: "it is a source checkout" } }
  })`);
  assert.ok(text.includes("cannot be removed"), text);
  assert.ok(text.includes("it is a source checkout"), text);
  assert.equal(call("uninstallSummary(null)"), "");
});

test("uninstallReportText distinguishes removed, kept and scheduled", () => {
  const removed = call(`uninstallReportText({ targets: {
    install: { path: "C:/apps/studio", removed: true },
    data: { path: "C:/data", removed: true },
    config: { path: "C:/config", kept: true }
  } })`);
  assert.ok(removed.includes("Program folder removed."), removed);
  assert.ok(removed.includes("Data removed."), removed);
  assert.ok(removed.includes("Settings kept"), removed);

  // Windows cannot delete a running program, so the folder is named as pending
  // rather than claimed removed.
  const pending = call(`uninstallReportText({ targets: {
    install: { path: "C:/apps/studio", removed: false, pending: "C:/apps/studio" },
    data: { path: "C:/data", removed: false, kept: false, error: "busy" }
  } })`);
  assert.ok(pending.includes("stays until you delete it"), pending);
  assert.ok(pending.includes("C:/apps/studio"), pending);
  assert.ok(pending.includes("busy"), pending);

  assert.equal(call("uninstallReportText(null)"), "Nothing to remove.");
});

test("rollbackStatusText stays empty before any rollback", () => {
  assert.equal(call("rollbackStatusText(null)"), "");
  assert.equal(call("rollbackStatusText({})"), "");
  assert.equal(call('rollbackStatusText({ restored: "" })'), "");
});

test("appliedStatusText stays empty before any swap", () => {
  assert.equal(call("appliedStatusText(null)"), "");
  assert.equal(call("appliedStatusText({})"), "");
});

test("the file tree refreshes only when the fingerprint actually moves", () => {
  // First poll after opening a folder only seeds the hash: no needless render.
  assert.equal(call("treeNeedsRefresh(null, 'abc')"), false);
  assert.equal(call("treeNeedsRefresh(undefined, 'abc')"), false);
  // A poll that was answered with nothing must not wipe the seeded hash.
  assert.equal(call("treeNeedsRefresh('abc', null)"), false);
  // Unchanged workspace: nothing happens.
  assert.equal(call("treeNeedsRefresh('abc', 'abc')"), false);
  // A real edit: refresh.
  assert.equal(call("treeNeedsRefresh('abc', 'def')"), true);
});

test("capabilities hide only what a contract this UI reads reports false", () => {
  assert.equal(call("capabilitiesAllow(null, 'mcp')"), true);
  assert.equal(call("capabilitiesAllow({}, 'mcp')"), true);
  // A future contract is not guessed at: every control stays where it was.
  assert.equal(call("capabilitiesAllow({ contract: 2, features: { mcp: false } }, 'mcp')"), true);
  assert.equal(call("capabilitiesAllow({ contract: 1 }, 'mcp')"), true);
  assert.equal(call("capabilitiesAllow({ contract: 1, features: {} }, 'mcp')"), true);
  assert.equal(call("capabilitiesAllow({ contract: 1, features: { mcp: false } }, 'mcp')"), false);
  assert.equal(call("capabilitiesAllow({ contract: 1, features: { mcp: true } }, 'mcp')"), true);
});

test("every control the contract can hide carries its feature id", () => {
  const fs = require("node:fs");
  const path = require("node:path");
  const html = fs.readFileSync(path.join(__dirname, "..", "index.html"), "utf8");
  const pairs = [
    ['data-settings-section="search"', "deep_search"],
    ['data-settings-section="openai"', "openai_platform"],
    ['data-settings-section="permissions"', "permissions"],
    ['data-settings-section="mcp"', "mcp"],
    ['data-settings-section="diagnostics"', "diagnostics"],
    ['data-settings-section="extensions"', "extensions"],
    ['id="settings-search"', "deep_search"],
    ['id="settings-openai"', "openai_platform"],
    ['id="settings-permissions"', "permissions"],
    ['id="settings-mcp"', "mcp"],
    ['id="settings-diagnostics"', "diagnostics"],
    ['id="settings-extensions"', "extensions"],
    ['id="indexPane"', "code_index"],
    ['id="modeIndex"', "code_index"],
    ['id="memoryPane"', "memory"],
    ['id="modeMemory"', "memory"],
    ['id="formatBtn"', "editor_format"],
    ['id="completions"', "editor_complete"],
  ];
  for (const [needle, feature] of pairs) {
    const start = html.indexOf(needle);
    assert.ok(start >= 0, `${needle} exists`);
    const end = html.indexOf(">", start);
    assert.ok(
      html.slice(start, end).includes(`data-feature="${feature}"`),
      `${needle} is gated on ${feature}`,
    );
  }
  assert.ok(html.includes('data-feature="saved_commands"'), "the saved-commands field is gated");
  const ide = fs.readFileSync(path.join(__dirname, "..", "ide.js"), "utf8");
  assert.match(ide, /id: "scm"[^}]*feature: "git"/, "the Source control activity is gated on git");
  assert.match(ide, /id="watchPreset"[^>]*data-feature="watch"/, "the Watch button is gated on watch");
});
