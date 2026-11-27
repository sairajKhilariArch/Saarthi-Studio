"use strict";

/* Self-improvement dashboard helpers.
 *
 * The dashboard prints server rows; these tests cover the wording it applies to
 * them — a null rate must read as "not measured", and a verdict is always
 * qualified as one sample.
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

test("improveRate never turns a missing measurement into a number", () => {
  assert.equal(call("improveRate(null)"), "not measured");
  assert.equal(call("improveRate(undefined)"), "not measured");
  assert.equal(call("improveRate(NaN)"), "not measured");
  assert.equal(call("improveRate(0)"), "0%");
  assert.equal(call("improveRate(66.7)"), "66.7%");
});

test("risk and verdict labels are explicit rather than optimistic", () => {
  assert.equal(call("improveRiskLabel('high')"), "High risk");
  assert.equal(call("improveRiskLabel('medium')"), "Medium risk");
  assert.equal(call("improveRiskLabel('low')"), "Low risk");
  assert.equal(call("improveRiskLabel('')"), "Unknown");
  assert.equal(call("improveVerdictLabel('variant')"), "Variant better on this sample");
  assert.equal(call("improveVerdictLabel('tie')"), "No measurable difference");
  assert.equal(call("improveVerdictLabel('inconclusive')"), "Inconclusive");
  assert.equal(call("improveVerdictLabel('neither')"), "Both arms failed");
});

test("confidence is printed as a percentage, never as certainty", () => {
  assert.equal(call("improveConfidence(0.55)"), "55% confidence");
  assert.equal(call("improveConfidence(null)"), "confidence unknown");
});

test("the self-improvement view and its loaders are registered", () => {
  assert.equal(call("VIEWS.includes('improve')"), true);
  assert.equal(call("typeof loadImprove"), "function");
  assert.equal(call("typeof approveProposal"), "function");
  assert.equal(call("typeof rollbackProposal"), "function");
  assert.equal(call("typeof renderImproveSettings"), "function");
  assert.equal(call("typeof renderImproveReport"), "function");
});
