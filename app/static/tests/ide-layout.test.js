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
