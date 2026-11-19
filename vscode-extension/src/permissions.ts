// Saved permission rules for the VS Code extension's built-in server.
//
// The desktop owns the full policy engine (app/src/permissions.rs): eleven
// action kinds, four scopes, an audit trail and an emergency stop. This module
// stores and resolves the three actions this server can actually enforce, so a
// saved rule means something here instead of being a checkbox that lies:
//
//   * fs_read   — a file the agent reads or a folder it lists (allowed by
//                 default here, as on the desktop, so a deny is the useful
//                 rule: it keeps a path out of the model's reach);
//   * fs_write  — a file the agent writes (and the editor's own save route is
//                 left to the person driving it);
//   * command   — a command line the agent runs;
//   * mcp_tool  — one `server.tool` an agent or the MCP panel calls.
//
// Resolution is the desktop's: the longest matching target wins, then
// deny > ask > allow, then the narrower scope. A rule can only ever make an
// action stricter or equally strict — a run's own flags still decide the
// ceiling, and `allow` never widens them.

import * as fs from "node:fs";
import * as path from "node:path";

export type PermitAction = "fs_read" | "fs_write" | "command" | "mcp_tool";
/** The safety release is recorded too, as it is on the desktop. */
export type PermitAuditAction = PermitAction | "emergency-stop";
export type PermitDecision = "allow" | "ask" | "deny";
export type PermitScope = "once" | "session" | "project" | "always";

export interface PermitRule {
  action: PermitAction;
  /** "" covers every target for the action; otherwise a prefix. */
  target: string;
  decision: PermitDecision;
  scope: PermitScope;
  /** Project the rule belongs to, "" for every project. */
  project: string;
  created_at: number;
}

export interface PermitAuditEntry {
  action: PermitAuditAction;
  target: string;
  decision: PermitDecision;
  scope: PermitScope;
  project: string;
  detail: string;
  created_at: number;
}

export interface PermitEffective {
  decision: PermitDecision;
  scope: PermitScope;
}

/** What the shared Permissions panel is told this server enforces. */
export const PERMIT_ACTIONS: Array<{ id: PermitAction; description: string; default: PermitDecision }> = [
  { id: "fs_read", description: "Have an agent read a file or list a folder in the open folder", default: "allow" },
  { id: "fs_write", description: "Have an agent create, change or delete files in the open folder", default: "ask" },
  { id: "command", description: "Have an agent run a shell command in the workspace", default: "ask" },
  { id: "mcp_tool", description: "Call a tool exposed by a registered MCP server", default: "ask" },
];

const MAX_RULES = 200;
const MAX_AUDIT = 200;
const MAX_TARGET = 1000;

function isAction(value: unknown): value is PermitAction {
  return value === "fs_read" || value === "fs_write" || value === "command" || value === "mcp_tool";
}

function isDecision(value: unknown): value is PermitDecision {
  return value === "allow" || value === "ask" || value === "deny";
}

function isScope(value: unknown): value is PermitScope {
  return value === "once" || value === "session" || value === "project" || value === "always";
}

/** Paths and targets arrive with either separator; a rule must still match. */
function normalizeTarget(value: string): string {
  let out = String(value || "").trim().replace(/\\/g, "/");
  while (out.length > 1 && out.endsWith("/")) out = out.slice(0, -1);
  return out;
}

const DECISION_RANK: Record<PermitDecision, number> = { allow: 0, ask: 1, deny: 2 };
const SCOPE_RANK: Record<PermitScope, number> = { always: 0, project: 1, session: 2, once: 3 };

function applies(rule: PermitRule, action: PermitAction, target: string, project: string): boolean {
  if (rule.action !== action) return false;
  if (rule.project && rule.project !== project) return false;
  const ruleTarget = normalizeTarget(rule.target);
  return !ruleTarget || normalizeTarget(target).startsWith(ruleTarget);
}

