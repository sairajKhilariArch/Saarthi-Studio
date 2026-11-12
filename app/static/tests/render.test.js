"use strict";

/* Render smoke for the two views that build DOM the unit tests never touched.
 *
 * `app.test.js` asserts pure helpers; this file runs the real render paths in
 * the same browser-free harness and inspects the elements they produced, so a
 * canvas or IDE shell that throws on load, renders nothing, or loses its
 * children fails here. It is not a layout test: no rect is measured and no
 * pixels are compared, because only a browser can answer those.
 *
 * Run with: bash scripts/ci/test-frontend.sh
 */

const test = require("node:test");
const assert = require("node:assert/strict");
const { loadApp, loadIde, evaluate } = require("./harness");

/** Every element in the stub tree, depth first. */
function walk(node, out = []) {
  if (!node || typeof node !== "object") return out;
  out.push(node);
  for (const child of node.children || []) walk(child, out);
  return out;
}

const hasClass = (node, name) =>
  String(node.className || "")
    .split(/\s+/)
    .includes(name);

test("the workflow canvas renders a card per node and an edge per dependency", () => {
  const ui = loadApp();
  const call = (expression) => evaluate(ui, expression);

  // Workflow nodes carry `title`; `name` is not part of the node schema, so a
  // test that used it would have asserted a field the app never reads.
  call(`workflowGraph = [
    { id: "plan", kind: "task", title: "Plan the change" },
    { id: "build", kind: "agent", title: "Build it", dependsOn: ["plan"] },
    { id: "check", kind: "approval", dependsOn: ["build"] }
  ]`);
  call("renderWorkflowCanvas()");

  const layer = ui.document.getElementById("workflowNodesLayer");
  assert.equal(layer.children.length, 3, "one card per node");
  // A card builds its body with innerHTML and edits its name in an <input>'s
  // value, so all three paths have to be searched.
  const cards = walk(layer);
  const rendered = cards
    .map((node) => `${node.textContent || ""} ${node.innerHTML || ""} ${node.value || ""}`)
    .join(" ");
  assert.ok(rendered.includes("Plan the change"), "a node's title reaches the canvas");
  assert.ok(rendered.includes("Build it"), "and so does the next node's");
  // A node that was never titled still shows something usable: its id.
  assert.ok(rendered.includes("check"), "an untitled node falls back to its id");

  const svg = ui.document.getElementById("workflowEdges");
  // The arrowhead defs plus one path per dependency edge.
  assert.equal(svg.children.length, 3, "one edge per dependency, plus defs");

  const canvas = ui.document.getElementById("workflowCanvas");
  assert.match(String(canvas.style.height), /^\d+px$/, "the canvas gets a height");

  // Two nodes at the same depth sit side by side rather than overlapping.
  const positions = call("JSON.stringify(Array.from(graphLayout(workflowGraph)))");
  const byId = new Map(JSON.parse(positions));
  assert.ok(byId.get("plan").x < byId.get("build").x, "depth runs left to right");
});

