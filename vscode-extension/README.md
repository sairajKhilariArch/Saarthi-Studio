# Free AI Studio for VS Code

The same local-first AI workspace as the desktop app, inside VS Code.

The extension probes `http://127.0.0.1:8765/api/health` when it starts:

- **Desktop app running** → the panel embeds the running app, so threads, the
  file tree, the editor, usage and the router console are exactly the ones the
  desktop app has.
- **Desktop app not running** → the extension starts its **own** HTTP server on
  a free loopback port, packaged inside the extension. It serves the same UI and
  the same API surface: chat, settings, history, usage, the router console, and a
  folder API scoped to the workspace VS Code has open.

Either way nothing leaves your machine except the request you send to your own
router.

## Commands

| Command | What it does |
| --- | --- |
| `Free AI Studio: Open chat` | Opens the panel in a side column |
| `Free AI Studio: Ask about the selection` | Sends the selection (or the file) with your question |
| `Free AI Studio: Attach the active file` | Attaches the whole file (up to 50,000 characters) |
| `Free AI Studio: Set router API key` | Stores the router key in VS Code's secret storage |
| `Free AI Studio: Show server status` | Backend, router URL, model, budget, workspace |
| `Free AI Studio: Restart the built-in server` | Restarts the embedded server |
| `Free AI Studio: Open the desktop app` | Opens the desktop app in a browser window |

## Settings

| Setting | Default | Purpose |
| --- | --- | --- |
| `freeAiStudio.desktopUrl` | `http://127.0.0.1:8765` | Loopback address of the desktop app |
| `freeAiStudio.routerBaseUrl` | `http://127.0.0.1:3001/v1` | Router base URL for the built-in server |
| `freeAiStudio.model` | `auto` | Model requested by the built-in server |
| `freeAiStudio.contextBudgetTokens` | `6000` | Hard per-request token budget |
| `freeAiStudio.historyMessages` | `6` | Recent messages competing for that budget |

## How the built-in server packs context

Identical rules to the desktop app, enforced by `src/lib.ts` and covered by
`test/lib.test.mjs`:

1. A short system prompt, plus a names-only outline of the open workspace.
2. Only the newest `historyMessages` messages are considered.
3. Messages older than the budget are dropped; the current message is always
   sent, cropped to 3,000 tokens.
4. Attachments from earlier turns are replaced by a one-line marker instead of
   being re-sent.

## Security notes

- The server binds `127.0.0.1` on a random port and refuses non-loopback `Host`
  headers.
- Folder access is limited to the workspace VS Code has open; `..`, absolute
  paths and symlinks that leave it are rejected.
- The folder picker is delegated to VS Code (`File → Open Folder`).
- The router API key lives in `context.secrets`, never in settings files.
- `POST /v1/...`-style proxy calls are not exposed; only the allow-listed
  FreeLLMAPI admin API families are reachable through the console bridge.

## Development

```bash
cd vscode-extension
npm install
npm run build        # bundles dist/extension.js and copies the desktop UI
npm test             # builds dist/lib.mjs and runs node --test
npm run package      # produces free-ai-studio-vscode.vsix
```

`npm run build` copies `../app/static` into `media/ui`, so the extension always
ships the same interface as the desktop app. Rebuild after changing the UI.
