import { test } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { ENFORCED_ACTIONS, PERMIT_ACTIONS, PermissionStore } from "../dist/lib.mjs";

const NEW_ACTIONS = [
  "network",
  "git",
  "router_install",
  "update",
  "uninstall",
  "settings",
  "extension_tool",
];

/* The shared wire language is all eleven, so a permissions.json written by the
   desktop still loads. This server enforces only the seven it has routes for. */
const ENFORCED_HERE = ["fs_read", "fs_write", "command", "mcp_tool", "network", "git", "settings"];
const DESKTOP_ONLY = ["router_install", "update", "uninstall", "extension_tool"];

test("PERMIT_ACTIONS carries the eleven actions the desktop enforces", () => {
  const ids = PERMIT_ACTIONS.map((entry) => entry.id);
  assert.equal(PERMIT_ACTIONS.length, 11, "the model must stay in sync with app/src/permissions.rs");
  for (const action of NEW_ACTIONS) {
    assert.ok(ids.includes(action), `missing ${action}`);
  }
});

test("the extension is honest about which actions it enforces", () => {
  for (const action of ENFORCED_HERE) {
    const entry = PERMIT_ACTIONS.find((item) => item.id === action);
    assert.ok(entry, `no entry for ${action}`);
    assert.equal(entry.enforced, true, `${action} has a route in this server`);
  }
  for (const action of DESKTOP_ONLY) {
    const entry = PERMIT_ACTIONS.find((item) => item.id === action);
    assert.ok(entry, `no entry for ${action}`);
    assert.equal(entry.enforced, false, `${action} has no route in this server`);
    assert.match(entry.description, /Desktop app only/);
  }
  assert.deepEqual([...ENFORCED_ACTIONS].sort(), [...ENFORCED_HERE].sort());
});

test("destructive actions default to ask, matching the desktop", () => {
  for (const action of NEW_ACTIONS) {
    const entry = PERMIT_ACTIONS.find((item) => item.id === action);
    assert.ok(entry, `no entry for ${action}`);
    assert.equal(entry.default, "ask", `${action} is deliberately ask-by-default like the desktop`);
  }
});

test("a rule for a desktop-only action is refused, not silently stored", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "studio-perm-"));
  try {
    const store = new PermissionStore(path.join(dir, "permissions.json"));
    for (const action of DESKTOP_ONLY) {
      assert.throws(
        () => store.grant({ action, target: "", decision: "deny", scope: "always", project: "" }, ""),
        /enforced by the Sarthi Studio desktop app/,
        `${action} must not be savable here`,
      );
    }
    // Nothing was written, so the panel cannot show protection that never runs.
    assert.equal(store.rules().length, 0);
    assert.equal(store.check("update", "", "", true), null);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a stored rule from an older build still loads and resolves", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "studio-perm-"));
  try {
    const file = path.join(dir, "permissions.json");
    fs.writeFileSync(
      file,
      JSON.stringify({
        rules: [
          { action: "update", target: "", decision: "deny", scope: "always", project: "", created_at: 1 },
          { action: "network", target: "/providers", decision: "deny", scope: "always", project: "", created_at: 2 },
        ],
        audit: [],
      }),
    );
    const store = new PermissionStore(file);
    // The legacy row is preserved on disk rather than dropped by a rewrite.
    assert.equal(store.rules().length, 2);
    assert.equal(store.check("network", "/providers", "", true).decision, "deny");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the permission store enforces every action it advertises", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "studio-perm-"));
  try {
    const store = new PermissionStore(path.join(dir, "permissions.json"));

    store.grant({ action: "network", target: "/providers", decision: "deny", scope: "always", project: "" }, "");
    {
      const effective = store.check("network", "/providers", "", true);
      assert.equal(effective.decision, "deny");
      assert.match(store.deniedForUser("network", "/providers", "", true), /denies network/);
    }

    for (const action of ENFORCED_HERE) {
      const rule = store.grant(
        { action, target: action === "git" ? "src/main.rs" : "", decision: "ask", scope: "session", project: "" },
        "",
      );
      assert.equal(rule.action, action);
      assert.equal(rule.scope, "session");
      assert.equal(store.check(action, rule.target, "", true).decision, "ask");
    }

    store.revoke({ action: "settings", target: "", scope: "session", project: "" }, "");
    assert.equal(store.check("settings", "anything", "", true), null);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("grant rejects an unknown action, not a new one", () => {
  const store = new PermissionStore("/no/such/dir/permissions.json");
  assert.throws(
    () => store.grant({ action: "not_a_real_action", target: "", decision: "ask", scope: "session", project: "" }, ""),
    /Name one of these actions/,
  );
});
