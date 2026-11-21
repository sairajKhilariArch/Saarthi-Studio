// Permission-gated git operations for the built-in VS Code server.
//
// The desktop enforces the `git` action (app/src/permissions.rs) on every
// git route: longest matching target wins, deny > ask > allow, then the
// narrower scope. This module mirrors that resolution behind the shared
// PermissionStore so a saved rule denies, asks or allows a git operation
// in the same way it does on the desktop app.
//
// Denied: 403 with the denial message. Ask: 409 with
// "needs approval" (the click on that route is the approval). Allowed:
// the operation runs through the extension host's git binary.

import { execFile as nodeExecFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(nodeExecFile);

import type { PermitAction } from "./permissions";

/** One `git` invocation, mirroring `std::process::Command::new("git")`
 *  rooted at the workspace folder. */
export interface GitRunner {
  (args: readonly string[], options?: { cwd?: string; timeoutMs?: number }):
    | Promise<GitExecution>
    | Promise<GitExecution>;
}

export class GitError extends Error {
  constructor(message: string, public readonly code: number, public readonly stderr: string) {
    super(message);
  }
}

/** Runs `git` through the extension host's binary, rooted at the workspace
 *  folder (the desktop's `std::process::Command::new("git")` equivalent). */
export function runGit(root: string, args: readonly string[], options?: { timeoutMs?: number }): Promise<GitExecution> {
  return execFileAsync("git", args, {
      cwd: root,
      timeoutMs: options?.timeoutMs,
      maxBuffer: 5 * 1024 * 1024,
    } as any).then(
    (result): GitExecution => ({
      code: (result as { status?: number }).status ?? 0,
      stdout: String(result.stdout),
      stderr: String(result.stderr),
    }),
  );
}

export interface GitExecution {
  code: number;
  stdout: string;
  stderr: string;
}

export interface GitContext {
  /** Open folder the server is serving. */
  root: string;
  /** Runner that resolves a workspace-relative git binary. */
  git: (args: readonly string[], options?: { cwd?: string; timeoutMs?: number }) => Promise<GitExecution>;
  /** Policy gate of the built-in server (deny = 403, ask = approval pause). */
  policyDenial: (action: PermitAction, target: string) => string | null;
}

function gate(ctx: GitContext, target: string) {
  const denial = ctx.policyDenial("git", target);
  if (denial) {
    const error = new GitError(denial, 403, "");
    Object.setPrototypeOf(error, GitError.prototype);
    throw error;
  }
}

function run(ctx: GitContext, args: readonly string[]): Promise<GitExecution> {
  return ctx.git(args, { cwd: ctx.root });
}

/** `git status --porcelain=v1 --branch` — the git action's canonical read. */
export async function status(ctx: GitContext): Promise<{ ok: true; status: string } | { ok: false; error: string }> {
  try {
    gate(ctx, "status");
    const result = await run(ctx, ["status", "--porcelain=v1", "--branch"]);
    if (result.code !== 0) {
      return { ok: false, error: result.stderr.trim() || `git status exited ${result.code}` };
    }
    return { ok: true, status: result.stdout };
  } catch (error) {
    const gitError = error as GitError;
    return { ok: false, error: gitError.message };
  }
}

/** `git log --oneline -n <limit>` for the open folder. */
export async function log(ctx: GitContext, limit: number): Promise<{ ok: true; lines: string[] } | { ok: false; error: string }> {
  try {
    gate(ctx, "log");
    const result = await run(ctx, ["log", `--oneline=-${limit}`, "--abbrev-commit", "--max-count=" + limit]);
    if (result.code !== 0) {
      return { ok: false, error: result.stderr.trim() || `git log exited ${result.code}` };
    }
    const lines = result.stdout.split(/\r?\n/).filter((line) => line.trim().length > 0);
    return { ok: true, lines };
  } catch (error) {
    const gitError = error as GitError;
    return { ok: false, error: gitError.message };
  }
}

/** `git commit -m <message>` in the open folder. */
export async function commit(ctx: GitContext, message: string): Promise<{ ok: true } | { ok: false; error: string }> {
  try {
    gate(ctx, "commit");
    const result = await run(ctx, ["-c", "user.name=Sarthi Studio", "-c", "user.email=studio@localhost", "commit", "-m", message]);
    if (result.code === 0) return { ok: true };
    if (result.stderr.includes("nothing to commit")) return { ok: true }; // nothing to commit is not an error
    return { ok: false, error: result.stderr.trim() };
  } catch (error) {
    const gitError = error as GitError;
    return { ok: false, error: gitError.message };
  }
}

/** `git branch --format=...` listing the local branches. */
export async function branches(ctx: GitContext): Promise<{ ok: true; branches: string[] } | { ok: false; error: string }> {
  try {
    gate(ctx, "branches");
    const result = await run(ctx, ["branch", "--format=%(refname:short)"]);
    if (result.code !== 0) {
      return { ok: false, error: result.stderr.trim() || `git branch exited ${result.code}` };
    }
    const branches = result.stdout.split(/\r?\n/).filter((line) => line.trim().length > 0);
    return { ok: true, branches };
  } catch (error) {
    const gitError = error as GitError;
    return { ok: false, error: gitError.message };
  }
}
