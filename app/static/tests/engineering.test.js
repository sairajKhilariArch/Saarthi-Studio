"use strict";

/* Engineering dashboard helpers.
 *
 * The page prints what the API returns; these tests cover the places where it
 * interprets that data — status words, the autonomy label, the trace wording
 * and progress derived from the mission's own counts.
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

test("engineeringStatusLabel names every pipeline state", () => {
  const cases = {
    "needs-input": "Needs input",
    planned: "Queued",
    running: "Running",
    paused: "Paused",
    "awaiting-approval": "Waiting for approval",
    succeeded: "Succeeded",
    failed: "Failed",
    cancelled: "Cancelled",
    delivered: "Delivered",
    unknown: "Mission missing",
  };
  Object.entries(cases).forEach(([status, label]) => {
    assert.equal(call(`engineeringStatusLabel(${JSON.stringify(status)})`), label);
  });
  assert.equal(call("engineeringStatusLabel('')"), "Unknown");
});

test("autonomyLabel covers the three levels and refuses everything else", () => {
  assert.equal(call("autonomyLabel(1)"), "Assisted");
  assert.equal(call("autonomyLabel(2)"), "Supervised");
  assert.equal(call("autonomyLabel(3)"), "Autonomous");
  assert.equal(call("autonomyLabel(0)"), "Unknown");
  assert.equal(call("autonomyLabel(null)"), "Unknown");
});

test("trace wording separates evidence from a finished run", () => {
  assert.equal(call("traceStatusLabel('verified')"), "Verified");
  assert.equal(call("traceStatusLabel('no-artifact')"), "Observed, no artifact");
  assert.equal(call("traceStatusLabel('in-progress')"), "In progress");
  assert.equal(call("traceStatusLabel('not-verified')"), "Not verified");
  assert.equal(call("traceStatusLabel('')"), "Unknown");
});

test("engineeringProgress counts settled stages only", () => {
  const progress = call(`engineeringProgress({
    counts: { total: 6, done: 3, failed: 1, blocked: 0, running: 1, planned: 1 },
  })`);
  assert.equal(progress.settled, 4);
  assert.equal(progress.total, 6);
  assert.equal(progress.percent, 67);
  ["engineeringProgress({})", "engineeringProgress(null)"].forEach((expression) => {
    const empty = call(expression);
    assert.equal(empty.settled, 0, expression);
    assert.equal(empty.percent, 0, expression);
  });
});

test("a pipeline counts as active only while the supervisor is driving it", () => {
  ["planned", "running", "paused", "awaiting-approval"].forEach((status) => {
    assert.equal(call(`engineeringIsActive({ status: ${JSON.stringify(status)} })`), true);
  });
  ["succeeded", "failed", "cancelled", "needs-input", "delivered"].forEach((status) => {
    assert.equal(call(`engineeringIsActive({ status: ${JSON.stringify(status)} })`), false);
  });
  assert.equal(call("engineeringIsActive(null)"), false);
});

test("the engineering view and its loaders are registered", () => {
  assert.equal(call("VIEWS.includes('engineering')"), true);
  assert.equal(call("typeof loadEngineering"), "function");
  assert.equal(call("typeof openEngineeringPipeline"), "function");
  assert.equal(call("typeof engineeringEmergencyStop"), "function");
});
