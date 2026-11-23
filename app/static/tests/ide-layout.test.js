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

test("a centered thread fills the viewport and centers its content", () => {
  assert.match(css, /body\.no-activity\s*\{\s*--activity-w:\s*0px;?\s*\}/);
  assert.match(css, /body\.no-status\s*\{\s*--status-h:\s*0px;?\s*\}/);
  assert.match(css, /body\.chat-center \.main,\s*body\.chat-center \.activity-bar,\s*body\.chat-center \.sidebar\s*\{\s*display:\s*none;?\s*\}/);
  assert.match(css, /body\.chat-center \.app\s*\{\s*grid-template-columns:\s*minmax\(0,\s*1fr\);?\s*\}/);
  assert.match(css, /body\.chat-center \.app > \.ai-dock\s*\{[^}]*grid-column:\s*1\s*\/\s*-1/);
  assert.match(css, /body\.chat-center \.ai-dock \.chat-column\s*\{[^}]*max-width:\s*840px/);
  assert.match(css, /body\.chat-center \.ai-dock \.composer-shell\s*\{[^}]*padding:\s*8px 28px 18px/);

  const block = mediaBlock("@media (max-width: 1100px)");
  assert.match(
    block,
    /body\.chat-center \.app > \.ai-dock\s*\{[^}]*position:\s*static/,
    "the narrow layout keeps a centered thread in the grid",
  );
});