function prefer(candidate: PermitRule, current: PermitRule): boolean {
  const left = [candidate.target.length, DECISION_RANK[candidate.decision], SCOPE_RANK[candidate.scope]];
  const right = [current.target.length, DECISION_RANK[current.decision], SCOPE_RANK[current.scope]];
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) return left[index] > right[index];
  }
  return false;
}

/** The winning rule's decision, or null when no rule covers the action. */
export function resolveRule(
  rules: PermitRule[],
  action: PermitAction,
  target: string,
  project: string,
): PermitRule | null {
  let best: PermitRule | null = null;
  for (const rule of rules) {
    if (!applies(rule, action, target, project)) continue;
    if (!best || prefer(rule, best)) best = rule;
  }
  return best;
}

interface StoreFile {
  rules: PermitRule[];
  audit: PermitAuditEntry[];
}

export class PermissionStore {
  private session: PermitRule[] = [];

  constructor(private readonly file: string) {}

  private read(): StoreFile {
    try {
      const parsed = JSON.parse(fs.readFileSync(this.file, "utf8")) as Partial<StoreFile>;
      return {
        rules: Array.isArray(parsed.rules) ? parsed.rules.filter((rule) => this.valid(rule)) : [],
        audit: Array.isArray(parsed.audit) ? parsed.audit.slice(-MAX_AUDIT) : [],
      };
    } catch {
      // A policy file that cannot be parsed is empty, not permissive: the
      // actions' own ask-by-default still applies.
      return { rules: [], audit: [] };
    }
  }

  private valid(rule: unknown): rule is PermitRule {
    if (!rule || typeof rule !== "object") return false;
    const entry = rule as Record<string, unknown>;
    return (
      isAction(entry.action) &&
      isDecision(entry.decision) &&
      isScope(entry.scope) &&
      typeof entry.target === "string" &&
      typeof entry.project === "string"
    );
  }

  private write(data: StoreFile): void {
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    const staging = `${this.file}.tmp-${process.pid}`;
    fs.writeFileSync(
      staging,
      `${JSON.stringify({ rules: data.rules, audit: data.audit.slice(-MAX_AUDIT) }, null, 2)}\n`,
      { encoding: "utf8", mode: 0o600 },
    );
    fs.renameSync(staging, this.file);
    try {
      fs.chmodSync(this.file, 0o600);
    } catch {
      // Windows has no POSIX mode; the file still lives in the app's data folder.
    }
  }

  /** Persisted rules first, session rules after (a session rule may override). */
  rules(): PermitRule[] {
    const stored = this.read().rules;
    return [...stored, ...this.session];
  }

  audit(limit = 50): PermitAuditEntry[] {
    return this.read().audit.slice(-Math.max(1, Math.min(MAX_AUDIT, limit)));
  }

  record(
    action: PermitAuditAction,
    target: string,
    decision: PermitDecision,
    scope: PermitScope,
    project: string,
    detail: string,
  ): void {
    const data = this.read();
    data.audit.push({
      action,
      target: normalizeTarget(target).slice(0, MAX_TARGET),
      decision,
      scope,
      project,
      detail: String(detail || "").slice(0, 400),
      created_at: Date.now() / 1000,
    });
    this.write(data);
  }

  grant(input: Record<string, unknown>, project: string): PermitRule {
    if (!isAction(input.action)) {
      throw new Error(`Name one of these actions: ${PERMIT_ACTIONS.map((entry) => entry.id).join(", ")}.`);
    }
    if (!isDecision(input.decision)) throw new Error("Decision must be allow, ask or deny.");
    if (!isScope(input.scope)) throw new Error("Scope must be once, session, project or always.");
    const target = String(input.target ?? "").trim();
    if (target.length > MAX_TARGET) throw new Error("That target is too long.");
    if (/[\u0000-\u001f\u007f]/.test(target)) throw new Error("The target cannot contain control characters.");
    const wantedProject = input.scope === "project" ? String(input.project || project || "").trim() : "";
    if (input.scope === "project" && !wantedProject) {
      throw new Error("Open a project before saving a project-scoped rule.");
    }
    const rule: PermitRule = {
      action: input.action,
      target: normalizeTarget(target),
      decision: input.decision,
      scope: input.scope,
      project: wantedProject,
      created_at: Date.now() / 1000,
    };
    if (rule.scope === "once" || rule.scope === "session") {
      this.session.push(rule);
      this.record(rule.action, rule.target, rule.decision, rule.scope, rule.project, "session rule saved");
      return rule;
    }
    const data = this.read();
    if (data.rules.length >= MAX_RULES) throw new Error(`At most ${MAX_RULES} saved permission rules.`);
    data.rules.push(rule);
    this.write(data);
    this.record(rule.action, rule.target, rule.decision, rule.scope, rule.project, "rule saved");
    return rule;
  }

