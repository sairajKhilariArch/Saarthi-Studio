"use strict";

/* The conversation lives in the AI dock. A narrow-window rule that hides the
 * dock unconditionally strands the chat: threads cannot be opened and the
 * composer cannot be reached, which is exactly the regression this guards.
 * The dock may only disappear through the user's own right-panel toggle
 * (body.no-right), and the narrow layout must keep it reachable as an
 * overlay. Asserted against the real stylesheet the browser loads — the
 * harness has no layout engine, so the contract is the CSS itself. */

const { test } = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const css = fs.readFileSync(path.join(__dirname, "..", "ide.css"), "utf8");
const baseCss = fs.readFileSync(path.join(__dirname, "..", "styles.css"), "utf8");

/** The body of one @media block, found by brace depth so nested rules and
 * comments inside it cannot truncate the read. */
function mediaBlock(query) {
  const start = css.indexOf(query);
  assert.ok(start >= 0, `the stylesheet is missing ${query}`);
  const open = css.indexOf("{", start);
  let depth = 1;
  let i = open + 1;
  while (i < css.length && depth > 0) {
    if (css[i] === "{") depth += 1;
    else if (css[i] === "}") depth -= 1;
    i += 1;
  }
  return css.slice(open + 1, i - 1);
}

test("a narrow window keeps the chat dock reachable", () => {
  const block = mediaBlock("@media (max-width: 1100px)").replace(/\/\*[\s\S]*?\*\//g, "");
  const rules = [...block.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(([, selector, body]) => [
    selector.trim(),
    body,
  ]);
  const dockRules = rules.filter(([selector]) => /\.ai-dock\b/.test(selector));
  assert.ok(dockRules.length > 0, "the narrow layout says nothing about the dock");

  const hides = dockRules.filter(([, body]) => /display:\s*none/.test(body));
  assert.ok(hides.length > 0, "the narrow layout keeps no way to dismiss the dock");
  for (const [selector] of hides) {
    assert.match(
      selector,
      /body\.no-right/,
      `the dock is hidden without the user's panel toggle: ${selector}`
    );
  }

  const shows = dockRules.some(
    ([selector, body]) =>
      !/body\.no-right/.test(selector) && /display:\s*flex/.test(body)
  );
  assert.ok(shows, "the narrow layout must show the dock as an overlay, not hide it");
});

test("the layout menu exposes every chrome region", () => {
  const html = fs.readFileSync(path.join(__dirname, "..", "index.html"), "utf8");
  assert.match(html, /id="layoutBtn"/, "there is a layout control");
  for (const key of ["activity", "left", "right", "bottom", "status", "center"]) {
    assert.match(html, new RegExp(`data-layout="${key}"`), `the menu can toggle ${key}`);
  }
  assert.match(html, /id="layoutReset"/, "the layout can be reset");
});

test("a centered thread centers its content without hiding navigation", () => {
  assert.match(css, /body\.no-activity\s*\{\s*--activity-w:\s*0px;?\s*\}/);
  assert.match(css, /body\.no-status\s*\{\s*--status-h:\s*0px;?\s*\}/);
  assert.match(css, /body\.chat-center \.main,\s*body\.chat-center \.sidebar\s*\{\s*display:\s*none;?\s*\}/);
  assert.match(css, /body\.chat-center \.activity-bar\s*\{\s*display:\s*flex;?\s*\}/);
  assert.match(css, /body\.chat-center\.no-activity \.activity-bar\s*\{\s*display:\s*none;?\s*\}/);
  assert.match(css, /body\.chat-center \.app\s*\{\s*grid-template-columns:\s*var\(--activity-w\)\s+minmax\(0,\s*1fr\);?\s*\}/);
  assert.match(css, /body\.chat-center \.app > \.ai-dock\s*\{[^}]*grid-column:\s*2/);
  assert.match(css, /body\.chat-center \.app > \.ai-dock\s*\{[^}]*padding-right:\s*var\(--activity-w\)/);
  assert.match(css, /body\.chat-center \.ai-dock \.chat-column\s*\{[^}]*max-width:\s*840px/);
  assert.match(css, /body\.chat-center \.ai-dock \.composer-shell\s*\{[^}]*padding:\s*8px 28px 18px/);

  const block = mediaBlock("@media (max-width: 1100px)");
  assert.match(
    block,
    /body\.chat-center \.app > \.ai-dock\s*\{[^}]*position:\s*static/,
    "the narrow layout keeps a centered thread in the grid",
  );
});

test("a narrow window gives pages the width the drawer used to reserve", () => {
  const block = mediaBlock("@media (max-width: 980px)");
  assert.match(block, /\.app\s*\{\s*grid-template-columns:\s*var\(--activity-w\)\s+minmax\(0,\s*1fr\);?\s*\}/);
  assert.match(block, /grid-template-rows:\s*minmax\(0,\s*1fr\)\s+0\s+var\(--status-h\)/);
  assert.match(block, /\.app > \.main\s*\{\s*grid-column:\s*2;?\s*\}/);

  // styles.css owns the fixed drawer; its selector must beat ide.css's later
  // `.sidebar { position: relative }` or the drawer covers the activity rail.
  const start = baseCss.indexOf("@media (max-width: 980px)");
  assert.ok(start >= 0);
  const narrow = baseCss.slice(start, start + 1400);
  assert.match(narrow, /\.app > \.sidebar\s*\{[^}]*position:\s*fixed/);
  assert.match(narrow, /body:not\(\.nav-open\) \.app > \.sidebar\s*\{\s*pointer-events:\s*none/);
});

test("activity navigation exits the centered and overlay layouts", () => {
  const ide = fs.readFileSync(path.join(__dirname, "..", "ide.js"), "utf8");
  assert.match(ide, /\(activity\.pane \|\| activity\.view\) && typeof setChatCenter === "function"/);
  assert.match(ide, /matchMedia\("\(max-width: 1100px\)"\)/);
  assert.match(ide, /setPanel\("right", false\)/);
  assert.match(ide, /document\.body\.classList\.add\("nav-open"\)/);
});
