"use strict";

/* Regression: the capabilities answer must never unhide a sidebar pane.
 *
 * applyCapabilities used to toggle `hidden` off for every allowed feature,
 * which re-showed the memory and index panes after setSidebarMode had hidden
 * them — three panes stacked in the sidebar, overflowing into the footer.
 * Capabilities are a hiding-only signal; pane visibility belongs to the
 * sidebar mode. These tests run the real function from app.js.
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

/* Builds fake [data-feature] elements, runs applyCapabilities against them,
 * and reports which ones carry `hidden` afterwards. */
const runCapabilities = (features, capabilities) =>
  call(`(() => {
    const make = (feature, hidden) => {
      const classes = new Set(hidden ? ["hidden"] : []);
      return {
        dataset: { feature },
        classList: {
          add: (c) => classes.add(c),
          remove: (c) => classes.delete(c),
          toggle: (c, force) => { force ? classes.add(c) : classes.delete(c); },
          contains: (c) => classes.has(c),
        },
        isHidden: () => classes.has("hidden"),
      };
    };
    const panes = ${JSON.stringify(features)}.map(([feature, hidden]) => make(feature, hidden));
    const originalAll = document.querySelectorAll;
    const originalOne = document.querySelector;
    const originalDispatch = window.dispatchEvent;
    if (typeof CustomEvent === "undefined") {
      globalThis.CustomEvent = class { constructor(type) { this.type = type; } };
    }
    document.querySelectorAll = (sel) => (sel === "[data-feature]" ? panes : originalAll(sel));
    document.querySelector = () => null;
    window.dispatchEvent = () => {};
    state.capabilities = ${JSON.stringify(capabilities)};
    try { applyCapabilities(); } finally {
      document.querySelectorAll = originalAll;
      document.querySelector = originalOne;
      window.dispatchEvent = originalDispatch;
    }
    return JSON.stringify(panes.map((p) => [p.dataset.feature, p.isHidden()]));
  })()`);

test("an allowed feature does not unhide a pane the sidebar mode hid", () => {
  const result = JSON.parse(
    runCapabilities(
      [["memory", true], ["code_index", true], ["skills", false]],
      { contract: 1, features: { memory: true, code_index: true, skills: true } },
    ),
  );
  assert.deepEqual(result, [
    ["memory", true], // still hidden: the files mode owns visibility
    ["code_index", true], // still hidden
    ["skills", false], // visible control stays visible
  ]);
});

test("a disabled feature is hidden even when it was visible", () => {
  const result = JSON.parse(
    runCapabilities(
      [["memory", false], ["code_index", false]],
      { contract: 1, features: { memory: false, code_index: false } },
    ),
  );
  assert.deepEqual(result, [
    ["memory", true],
    ["code_index", true],
  ]);
});

test("an unknown contract leaves every control exactly as it was", () => {
  const result = JSON.parse(
    runCapabilities(
      [["memory", true], ["code_index", false]],
      { contract: 99, features: { memory: false } },
    ),
  );
  assert.deepEqual(result, [
    ["memory", true],
    ["code_index", false],
  ]);
});
