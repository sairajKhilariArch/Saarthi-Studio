// Extension host: pick a backend (running desktop app, or the built-in server),
// show the Studio interface in a webview, and expose editor commands.

import * as vscode from "vscode";
import {
  EmbeddedServer,
  StudioConfig,
  probeDesktop,
  startEmbeddedServer,
  normalizeBaseUrl,
} from "./lib";

const API_KEY_SECRET = "freeAiStudio.apiKey";
const MAX_ATTACHMENT_CHARS = 50_000;

interface Backend {
  kind: "desktop" | "embedded";
  url: string;
  note?: string;
}

let backend: Backend | undefined;
let embedded: EmbeddedServer | undefined;
let panel: vscode.WebviewPanel | undefined;
let statusBar: vscode.StatusBarItem;
let output: vscode.OutputChannel;
let contextRef: vscode.ExtensionContext;
// The key is read from secret storage once at activation and kept in memory so
// the built-in server's synchronous config provider can see it.
let apiKeyCache = "";

export function activate(context: vscode.ExtensionContext): void {
  contextRef = context;
  output = vscode.window.createOutputChannel("Free AI Studio");
  statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 90);
  statusBar.command = "freeAiStudio.showStatus";
  statusBar.text = "$(sparkle) Studio";
  statusBar.tooltip = "Free AI Studio";
  statusBar.show();
  context.subscriptions.push(statusBar, output);

  void context.secrets.get(API_KEY_SECRET).then((value) => {
    apiKeyCache = value || "";
  });

  const register = (id: string, handler: (...args: any[]) => unknown) =>
    context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  register("freeAiStudio.openChat", () => openChat());
  register("freeAiStudio.askSelection", () => askSelection());
  register("freeAiStudio.attachFile", () => attachActiveFile());
  register("freeAiStudio.setApiKey", () => setApiKey());
  register("freeAiStudio.showStatus", () => showStatus());
  register("freeAiStudio.restartServer", () => restartServer());
  register("freeAiStudio.openInDesktop", async () => {
    const url = vscode.workspace.getConfiguration("freeAiStudio").get<string>("desktopUrl", "http://127.0.0.1:8765");
    await vscode.env.openExternal(vscode.Uri.parse(url));
  });

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (!event.affectsConfiguration("freeAiStudio")) return;
      // A changed desktop URL or router setting invalidates the chosen backend.
      await shutdownEmbedded();
      backend = undefined;
      await refreshStatus();
    }),
  );

  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(async () => {
      await shutdownEmbedded();
      backend = undefined;
      await refreshStatus();
    }),
  );

  void refreshStatus();
}

export async function deactivate(): Promise<void> {
  await shutdownEmbedded();
}

/* ------------------------------------------------------------------ */
/* backend selection                                                   */
/* ------------------------------------------------------------------ */

function readConfig(): StudioConfig {
  const config = vscode.workspace.getConfiguration("freeAiStudio");
  const apiKey = apiKeyCache;
  return {
    routerBaseUrl: normalizeBaseUrl(config.get<string>("routerBaseUrl", "http://127.0.0.1:3001/v1")),
    apiKey,
    model: config.get<string>("model", "auto") || "auto",
    contextBudgetTokens: clamp(config.get<number>("contextBudgetTokens", 6000), 1000, 32000),
    historyMessages: clamp(config.get<number>("historyMessages", 6), 2, 24),
    sendWorkspaceOutline: true,
  };
}

function clamp(value: number, min: number, max: number): number {
  const number = Number(value);
  if (!Number.isFinite(number)) return min;
  return Math.min(max, Math.max(min, Math.round(number)));
}

async function resolveBackend(): Promise<Backend> {
  if (backend) return backend;
  const config = vscode.workspace.getConfiguration("freeAiStudio");
  const desktopUrl = String(config.get<string>("desktopUrl", "http://127.0.0.1:8765") || "").replace(/\/+$/, "");
  const probe = await probeDesktop(desktopUrl);
  output.appendLine(`desktop probe ${desktopUrl}: running=${probe.running} embeddable=${probe.embeddable}`);

  if (probe.running && probe.embeddable) {
    backend = { kind: "desktop", url: desktopUrl };
    return backend;
  }

  const server = await ensureEmbedded();
  backend = {
    kind: "embedded",
    url: server.url,
    note: probe.running
      ? "The desktop app is running but cannot be embedded in this window; this panel uses the built-in server."
      : "The desktop app is not running, so the built-in server is serving this panel.",
  };
  return backend;
}

