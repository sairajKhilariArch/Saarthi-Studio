import test from "node:test";
import assert from "node:assert/strict";

import {
  status,
  log,
  commit,
  branches,
  reflog,
  undo,
  merge,
  mergeAbort,
  rebase,
  push,
  fetch,
  pull,
  remotes,
} from "../dist/git.mjs";

/**
 * Fake git context: the extension host's git runner resolves the binary at
 * startup; the service verbs (gate + run + ok/error shape) are exercised
 * here independent of a real repository.
 */
function gitService() {
  return {
    root: "/tmp/studio-fake-git",
    git: async (args, options) => ({
      code: 0,
      stdout: args.map((a) => String(a)).join(" ") + "\n",
      stderr: "",
    }),
    policyDenial: () => null,
  };
}

test("git verb services return ok:true and gate through the policy deny", async (t) => {
  const ctx = gitService();
  assert.equal((await status(ctx)).ok, true);
  assert.equal((await log(ctx, 5)).ok, true);
  assert.equal((await commit(ctx, "a commit")).ok, true);
  assert.equal((await branches(ctx)).ok, true);
  assert.equal((await reflog(ctx, 3)).ok, true);
  assert.equal((await undo(ctx, "HEAD~1")).ok, true);
  assert.equal((await merge(ctx, "main")).ok, true);
  assert.equal((await mergeAbort(ctx)).ok, true);
  assert.equal((await rebase(ctx, "continue")).ok, true);
  assert.equal((await push(ctx, "origin", "HEAD:branch")).ok, true);
  assert.equal((await fetch(ctx, "origin")).ok, true);
  assert.equal((await pull(ctx, "origin", "main")).ok, true);
  const remoteUrls = (await remotes(ctx)).remotes;
  assert.ok(Array.isArray(remoteUrls) && remoteUrls.length > 0);
});

test("permission deny returns the gated shape instead of running the verb", async (t) => {
  const ctx = {
    root: "/tmp/studio-fake-git",
    git: async (args) => ({ code: 0, stdout: args.join(" ") + "\n", stderr: "" }),
    policyDenial: (action, target) =>
      action === "git" && target === "merge" ? "deny git merge" : null,
  };
  const denied = await merge(ctx, "feature");
  assert.equal(denied.ok, false);
  assert.match(denied.error, /^deny git merge$/);
});
