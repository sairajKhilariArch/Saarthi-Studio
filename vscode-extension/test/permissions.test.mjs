import { test } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { PERMIT_ACTIONS, PermissionStore } from "../dist/lib.mjs";

const NEW_ACTIONS = [
  "network",
  "git",
  "router_install",
  "update",
  "uninstall",
  "settings",
  "extension_tool",
];

test("PERMIT_ACTIONS carries the seven actions the desktop enforces", () => {
  const ids = PERMIT_ACTIONS.map((entry) => entry.id);
  assert.equal(PERMIT_ACTIONS.length, 11, "the model must stay in sync with app/src/permissions.rs");
  for (const action of NEW_ACTIONS) {
    assert.ok(ids.includes(action), `missing ${action}`);
  }
});

test("destructive actions default to ask, matching the desktop", () => {
  for (const action of NEW_ACTIONS) {
    const entry = PERMIT_ACTIONS.find((item) => item.id === action);
    assert.ok(entry, `no entry for ${action}`);
    assert.equal(entry.default, "ask", `${action} is deliberately ask-by-default like the desktop`);
  }
});

test("the permission store understands and enforces every new action", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "studio-perm-"));
  try {
    const store = new PermissionStore(path.join(dir, "permissions.json"));

    // A stored deny for one of the new actions must be respected.
    store.grant({ action: "network", target: "/providers", decision: "deny", scope: "always", project: "" }, "");
    {
      const effective = store.check("network", "/providers", "", true);
      assert.equal(effective.decision, "deny");
      assert.match(store.deniedForUser("network", "/providers", "", true), /denies network/);
    }

    // Grant the other six new actions; each one stays within the shared store.
    for (const action of ["git", "router_install", "update", "uninstall", "settings", "extension_tool"]) {
      const rule = store.grant({ action, target: action === "git" ? "src/main.rs" : "", decision: "ask", scope: "session", project: "" }, "");
      assert.equal(rule.action, action);
      assert.equal(rule.scope, "session");
      const effective = store.check(action, rule.target, "", true);
      assert.equal(effective.decision, "ask");
    }

    // Revoking works for the new actions too.
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