async function ensureEmbedded(): Promise<EmbeddedServer> {
  if (embedded) return embedded;
  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  embedded = await startEmbeddedServer({
    uiDir: vscode.Uri.joinPath(contextRef.extensionUri, "media", "ui").fsPath,
    storeFile: vscode.Uri.joinPath(contextRef.globalStorageUri, "history.json").fsPath,
    workspaceRoot,
    getConfig: readConfig,
    log: (message) => output.appendLine(message),
  });
  output.appendLine(`built-in server listening on ${embedded.url}`);
  return embedded;
}

async function shutdownEmbedded(): Promise<void> {
  if (!embedded) return;
  const server = embedded;
  embedded = undefined;
  await server.close();
  output.appendLine("built-in server stopped");
}

async function refreshStatus(): Promise<void> {
  try {
    const chosen = await resolveBackend();
    statusBar.text = chosen.kind === "desktop" ? "$(sparkle) Studio · desktop" : "$(sparkle) Studio · built-in";
    statusBar.tooltip = `Free AI Studio (${chosen.kind}) — ${chosen.url}`;
  } catch (error) {
    statusBar.text = "$(sparkle) Studio · error";
    statusBar.tooltip = (error as Error).message;
    output.appendLine(`status error: ${(error as Error).message}`);
  }
}

/* ------------------------------------------------------------------ */
/* panel                                                               */
/* ------------------------------------------------------------------ */

async function openChat(prefill?: { prompt?: string; code?: string; language?: string }): Promise<void> {
  const chosen = await resolveBackend();
  if (!panel) {
    panel = vscode.window.createWebviewPanel(
      "freeAiStudio.chat",
      "Free AI Studio",
      { viewColumn: vscode.ViewColumn.Beside, preserveFocus: false },
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(contextRef.extensionUri, "media")],
      },
    );
    panel.onDidDispose(() => {
      panel = undefined;
    });
    panel.webview.onDidReceiveMessage((message: { type?: string }) => {
      if (message?.type === "freeai:reload") void reloadPanel();
    });
  }
  panel.webview.html = renderPanelHtml(chosen);
  panel.reveal(vscode.ViewColumn.Beside, false);

  if (prefill && (prefill.prompt || prefill.code)) {
    // Give the iframe a moment to boot before forwarding the prefill payload.
    setTimeout(() => {
      void panel?.webview.postMessage({
        type: "freeai:prefill",
        prompt: prefill.prompt || "",
        code: prefill.code || "",
        language: prefill.language || "text",
      });
    }, 700);
  }
  await refreshStatus();
}

async function reloadPanel(): Promise<void> {
  if (!panel) return;
  const chosen = await resolveBackend();
  panel.webview.html = renderPanelHtml(chosen);
}

