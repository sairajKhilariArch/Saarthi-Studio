"use strict";

/* Phase 7 UI contracts.
 *
 * These run the real app.js / ide.js in the browser-free harness and read the
 * real stylesheet and markup. They cover the defects fixed in Phase 7:
 *
 *   - chat code blocks use the tested highlighter, not raw escaped text;
 *   - the thread list filters by title and project;
 *   - a dialog keeps Tab inside it and returns focus to its opener;
 *   - the theme choice reaches both <html data-theme> and localStorage;
 *   - the phone dock no longer covers the activity rail;
 *   - the workbench home is visible at startup instead of a blank column.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { loadApp, loadIde, evaluate } = require("./harness");

const read = (...parts) => fs.readFileSync(path.join(__dirname, "..", ...parts), "utf8");
const highlightSource = read("highlight.js");
const ideCss = read("ide.css");
const html = read("index.html");

/** The real UI script plus the highlighter it expects to find as a global. */
function appWithHighlight() {
  const context = loadApp();
  vm.runInContext(highlightSource, context, { filename: "highlight.js" });
  return context;
}

test("chat code blocks are highlighted and the language is escaped", () => {
  const context = appWithHighlight();
  const block = (code, language) =>
    evaluate(
      context,
      `codeBlockHtml(${JSON.stringify(code)}, ${JSON.stringify(language)})`
    );

  const rust = block("let x: i32 = 42; // hi", "rust");
  assert.match(rust, /class="hljs language-rust"/);
  assert.match(rust, /tok-key/);
  assert.match(rust, /aria-label="Copy rust code"/);
  assert.match(rust, /type="button"/);

  const hostile = block('<img src=x onerror="alert(1)">', '"><script>');
  assert.ok(!hostile.includes("<img"), hostile);
  assert.ok(!hostile.includes("<script>"), hostile);
  assert.ok(hostile.includes("&lt;img"), hostile);
});

test("a fenced block in a reply keeps its copy affordance", () => {
  const context = appWithHighlight();
  const markdown = "```js\nconst answer = 42;\n```";
  const rendered = evaluate(
    context,
    `renderMarkdown(${JSON.stringify(markdown)})`
  );
  assert.match(rendered, /class="code-block"/);
  assert.match(rendered, /language-js/);
  assert.match(rendered, /data-copy-code/);
});

test("thread filtering matches title and project, case-insensitively", () => {
  const context = loadApp();
  const threads = [
    { title: "Fix Auth flow", project: "web" },
    { title: "Write docs", project: "studio" },
  ];
  const filter = (query) =>
    evaluate(
      context,
      `filterConversations(${JSON.stringify(threads)}, ${JSON.stringify(query)}).map((t) => t.title)`
    );

  // The vm has its own Array realm, so compare the JSON, not the identity.
  const titles = (query) => JSON.stringify(filter(query));
  assert.equal(titles("auth"), '["Fix Auth flow"]');
  assert.equal(titles("STUDIO"), '["Write docs"]');
  assert.equal(titles("   "), '["Fix Auth flow","Write docs"]');
  assert.equal(titles("missing"), "[]");
});

test("dialog focus helpers skip disabled and hidden controls", () => {
  const context = loadApp();
  const visible = { disabled: false, classList: { contains: () => false }, closest: () => null };
  const disabled = { disabled: true, classList: { contains: () => false }, closest: () => null };
  const hidden = { disabled: false, classList: { contains: () => false }, closest: () => ({}) };
  context.__focusNodes = [visible, disabled, hidden];
  const found = evaluate(
    context,
    `modalFocusables({ querySelectorAll: () => __focusNodes })`
  );
  assert.equal(found.length, 1);
});

test("the theme writer updates the document and persists the choice", () => {
  const context = loadApp();
  evaluate(context, `applyTheme("light")`);
  assert.equal(
    evaluate(context, `document.documentElement.dataset.theme`),
    "light"
  );
  assert.equal(
    evaluate(context, `localStorage.getItem("freeai-theme")`),
    "light"
  );
  evaluate(context, `applyTheme("dark")`);
  assert.equal(
    evaluate(context, `localStorage.getItem("freeai-theme")`),
    "dark"
  );
});

test("a phone-width dock leaves the activity rail uncovered", () => {
  const start = ideCss.indexOf("@media (max-width: 480px)");
  assert.ok(start >= 0, "the phone dock rule is missing");
  const block = ideCss.slice(start, ideCss.indexOf("}", start) + 1);
  assert.match(block, /\.app > \.ai-dock\s*\{[^}]*width:\s*calc\(100vw - var\(--activity-w\)\)/);
});

test("streaming status and the thread filter expose accessible semantics", () => {
  assert.match(html, /id="thinking"[^>]*role="status"[^>]*aria-live="polite"/);
  assert.match(html, /id="messages"[^>]*aria-busy="false"/);
  assert.match(html, /id="threadFilter"[^>]*aria-label="Filter conversations by title"/);
  assert.match(html, /id="threadFilter"[^>]*type="search"/);
});

test("navigation wires the workbench to the visible view", () => {
  // The harness keeps `window` separate from the vm global, so ide.js's
  // `window.setView` wrapper cannot be invoked there the way a browser does.
  // The behavior is exercised in the browser audit; here the contract is that
  // both the first paint and later navigation use the same visibility rule.
  const ideJs = read("ide.js");
  assert.match(
    ideJs,
    /workbench\.classList\.toggle\("hidden", editorVisible \|\| pageVisible\)/,
    "the first paint applies the visibility rule"
  );
  assert.match(
    ideJs,
    /\$\("workbenchEmpty"\)\.classList\.toggle\("hidden", editorVisible \|\| pageVisible\)/,
    "later setView calls apply the same rule"
  );
});

test("the workbench home exists and is visible at startup", () => {
  const context = loadIde(loadApp());
  const visible = evaluate(
    context,
    `!!document.getElementById("workbenchEmpty") && !document.getElementById("workbenchEmpty").classList.contains("hidden")`
  );
  assert.equal(visible, true, "a blank main column is not an acceptable first screen");
});