  /** Remove matching rules (every scope) and report how many went. */
  revoke(input: Record<string, unknown>, project: string): number {
    if (!isAction(input.action)) throw new Error("Name the action to revoke.");
    const wantedAction: PermitAction = input.action;
    const wantedTarget = normalizeTarget(String(input.target ?? ""));
    const wantedScope = isScope(input.scope) ? input.scope : null;
    const wantedProject = String(input.project || project || "").trim();
    const matches = (rule: PermitRule): boolean =>
      rule.action === wantedAction &&
      normalizeTarget(rule.target) === wantedTarget &&
      (!wantedScope || rule.scope === wantedScope) &&
      (!wantedProject || rule.project === wantedProject);
    const data = this.read();
    const matched = data.rules.filter((rule) => matches(rule));
    data.rules = data.rules.filter((rule) => !matches(rule));
    if (data.rules.length !== matched.length) this.write(data);
    const matchedSession = this.session.filter((rule) => matches(rule));
    this.session = this.session.filter((rule) => !matches(rule));
    const removed = matched.length + matchedSession.length;
    if (removed) {
      const last = [...matched, ...matchedSession][0];
      this.record(
        wantedAction,
        wantedTarget,
        last.decision,
        wantedScope || last.scope,
        wantedProject,
        `revoked ${removed} rule(s)`,
      );
    }
    return removed;
  }

  /** Consume a `once` rule the moment it answers, so it really is one use. */
  private consume(action: PermitAction, target: string, project: string): void {
    const wanted = normalizeTarget(target);
    const mine = (entry: PermitRule): boolean => {
      const entryTarget = normalizeTarget(entry.target);
      return (
        entry.action === action &&
        entry.scope === "once" &&
        (!entry.project || entry.project === project) &&
        (!entryTarget || wanted.startsWith(entryTarget))
      );
    };
    const data = this.read();
    const before = data.rules.length;
    data.rules = data.rules.filter((entry) => !mine(entry));
    if (before !== data.rules.length) this.write(data);
    this.session = this.session.filter((entry) => !mine(entry));
  }

  /**
   * The decision for one action, or null when no rule covers it. `audit` is
   * for the agent path: a route a person clicks should not record an ask the
   * click itself answers.
   */
  check(
    action: PermitAction,
    target: string,
    project: string,
    audit = false,
  ): PermitEffective | null {
    const rule = resolveRule(this.rules(), action, target, project);
    if (!rule) return null;
    if (rule.scope === "once") this.consume(action, target, project);
    if (audit && rule.decision !== "allow") {
      this.record(
        action,
        target,
        rule.decision,
        rule.scope,
        project,
        rule.decision === "deny" ? "denied by a saved rule" : "needs approval (saved rule)",
      );
    }
    return { decision: rule.decision, scope: rule.scope };
  }

  /** Only a saved deny blocks a person-driven route; a click is the yes. */
  deniedForUser(action: PermitAction, target: string, project: string): string | null {
    const rule = resolveRule(this.rules(), action, target, project);
    if (!rule || rule.decision !== "deny") return null;
    this.record(action, target, "deny", rule.scope, project, "denied by a saved rule");
    return `A saved permission rule denies ${action} on "${target || "everything"}". Change it in Settings → Permissions.`;
  }
}