function renderPanelHtml(chosen: Backend): string {
  const nonce = randomNonce();
  const frameUrl = chosen.url;
  const contentPolicy = [
    "default-src 'none'",
    `frame-src ${frameUrl} http://127.0.0.1:* http://localhost:*`,
    "style-src 'unsafe-inline'",
    `script-src 'nonce-${nonce}'`,
    "img-src data:",
  ].join("; ");

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${contentPolicy}">
<style>
  :root { color-scheme: dark; }
  body {
    margin: 0; padding: 0; height: 100vh; display: flex; flex-direction: column;
    background: var(--vscode-editor-background, #08080a);
    color: var(--vscode-editor-foreground, #ededf2);
    font-family: var(--vscode-font-family, sans-serif);
  }
  .bar {
    display: flex; align-items: center; gap: 8px; padding: 5px 10px;
    font-size: 11px; border-bottom: 1px solid var(--vscode-panel-border, #232329);
    background: var(--vscode-editorGroupHeader-tabsBackground, #0b0b0e);
    flex: none;
  }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: #4bd6a0; }
  .dot.embedded { background: #ff8a3d; }
  .mode { text-transform: uppercase; letter-spacing: .05em; font-weight: 600; font-size: 10px; color: #ff8a3d; }
  .note { opacity: .7; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar button {
    margin-left: auto; background: none; border: 1px solid var(--vscode-panel-border, #232329);
    color: inherit; border-radius: 5px; padding: 2px 8px; font-size: 10.5px; cursor: pointer;
  }
  iframe { flex: 1; border: 0; width: 100%; background: #08080a; }
</style>
</head>
<body>
  <div class="bar">
    <span class="dot ${chosen.kind}"></span>
    <span class="mode">${chosen.kind === "desktop" ? "desktop app" : "built-in server"}</span>
    <span class="note">${escapeAttribute(chosen.note || frameUrl)}</span>
    <button id="reload">Reload</button>
  </div>
  <iframe id="studio" src="${frameUrl}" title="Free AI Studio"></iframe>
  <script nonce="${nonce}">
    const frame = document.getElementById("studio");
    const vscode = acquireVsCodeApi();
    window.addEventListener("message", (event) => {
      const data = event.data;
      if (!data || data.type !== "freeai:prefill" || !frame.contentWindow) return;
      frame.contentWindow.postMessage({ ...data, source: "free-ai-studio-extension" }, "${frameUrl}");
    });
    document.getElementById("reload").addEventListener("click", () => {
      frame.src = "${frameUrl}";
      vscode.postMessage({ type: "freeai:reload" });
    });
  </script>
</body>
</html>`;
}

function escapeAttribute(value: string): string {
  return String(value).replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[char] as string));
}

function randomNonce(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let nonce = "";
  for (let index = 0; index < 32; index += 1) nonce += alphabet[Math.floor(Math.random() * alphabet.length)];
  return nonce;
}

/* ------------------------------------------------------------------ */
/* editor integration                                                  */
/* ------------------------------------------------------------------ */

async function askSelection(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    void vscode.window.showInformationMessage("Open a file and select some code first.");
    return;
  }
  const selection = editor.selection;
  const selected = selection.isEmpty ? "" : editor.document.getText(selection);
  const code = (selected || editor.document.getText()).slice(0, MAX_ATTACHMENT_CHARS);
  const truncated = (selected || editor.document.getText()).length > MAX_ATTACHMENT_CHARS;
  const prompt = await vscode.window.showInputBox({
    prompt: "What should Free AI Studio do with this code?",
    value: "Explain what this code does and point out anything risky.",
  });
  if (prompt === undefined) return;
  await openChat({
    prompt: truncated ? `${prompt}\n\n(The attached file was truncated to ${MAX_ATTACHMENT_CHARS} characters.)` : prompt,
    code,
    language: editor.document.languageId,
  });
}

async function attachActiveFile(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    void vscode.window.showInformationMessage("Open a file first.");
    return;
  }
  const text = editor.document.getText();
  await openChat({ prompt: "", code: text.slice(0, MAX_ATTACHMENT_CHARS), language: editor.document.languageId });
  void vscode.window.showInformationMessage(
    text.length > MAX_ATTACHMENT_CHARS
      ? `Attached the first ${MAX_ATTACHMENT_CHARS} characters of ${editor.document.fileName}.`
      : `Attached ${editor.document.fileName}.`,
  );
}

async function setApiKey(): Promise<void> {
  const value = await vscode.window.showInputBox({
    prompt: "Router API key (stored in VS Code's secret storage)",
    password: true,
    ignoreFocusOut: true,
  });
  if (value === undefined) return;
  if (!value.trim()) {
    apiKeyCache = "";
    await contextRef.secrets.delete(API_KEY_SECRET);
    void vscode.window.showInformationMessage("Free AI Studio: router API key cleared.");
    return;
  }
  apiKeyCache = value.trim();
  await contextRef.secrets.store(API_KEY_SECRET, apiKeyCache);
  void vscode.window.showInformationMessage("Free AI Studio: router API key saved.");
}

async function showStatus(): Promise<void> {
  const chosen = await resolveBackend();
  const config = readConfig();
  const lines = [
    `Backend: ${chosen.kind === "desktop" ? "desktop app" : "built-in server"} (${chosen.url})`,
    `Router: ${config.routerBaseUrl}`,
    `Model: ${config.model}`,
    `Context budget: ${config.contextBudgetTokens} tokens · ${config.historyMessages} messages`,
    `API key: ${config.apiKey ? "saved" : "not set (run “Free AI Studio: Set router API key”)"}`,
    `Workspace: ${vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || "no folder open"}`,
  ];
  const action = await vscode.window.showInformationMessage(lines.join("\n"), "Open chat", "Restart server");
  if (action === "Open chat") await openChat();
  if (action === "Restart server") await restartServer();
}

async function restartServer(): Promise<void> {
  await shutdownEmbedded();
  backend = undefined;
  await refreshStatus();
  if (panel) await reloadPanel();
  void vscode.window.showInformationMessage("Free AI Studio: server restarted.");
}
