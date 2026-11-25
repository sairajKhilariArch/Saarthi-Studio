# Sarthi Studio — Final User Guide

**Version:** 3.88.18 (acceptance evidence gathered on 3.88.17; 3.88.18 is a docs-only bump)  
**Product:** local AI coding and chat workspace for Linux, Windows, macOS, Android companion and VS Code  
**Verified platform for this guide:** Linux x86_64, local UI at `http://127.0.0.1:8765` (or the port you choose)

Sarthi Studio is a local, single-user application. The Rust server, SQLite history, workspace files and credentials stay on your computer. It talks to an OpenAI-compatible router or provider you configure; it does not create an online account.

## 1. First launch

1. Start `free-ai-studio` (or the installed `sarthi-studio` launcher).
2. The application window opens the same UI that is served at `http://127.0.0.1:8765` by default. Use `--browser` to open it in your default browser, or `--headless`/`--no-gui` to run the server without a window.
3. Open **Settings** (activity bar or sidebar footer).
4. Choose a **backend platform** preset or keep the built-in FreeLLMAPI router.
5. Enter the **router base URL** and **router API key**. A saved key is stored in the OS credential store when available; otherwise it is kept in the permission-restricted local config.
6. Choose a model, or leave `auto` when the router selects and falls back.
7. Click **Test connection**, then **Save settings**.
8. Restart once and confirm the settings return.

If no provider is configured, the app still opens and the local workspace, skills, history and settings work; chat returns a clear provider error.

## 2. Projects and workspace

- **Open folder…** in the sidebar opens a local directory. The explorer can browse, create, rename, edit and delete files and folders.
- Editing is text-only. Saves are atomic, `Ctrl+S` saves, and unsaved changes are protected when closing a tab.
- The app is confined to the opened folder. `..`, absolute escapes and symlinks that leave the folder are refused. Binary files and files over 1 MB are not opened.
- Use the **Scratch pad** for one-off code and **Attach to chat** to add file context.
- Project switching is separate from folder opening; reopening a project restores its folder.

## 3. Chat

1. Create a thread with **New thread**.
2. Select the provider/model if not using `auto`.
3. Type a message and send. Replies stream live; **Stop** stops generation and keeps the partial answer.
4. Markdown and code blocks are rendered with copy buttons.
5. Attach images, audio or video with the composer; they are stored with the message and sent with the newest user message only.
6. The composer meter shows approximate context use against the configured budget (6,000 tokens by default). Long threads are packed, not resent whole.
7. Restart the app and reopen the thread to confirm history and model selection are restored.

Chat is backed by SQLite. A conversation can be deleted as a whole; per-message edit/delete/regenerate is not implemented.

## 4. Skills

- **Skills** in the sidebar lists local `SKILL.md` packs discovered from the project, the user config directory, `~/.agents/skills` and bundled skills.
- Enable or disable a skill. Enabled skills are injected into the prompt; disabled ones remain installed.
- In the composer, attach one skill to a message, or accept a keyword-based suggestion.
- A skill that is missing or requires a newer Studio version is rejected with an error naming the problem.
- Skills are instructions, not executable code. Treat third-party skill text as untrusted guidance.

## 5. Agents

- Open **Agents** to create/configure a run.
- Choose a profile, model, skill (optional), step and token budget, and whether writes are allowed.
- Start the run and watch its steps. Writes and commands require approval unless the run is explicitly configured to auto-approve.
- **Cancel** stops the run; a killed process is marked `interrupted` on restart and can be resumed where allowed.
- Execution history and step logs stay in the local database.

## 6. Workflows

- Create a workflow with agent nodes and dependencies.
- A node can carry a skill and prompt; dependent nodes run after their prerequisites.
- Validate before running: a missing skill or invalid graph is rejected before execution.
- Run, inspect child runs/logs, cancel, retry, or resume an interrupted run.
- A schedule interval can be set (5 minutes to 7 days, or manual). Scheduled runs are claimed in the database so a restart does not fire early.

## 7. Terminal, source control and tools

- The **Terminal** panel runs a real PTY on the desktop host. It is not available in the VS Code extension host, which answers `501`.
- **Source control** shows git status and supports the implemented staging/commit flow.
- Language-server and debug-adapter clients exist; they require an installed external server/debugger. The app does not bundle a language server.
- Diagnostics and extension settings are under **Settings**.

## 8. Themes and layout

- The moon/sun button switches light and dark themes; the choice persists.
- Panels can be collapsed with the left/right/bottom toggles. The layout remains usable at narrow widths.
- The status bar shows project, run state, router state, model, cursor position, indentation and encoding.

## 9. Data locations (Linux)

| Data | Location |
|---|---|
| Settings | `~/.config/free-ai-studio/config.json` |
| Last opened folder | `~/.config/free-ai-studio/workspace.json` |
| Router ownership | `~/.config/free-ai-studio/router-owned.json` |
| Chat history / database | `~/.local/share/free-ai-studio/history.sqlite3` |
| Structured log | `~/.local/share/free-ai-studio/studio.log` |
| Source-install log (stdout) | `~/.local/state/sarthi-studio/studio.log` |

Windows and macOS use the corresponding platform config/data directories created by the app. The source installer uses `~/.local/share/sarthi-studio/app` for its build.

If `config.json` cannot be parsed, the app boots with defaults and keeps the original at `config.json.corrupt-<timestamp>`; check that file to recover settings.

## 10. Troubleshooting

| Symptom | Check |
|---|---|
| Chat fails | Provider base URL, API key, router running, **Test connection** |
| Port already in use | Choose another port with `FREE_AI_STUDIO_PORT=9000` |
| No UI window | Run with `--browser`; check `FREE_AI_STUDIO_NO_GUI` is not set |
| Folder cannot be opened | Confirm it exists and is readable; workspace access is restricted to it |
| Skills not listed | Check the project/user/bundled skill directories and `SKILL.md` validity |
| Agent cannot write | `allowWrites` is false or the approval was denied; this is intended |
| Database error at startup | Back up the file, inspect permissions; the app refuses a corrupt/newer schema rather than overwriting it |
| Lost settings | Look for `config.json.corrupt-*` beside `config.json` |

## 11. Important limitations

- The app is local and loopback-only; there is no account system or public hosting mode.
- Windows/macOS builds have CI coverage but were not installed or launched in Phase 10.
- Release artifacts are unsigned; verify checksums before installing.
- No live update/rollback has been validated.
- Providers, search, MCP, language servers and debuggers were tested against local fixtures, not live vendor services.
- A 2–4 hour soak and disk-full behaviour were not tested.
