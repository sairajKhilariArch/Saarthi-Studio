"use strict";

/* The settings form's `name :: command` round trip.
 *
 * Kept in its own file so the assertion is about the form's contract with the
 * server (label and command, both required) rather than about the DOM.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

test("saved commands round-trip through the settings form", () => {
  const text = call(
    `formatCommandPresets([
      { label: "Lint", command: "npm run lint" },
      { label: "Build", command: "cargo build --release" }
    ])`,
  );
  assert.equal(text, "Lint :: npm run lint\nBuild :: cargo build --release");

  const back = call(`parseCommandPresets("Lint :: npm run lint\\n\\n  Build :: cargo build --release  ")`);
  assert.equal(
    JSON.stringify(back),
    JSON.stringify([
      { label: "Lint", command: "npm run lint" },
      { label: "Build", command: "cargo build --release" },
    ]),
  );
});

test("a malformed saved command is dropped, not guessed at", () => {
  // No separator, an empty half, or an empty line: nothing runnable comes out.
  const parsed = call(
    `parseCommandPresets("just a command\\nLeft ::\\n:: right\\n\\nA :: b :: c")`,
  );
  assert.equal(JSON.stringify(parsed), JSON.stringify([{ label: "A", command: "b :: c" }]));
  assert.equal(JSON.stringify(call("parseCommandPresets(null)")), "[]");
  assert.equal(call("formatCommandPresets(null)"), "");
});
