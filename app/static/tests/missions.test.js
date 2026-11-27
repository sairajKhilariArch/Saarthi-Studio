"use strict";

/* Mission control helpers.
 *
 * The dashboard shows whatever the mission API returns; these tests cover the
 * two places where the UI interprets that data instead of printing it — the
 * status label and the progress derived from the server's own counts.
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, evaluate } = require("./harness");

const ui = loadApp();
const call = (expression) => evaluate(ui, expression);

test("missionStatusLabel names every state the server can send", () => {
  const cases = {
    planned: "Queued",
    running: "Running",
    paused: "Paused",
    "awaiting-approval": "Waiting for approval",
    succeeded: "Succeeded",
    failed: "Failed",
    cancelled: "Cancelled",
    done: "Done",
    blocked: "Blocked",
  };
  Object.entries(cases).forEach(([status, label]) => {
    assert.equal(call(`missionStatusLabel(${JSON.stringify(status)})`), label);
  });
  assert.equal(call("missionStatusLabel('')"), "Unknown");
  assert.equal(call("missionStatusLabel(undefined)"), "Unknown");
});

test("missionProgress counts settled tasks, not started ones", () => {
  // The object comes from the sandbox realm, so compare its fields rather than
  // its prototype: deepEqual would fail on two identical plain objects.
  const progress = call(`missionProgress({
    counts: { total: 5, done: 2, failed: 1, blocked: 0, running: 1, planned: 1 },
  })`);
  assert.equal(progress.settled, 3);
  assert.equal(progress.total, 5);
  assert.equal(progress.percent, 60);
});

test("missionProgress never divides by zero and tolerates missing counts", () => {
  ["missionProgress({})", "missionProgress(null)", "missionProgress({ counts: { total: 0 } })"].forEach(
    (expression) => {
      const progress = call(expression);
      assert.equal(progress.settled, 0, expression);
      assert.equal(progress.total, 0, expression);
      assert.equal(progress.percent, 0, expression);
    },
  );
});

test("missionIsActive is true only for states the supervisor is still driving", () => {
  ["planned", "running", "paused", "awaiting-approval"].forEach((status) => {
    assert.equal(call(`missionIsActive({ status: ${JSON.stringify(status)} })`), true);
  });
  ["succeeded", "failed", "cancelled"].forEach((status) => {
    assert.equal(call(`missionIsActive({ status: ${JSON.stringify(status)} })`), false);
  });
  assert.equal(call("missionIsActive(null)"), false);
});

test("the missions view is registered so setView can show it", () => {
  assert.equal(call("VIEWS.includes('missions')"), true);
  assert.equal(call("typeof loadMissions"), "function");
});
