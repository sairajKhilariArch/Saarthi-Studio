"use strict";

/* The plugin page: the helpers that turn the server's catalog into a row, and
 * the markup/endpoints the page depends on. The catalog itself, the marketplace
 * and the install path are Rust's and are tested there.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { loadApp, evaluate } = require("./harness");

const read = (name) => fs.readFileSync(path.join(__dirname, "..", name), "utf8");
const ui = loadApp();

test("plugin rows read the fields the server sends", () => {
  assert.equal(evaluate(ui, "pluginPermissions({ permissions: [] })"), "no permissions declared");
  assert.equal(
    evaluate(ui, "pluginPermissions({ permissions: ['workspace.read', 'network'] })"),
    "workspace.read, network",
  );
  assert.equal(evaluate(ui, "pluginError({ blockedReason: 'Needs a newer Studio.' })"), "Needs a newer Studio.");
  assert.equal(evaluate(ui, "pluginError({ error: 'manifest.json is not valid JSON' })"), "manifest.json is not valid JSON");
  assert.equal(evaluate(ui, "pluginError({})"), "");
  assert.equal(
    evaluate(ui, "pluginFacts({ id: 'hello-tools', version: '0.1.0', permissions: ['network'], tools: [{ name: 'a' }], kind: 'extension' })"),
    "hello-tools · v0.1.0 · 1 tool · network",
  );
  assert.equal(
    evaluate(ui, "pluginFacts({ id: 'code-review', kind: 'skill', source: 'user', permissions: [] })"),
    "code-review · user · no permissions declared",
  );
});

test("the page exists, is a view, and has no invented listings", () => {
  const html = read("index.html");
  assert.match(html, /id="pluginsView"/);
  assert.match(html, /id="pluginsInstalled"/);
  assert.match(html, /id="pluginMarketplace"/);
  assert.match(html, /No marketplace is bundled/);
  const app = read("app.js");
  assert.match(app, /const VIEWS = \[[\s\S]*?"plugins"[\s\S]*?\];/);
  assert.match(app, /if \(view === "plugins"\) void loadPlugins\(\);/);
  assert.match(app, /\$\{plugin\.kind\}\/\$\{encodeURIComponent\(plugin\.id\)\}\/state/);
  assert.match(app, /\/api\/plugins\/marketplace\/install/);
  const ide = read("ide.js");
  assert.match(ide, /\{ id: "plugins", icon: "cube", label: "Plugins", view: "plugins" \}/);
});