test("dragging a card moves it live, follows its edge and commits the position", () => {
  const ui = loadApp();
  const call = (expression) => evaluate(ui, expression);
  call(`workflowGraph = [
    { id: "plan", kind: "task", title: "Plan the change" },
    { id: "build", kind: "agent", title: "Build it", dependsOn: ["plan"] }
  ]`);
  call("renderWorkflowCanvas()");

  const layer = ui.document.getElementById("workflowNodesLayer");
  const card = layer.children[0];
  const svg = ui.document.getElementById("workflowEdges");
  const edges = () =>
    svg.children
      .map((node) => node.getAttribute("d"))
      .filter(Boolean)
      .join(" | ");
  const before = edges();
  assert.match(before, /^M \d+/, "the edge is drawn before the drag");

  const startX = Number(String(card.style.left).replace("px", ""));
  const startY = Number(String(card.style.top).replace("px", ""));
  assert.equal(card.dispatch("pointerdown", { clientX: 40, clientY: 40 }), 1, "one drag handler");
  card.dispatch("pointermove", { clientX: 140, clientY: 90 });

  // While the pointer is down the card itself follows it...
  assert.equal(
    String(card.style.left),
    `${startX + 100}px`,
    "the card follows the pointer without waiting for the release",
  );
  assert.equal(String(card.style.top), `${startY + 50}px`);
  assert.ok(card.classList.contains("dragging"), "the card is marked while dragged");
  // ...and so does the edge that starts at it.
  assert.notEqual(edges(), before, "the edge is redrawn as the card moves");

  card.dispatch("pointerup", {});
  assert.equal(card.classList.contains("dragging"), false, "the mark is cleared on release");
  const committed = JSON.parse(call("JSON.stringify(workflowGraph[0])"));
  assert.equal(committed.x, startX + 100, "the release writes the position into the graph");
  assert.equal(committed.y, startY + 50, "so saving the workflow returns the card here");

  // The canvas width is the limit: a drag past the right edge stops at it.
  const moved = layer.children[0];
  moved.dispatch("pointerdown", { clientX: 0, clientY: 0 });
  moved.dispatch("pointermove", { clientX: 99999, clientY: 99999 });
  moved.dispatch("pointerup", {});
  const clamped = JSON.parse(call("JSON.stringify(workflowGraph[0])"));
  assert.equal(clamped.x, 960 - 190 - 24, "the card stops inside the canvas");
  assert.ok(clamped.y > 0, "and the height is free so a downward drag is kept");
});

test("a press without movement selects the card and writes no position", () => {
  const ui = loadApp();
  const call = (expression) => evaluate(ui, expression);
  call(`workflowGraph = [{ id: "plan", kind: "task", title: "Plan" }]`);
  call("renderWorkflowCanvas()");

  const card = ui.document.getElementById("workflowNodesLayer").children[0];
  card.dispatch("pointerdown", { clientX: 10, clientY: 10 });
  // Below the 3px threshold the browser would call this a click, not a drag.
  card.dispatch("pointermove", { clientX: 11, clientY: 11 });
  card.dispatch("pointerup", {});
  assert.equal(call("workflowGraph[0].x === undefined"), true, "no x is invented");
  assert.equal(call("workflowGraph[0].y === undefined"), true, "no y is invented");

  card.dispatch("click");
  assert.equal(call("selectedWorkflowNode"), "plan", "the click selects the node");
});

test("an empty workflow says so instead of rendering a blank box", () => {
  const ui = loadApp();
  evaluate(ui, "workflowGraph = []");
  evaluate(ui, "renderWorkflowCanvas()");
  const layer = ui.document.getElementById("workflowNodesLayer");
  assert.equal(layer.children.length, 1);
  const text = String(layer.children[0].textContent || "");
  assert.ok(text.includes("No nodes yet"), text);
});

test("ide.js builds its chrome on top of app.js without throwing", () => {
  const ui = loadApp();
  const createdBefore = ui.created.length;
  loadIde(ui);

  assert.ok(ui.created.length > createdBefore, "the IDE shell created elements");
  assert.equal(typeof ui.window.api, "function", "it wrapped the app's api");

  const workbench = ui.created.find((node) => hasClass(node, "workbench-empty"));
  assert.ok(workbench, "the empty-workbench pane exists");
  assert.ok(String(workbench.innerHTML).includes("No file open"), workbench.innerHTML);
  assert.ok(
    walk(ui.document.querySelector(".main")).some((node) => node === workbench),
    "the workbench is inside the main pane",
  );

  // The shell is chrome, not one pane: a real boot builds many elements rather
  // than the single workbench the assertion above already covers.
  assert.ok(
    ui.created.length - createdBefore >= 10,
    `the shell built ${ui.created.length - createdBefore} elements`,
  );
});
