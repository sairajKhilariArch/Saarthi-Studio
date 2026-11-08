"use strict";
/* Saarthi Studio — IDE workbench shell.
 *
 * Runs after app.js and wraps it, never replaces it: the views, tabs, editor,
 * chat, terminal, settings and every backend call stay exactly as they were.
 * What this file adds is the IDE chrome (activity bar, search and source
 * control sidebars, docked AI panel, tabbed bottom panel, status bar,
 * command palette, in-file find, syntax highlighting) plus the glue that keeps
 * it bound to the real application state. */

(() => {
  const $ = (id) => document.getElementById(id);
  const LEGACY_PANES = ["threads", "files", "skills"];

  const IDE = {
    activity: "explorer",
    pane: "threads",
    bottomTab: "terminal",
    problems: [],
    output: [],
    vcs: null,
    diffKey: null,
    scmHistory: false,
    runner: { presets: [], running: false },
    searchSeq: 0,
    paletteIndex: 0,
    paletteItems: [],
    prefs: loadPrefs(),
  };

  function loadPrefs() {
    const fallback = { fontSize: 12.5, tabSize: 2, wrap: false };
    try {
      const raw = JSON.parse(localStorage.getItem("freeai-editor-prefs") || "null");
      return raw && typeof raw === "object" ? Object.assign({}, fallback, raw) : fallback;
    } catch {
      return fallback;
    }
  }

  function savePrefs() {
    try { localStorage.setItem("freeai-editor-prefs", JSON.stringify(IDE.prefs)); } catch { /* private mode */ }
    applyPrefs();
  }

  function applyPrefs() {
    const root = document.documentElement.style;
    root.setProperty("--code-font-size", `${IDE.prefs.fontSize}px`);
    root.setProperty("--code-tab-size", String(IDE.prefs.tabSize));
    const wrap = IDE.prefs.wrap ? "pre-wrap" : "pre";
    const area = $("codeArea");
    if (area) area.style.whiteSpace = wrap;
    const layer = $("codeLayer");
    if (layer) layer.style.whiteSpace = wrap;
  }

  function el(tag, className, html) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (html != null) node.innerHTML = html;
    return node;
  }

  function svg(name, cls) {
    return `<svg class="icon${cls ? " " + cls : ""}" aria-hidden="true"><use href="#i-${name}"/></svg>`;
  }

  function escapeHtml(value) {
    return String(value == null ? "" : value)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  /* The sprite has no git glyph; add one so the activity bar stays consistent. */
  function injectBranchSymbol() {
    if ($("i-branch")) return;
    const sprite = document.querySelector(".icon-sprite");
    if (!sprite) return;
    const symbol = document.createElementNS("http://www.w3.org/2000/svg", "symbol");
    symbol.setAttribute("id", "i-branch");
    symbol.setAttribute("viewBox", "0 0 24 24");
    symbol.innerHTML = '<circle cx="7" cy="5" r="2.2"/><circle cx="7" cy="19" r="2.2"/><circle cx="17" cy="8" r="2.2"/><path d="M7 7.2v9.6M9.2 5h3.6a4.2 4.2 0 0 1 4.2 4.2v0"/>';
    sprite.appendChild(symbol);
  }

  const ACTIVITIES = [
    { id: "explorer", icon: "folder", label: "Explorer", pane: "files" },
    { id: "search", icon: "search", label: "Search", pane: "search" },
    { id: "scm", icon: "branch", label: "Source control", pane: "scm" },
    { id: "run", icon: "play", label: "Run and task history", view: "runs" },
    { id: "modules", icon: "cube", label: "Modules and connections", view: "integrations" },
    { id: "agents", icon: "user", label: "Agents", view: "agents" },
    { id: "skills", icon: "spark", label: "Skills", view: "skills" },
    { id: "projects", icon: "layers", label: "Projects and workspaces", view: "workspaces" },
    { id: "settings", icon: "gear", label: "Settings", view: "settings" },
  ];

  function buildActivityBar() {
    const bar = el("nav", "activity-bar");
    bar.id = "activityBar";
    bar.setAttribute("aria-label", "Primary navigation");
    const app = $("app");
    app.insertBefore(bar, app.firstElementChild);

    ACTIVITIES.forEach((activity) => {
      const button = el("button", "activity-btn", svg(activity.icon));
      button.type = "button";
      button.dataset.activity = activity.id;
      button.title = activity.label;
      button.setAttribute("aria-label", activity.label);
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("click", () => openActivity(activity.id));
      bar.appendChild(button);
    });
  }

  function openActivity(id) {
    IDE.activity = id;
    document.querySelectorAll(".activity-btn").forEach((button) => {
      const active = button.dataset.activity === id;
      button.classList.toggle("active", active);
      button.setAttribute("aria-pressed", String(active));
    });
    const activity = ACTIVITIES.find((item) => item.id === id);
    if (!activity) return;
    if (activity.pane) {
      showSidebar(activity.pane);
      if ($("sidebar") && $("sidebar").classList.contains("no-left")) togglePanel("left");
      if (activity.pane === "search") $("searchInput").focus();
      if (activity.pane === "scm") refreshScm();
    }
    if (activity.view) setView(activity.view);
  }

  function showSidebar(pane) {
    IDE.pane = pane;
    LEGACY_PANES.forEach((name) => {
      const section = $(`${name}Pane`);
      if (section) section.classList.toggle("hidden", pane !== name);
    });
    if ($("searchPane")) $("searchPane").classList.toggle("hidden", pane !== "search");
    if ($("scmPane")) $("scmPane").classList.toggle("hidden", pane !== "scm");
    ["threads", "files", "skills"].forEach((name) => {
      const button = $(`mode${name.charAt(0).toUpperCase()}${name.slice(1)}`);
      if (!button) return;
      button.classList.toggle("active", pane === name);
      button.setAttribute("aria-selected", String(pane === name));
    });
  }

  function buildSidebarPanes() {
    const sidebar = $("sidebar");
    const foot = sidebar.querySelector(".sidebar-foot");

    const search = el("section", "pane hidden pane-search");
    search.id = "searchPane";
    search.innerHTML = [
      '<div class="search-row"><input id="searchInput" type="search" placeholder="Search the open folder" aria-label="Search the open folder" autocomplete="off"></div>',
      '<p class="search-meta" id="searchMeta">Type at least two characters. Build output and binary files are skipped.</p>',
      '<div class="search-results" id="searchResults"><p class="empty-note">Matches appear here.</p></div>',
    ].join("");
    sidebar.insertBefore(search, foot);

    const scm = el("section", "pane hidden scm-pane");
    scm.id = "scmPane";
    scm.innerHTML = [
      '<div class="pane-head"><p class="pane-label">Source control</p>',
      '<button id="scmRefresh" class="tiny-toggle" type="button">Refresh</button></div>',
      `<div class="scm-branch hidden" id="scmBranchBar">${svg("branch")}<span id="scmBranchName">—</span><small id="scmBranchMeta"></small></div>`,
      '<div class="scm-sync hidden" id="scmSync">' +
      '<button id="scmFetch" class="tiny-toggle" type="button" title="Fetch from the default remote">Fetch</button>' +
      '<button id="scmPull" class="tiny-toggle" type="button" title="Pull the current branch (fast-forward only)">Pull</button>' +
      '<button id="scmPush" class="tiny-toggle" type="button" title="Push the current branch">Push</button>' +
      '<span class="scm-sync-state" id="scmSyncState"></span></div>' +
      '<div id="scmBody" class="scm-list"><p class="empty-note">Open a folder to check its Git state.</p></div>',
      '<div class="scm-commit hidden" id="scmCommitBox">',
      '<textarea id="scmMessage" rows="2" placeholder="Commit message (Ctrl+Enter)" aria-label="Commit message"></textarea>',
      '<div class="scm-commit-row"><button id="scmCommit" class="tiny-toggle primary" type="button">Commit</button>',
      '<button id="scmBranchBtn" class="tiny-toggle" type="button">Branch…</button>',
      '<button id="scmHistoryBtn" class="tiny-toggle" type="button">History</button>',
      '<button id="scmMergeBtn" class="tiny-toggle" type="button">Merge…</button>',
      '<button id="scmRebaseBtn" class="tiny-toggle" type="button">Rebase…</button>',
      '<button id="scmRemotesBtn" class="tiny-toggle" type="button">Remotes…</button></div>',
      '<div class="scm-branch-form hidden" id="scmBranchForm">',
      '<input id="scmBranchInput" type="text" placeholder="branch name" aria-label="Branch name" autocomplete="off">',
      '<button id="scmBranchCreate" class="tiny-toggle" type="button">Create</button>',
      '<button id="scmBranchSwitch" class="tiny-toggle" type="button">Switch</button></div>',
      '<div class="scm-branch-form hidden" id="scmMergeForm">',
      '<input id="scmMergeInput" type="text" placeholder="branch to merge in" aria-label="Branch to merge" autocomplete="off">',
      '<button id="scmMergeGo" class="tiny-toggle primary" type="button">Merge</button>',
      '<button id="scmMergeAbort" class="tiny-toggle" type="button">Abort merge</button></div>',
      '<div class="scm-branch-form hidden" id="scmRebaseForm">',
      '<input id="scmRebaseInput" type="text" placeholder="branch to rebase onto" aria-label="Branch to rebase onto" autocomplete="off">',
      '<button id="scmRebaseGo" class="tiny-toggle primary" type="button">Rebase</button>',
      '<button id="scmRebaseContinue" class="tiny-toggle" type="button">Continue</button>',
      '<button id="scmRebaseAbort" class="tiny-toggle" type="button">Abort rebase</button></div>',
      '<div class="scm-branch-form hidden" id="scmRemoteForm">',
      '<input id="scmRemoteName" type="text" placeholder="remote name" aria-label="Remote name" autocomplete="off">',
      '<input id="scmRemoteUrl" type="text" placeholder="remote URL" aria-label="Remote URL" autocomplete="off">',
      '<button id="scmRemoteAdd" class="tiny-toggle primary" type="button">Add</button></div>',
      '<div class="scm-remote-list hidden" id="scmRemoteList"></div>',
      '<p class="scm-hint" id="scmHint"></p>',
      "</div>",
    ].join("");
    sidebar.insertBefore(scm, foot);

    const resizer = el("div", "sidebar-resizer");
    resizer.title = "Drag to resize the sidebar";
    sidebar.appendChild(resizer);
  }

  /* ---------- docked AI panel ---------- */
  function buildDock() {
    const dock = el("aside", "ai-dock");
    dock.id = "aiDock";
    dock.setAttribute("aria-label", "AI assistant");
    dock.innerHTML = [
      '<header class="ai-dock-head">',
      `<span class="ai-dock-title">${svg("spark")}AI</span>`,
      '<span class="status-pill" id="aiDockModel">auto</span>',
      '<div class="ai-dock-tabs" role="tablist" aria-label="AI panel tabs">',
      '<button type="button" class="ai-dock-tab active" data-dock="chat" role="tab" aria-selected="true">Assistant</button>',
      '<button type="button" class="ai-dock-tab" data-dock="router" role="tab" aria-selected="false">Router</button>',
      "</div>",
      '<div class="ai-dock-actions">',
      `<button type="button" class="icon-btn" id="aiDockClose" title="Hide the AI panel" aria-label="Hide the AI panel">${svg("close")}</button>`,
      "</div></header>",
      '<div class="ai-dock-body"><div class="ai-chat-host" id="aiChatHost"></div>',
      '<div class="ai-router-host hidden" id="aiRouterHost"></div></div>',
    ].join("");
    $("app").insertBefore(dock, $("bottomPanel"));
    // Move the existing chat view and router panel in; their ids do not change.
    $("aiChatHost").appendChild($("chatView"));
    $("aiRouterHost").appendChild($("rightPanel"));

    dock.querySelectorAll(".ai-dock-tab").forEach((tab) => {
      tab.addEventListener("click", () => selectDockTab(tab.dataset.dock));
    });
    $("aiDockClose").addEventListener("click", () => togglePanel("right"));
  }

  function selectDockTab(name) {
    document.querySelectorAll(".ai-dock-tab").forEach((tab) => {
      const active = tab.dataset.dock === name;
      tab.classList.toggle("active", active);
      tab.setAttribute("aria-selected", String(active));
    });
    $("aiChatHost").classList.toggle("hidden", name !== "chat");
    $("aiRouterHost").classList.toggle("hidden", name !== "router");
    if (name === "router") refreshRightPanel();
  }

  /* ---------- bottom panel ---------- */
  const BOTTOM_TABS = [
    ["terminal", "Terminal", null],
    ["problems", "Problems", "problemCount"],
    ["runs", "AI runs", "runsCount"],
    ["tasks", "Tasks", null],
    ["output", "Output", null],
  ];

  function buildBottom() {
    const panel = $("bottomPanel");
    const head = panel.querySelector(".panel-head");
    const title = head.querySelector(".panel-title");
    if (title) title.remove();

    const tabs = el("div", "bottom-tabs");
    tabs.id = "bottomTabs";
    tabs.setAttribute("role", "tablist");
    tabs.setAttribute("aria-label", "Panel views");
    BOTTOM_TABS.forEach(([id, label, countId]) => {
      const button = el("button", "bottom-tab",
        `<span>${label}</span>${countId ? `<span class="count" id="${countId}">0</span>` : ""}`);
      button.type = "button";
      button.dataset.bottom = id;
      button.setAttribute("role", "tab");
      button.addEventListener("click", () => selectBottomTab(id));
      tabs.appendChild(button);
    });
    head.insertBefore(tabs, head.firstChild);

    const body = el("div", "bottom-body");
    body.id = "bottomBody";
    const terminalPane = el("div", "bottom-pane");
    terminalPane.id = "terminalPane";
    terminalPane.appendChild($("terminalHost"));
    body.appendChild(terminalPane);
    const problemsPane = makePane("problemsPane", "problemsBody",
      '<span id="runnerStatus">No build run yet</span>' +
      '<span id="runnerTests" class="muted"></span>' +
      '<button type="button" class="btn btn-quiet btn-small" id="runBuild">Build</button>' +
      '<button type="button" class="btn btn-quiet btn-small" id="runTest">Test</button>' +
      '<button type="button" class="btn btn-quiet btn-small" id="problemsClear">Clear</button>',
      "No problems. Failed saves and requests, and build or test diagnostics, land here.");
    problemsPane.insertAdjacentHTML("beforeend", '<pre id="runnerOutput" class="runner-output hidden" aria-label="Build output"></pre>');
    body.appendChild(problemsPane);
    body.appendChild(makePane("runsPane", "runsBody",
      '<span>Agent runs on this machine</span><button type="button" class="btn btn-quiet btn-small" id="runsRefresh">Refresh</button>',
      "Loading runs…"));
    body.appendChild(makePane("tasksPane", "tasksBody",
      '<span>Saved prompt workflows</span><button type="button" class="btn btn-quiet btn-small" id="tasksRefresh">Refresh</button>',
      "Loading workflows…"));
    body.appendChild(makePane("outputPane", "outputBody",
      '<span>Requests this UI sent to the local server</span><button type="button" class="btn btn-quiet btn-small" id="outputClear">Clear</button>',
      "No requests logged yet."));
    panel.appendChild(body);

    $("problemsClear").addEventListener("click", () => { IDE.problems = []; renderProblems(); updateProblemCounts(); });
    $("runBuild").addEventListener("click", () => void runBuild("build"));
    $("runTest").addEventListener("click", () => void runBuild("test"));
    $("outputClear").addEventListener("click", () => { IDE.output = []; renderOutput(); });
    $("runsRefresh").addEventListener("click", () => void loadRunsPane());
    $("tasksRefresh").addEventListener("click", () => void loadTasksPane());
    selectBottomTab("terminal");
  }

  function makePane(paneId, bodyId, tools, empty) {
    const pane = el("div", "bottom-pane hidden");
    pane.id = paneId;
    pane.innerHTML = `<div class="pane-tools">${tools}</div>` +
      `<div class="pane-scroll" id="${bodyId}"><p class="pane-empty">${empty}</p></div>`;
    return pane;
  }

  function selectBottomTab(id) {
    IDE.bottomTab = id;
    document.querySelectorAll(".bottom-tab").forEach((button) => {
      const active = button.dataset.bottom === id;
      button.classList.toggle("active", active);
      button.setAttribute("aria-selected", String(active));
    });
    [["terminal", "terminalPane"], ["problems", "problemsPane"], ["runs", "runsPane"],
      ["tasks", "tasksPane"], ["output", "outputPane"]].forEach(([name, paneId]) => {
      const pane = $(paneId);
      if (pane) pane.classList.toggle("hidden", name !== id);
    });
    if (state.panelShow && !state.panelShow.bottom) togglePanel("bottom");
    if (id === "terminal") showTerminalPanel();
    if (id === "problems") { renderProblems(); void loadRunnerPresets(); }
    if (id === "runs") void loadRunsPane();
    if (id === "tasks") void loadTasksPane();
    if (id === "output") renderOutput();
  }

  /* ---------- status bar ---------- */
  function buildStatusBar() {
    const bar = el("footer", "status-bar");
    bar.id = "statusBar";
    bar.innerHTML = [
      `<button type="button" class="status-item clickable" id="statusProjectBtn" title="Projects">${svg("layers")}<span id="statusProject">No project</span></button>`,
      `<button type="button" class="status-item clickable hidden" id="statusBranchBtn" title="Source control">${svg("branch")}<span id="statusBranch"></span></button>`,
      `<button type="button" class="status-item clickable" id="statusProblemsBtn" title="Problems">${svg("close")}<span id="statusProblems">0</span></button>`,
      `<span class="status-item" id="statusRunItem">${svg("play")}<span id="statusRun">idle</span></span>`,
      '<span class="status-spacer"></span>',
      `<span class="status-item" title="Router connection"><span class="dot" id="statusConnDot"></span><span id="statusConn">checking</span></span>`,
      `<span class="status-item" title="Model">${svg("cpu")}<span id="statusModel">auto</span></span>`,
      '<span class="status-item" id="statusPos">Ln 1, Col 1</span>',
      '<span class="status-item" id="statusIndent">Spaces: 2</span>',
      '<span class="status-item" id="statusLang">text</span>',
      '<span class="status-item" title="Encoding">UTF-8</span>',
    ].join("");
    $("app").appendChild(bar);

    $("statusProjectBtn").addEventListener("click", () => openActivity("projects"));
    $("statusBranchBtn").addEventListener("click", () => openActivity("scm"));
    $("statusProblemsBtn").addEventListener("click", () => selectBottomTab("problems"));
  }

  /* ---------- breadcrumbs, find bar, palette ---------- */
  function buildBreadcrumbs() {
    const main = document.querySelector(".main");
    const tabbar = main.querySelector(".tabbar");
    const crumbs = el("div", "breadcrumbs hidden");
    crumbs.id = "breadcrumbs";
    crumbs.setAttribute("aria-label", "File path");
    main.insertBefore(crumbs, tabbar.nextSibling);
  }

  function buildFindBar() {
    const view = $("editorView");
    const toolbar = view.querySelector(".editor-toolbar");
    const bar = el("div", "find-bar hidden");
    bar.id = "findBar";
    bar.innerHTML = '<input id="findInput" type="search" placeholder="Find in file" aria-label="Find in file" autocomplete="off">' +
      '<span class="find-count" id="findCount">0 of 0</span>' +
      `<button type="button" class="icon-btn" id="findPrev" title="Previous match (Shift+F3)" aria-label="Previous match">${svg("chevron")}</button>` +
      `<button type="button" class="icon-btn" id="findNext" title="Next match (F3)" aria-label="Next match">${svg("arrow")}</button>` +
      `<button type="button" class="icon-btn" id="findClose" title="Close (Esc)" aria-label="Close find">${svg("close")}</button>`;
    view.insertBefore(bar, toolbar.nextSibling);
    $("findPrev").addEventListener("click", () => jumpMatch(-1));
    $("findNext").addEventListener("click", () => jumpMatch(1));
    $("findClose").addEventListener("click", closeFind);
    $("findInput").addEventListener("input", () => runFind(true));
    $("findInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); jumpMatch(event.shiftKey ? -1 : 1); }
      if (event.key === "Escape") { event.preventDefault(); closeFind(); }
    });
  }

  function buildPalette() {
    const palette = el("div", "palette hidden");
    palette.id = "palette";
    palette.innerHTML = '<div class="palette-card" role="dialog" aria-modal="true" aria-label="Command palette">' +
      '<input id="paletteInput" type="text" placeholder="Type a command…" aria-label="Type a command" autocomplete="off">' +
      '<div class="palette-list" id="paletteList"></div></div>';
    document.body.appendChild(palette);
    palette.addEventListener("click", (event) => { if (event.target === palette) closePalette(); });
    $("paletteInput").addEventListener("input", () => renderPalette());
    $("paletteInput").addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown") { event.preventDefault(); movePalette(1); }
      else if (event.key === "ArrowUp") { event.preventDefault(); movePalette(-1); }
      else if (event.key === "Enter") { event.preventDefault(); runPaletteCommand(); }
      else if (event.key === "Escape") { event.preventDefault(); closePalette(); }
    });
  }
  async function runSearch(query) {
    const results = $("searchResults"), meta = $("searchMeta");
    if (!state.workspace) { meta.textContent = "Open a folder first — search only reads inside it."; results.innerHTML = ""; return; }
    if (query.length < 2) { meta.textContent = "Type at least two characters."; results.innerHTML = ""; return; }
    const seq = ++IDE.searchSeq;
    meta.textContent = "Searching…";
    const result = await api(`/api/workspace/search?q=${encodeURIComponent(query)}`);
    if (seq !== IDE.searchSeq) return;
    if (!result.ok) { meta.textContent = result.data.error || "Search failed."; results.innerHTML = ""; return; }
    const matches = result.data.matches || [];
    const files = new Set(matches.map((m) => m.path)).size;
    meta.textContent = `${matches.length} match${matches.length === 1 ? "" : "es"} in ${files} file${files === 1 ? "" : "s"} · ${result.data.scanned} scanned${result.data.truncated ? " (truncated)" : ""}`;
    results.innerHTML = "";
    if (!matches.length) { results.innerHTML = '<p class="empty-note">No matches.</p>'; return; }
    const groups = new Map();
    matches.forEach((m) => { if (!groups.has(m.path)) groups.set(m.path, []); groups.get(m.path).push(m); });
    groups.forEach((hits, path) => {
      const group = el("div", "search-file");
      const head = el("button", "search-file-head", `${svg("file")}<span>${escapeHtml(basename(path))}</span><span class="hits">${hits.length}</span>`);
      head.type = "button"; head.title = path;
      head.addEventListener("click", () => void revealHit(hits[0]));
      group.appendChild(head);
      hits.slice(0, 20).forEach((hit) => {
        const row = el("button", "search-hit", `<span class="ln">${hit.line}</span><span class="snippet">${escapeHtml(hit.text)}</span>`);
        row.type = "button";
        row.addEventListener("click", () => void revealHit(hit));
        group.appendChild(row);
      });
      results.appendChild(group);
    });
  }

  async function revealHit(hit) {
    await openFile(hit.path);
    focusLine(hit.line, hit.column);
  }

  async function refreshScm() {
    const box = $("scmBody");
    if (!box) return;
    const commitBox = $("scmCommitBox");
    const branchBar = $("scmBranchBar");
    const syncBar = $("scmSync");
    IDE.diffKey = null;
    IDE.scmHistory = false;
    if (!state.workspace) {
      box.innerHTML = '<p class="empty-note">Open a folder to check its Git state.</p>';
      setBranchStatus(null);
      if (commitBox) commitBox.classList.add("hidden");
      if (branchBar) branchBar.classList.add("hidden");
      if (syncBar) syncBar.classList.add("hidden");
      return;
    }
    box.innerHTML = '<p class="empty-note">Reading git status…</p>';
    const result = await api("/api/workspace/vcs");
    if (!result.ok) {
      box.innerHTML = `<p class="empty-note">${escapeHtml(result.data.error || "Could not read git status.")}</p>`;
      return;
    }
    IDE.vcs = result.data;
    if (!result.data.available) {
      box.innerHTML = `<p class="empty-note">${escapeHtml(result.data.message || "This folder is not a Git repository.")}</p>`;
      setBranchStatus(null);
      if (commitBox) commitBox.classList.add("hidden");
      if (branchBar) branchBar.classList.add("hidden");
      if (syncBar) syncBar.classList.add("hidden");
      return;
    }
    if (commitBox) commitBox.classList.remove("hidden");
    if (branchBar) branchBar.classList.remove("hidden");
    if (syncBar) syncBar.classList.remove("hidden");
    if ($("scmBranchName")) $("scmBranchName").textContent = result.data.branch || "detached";
    if ($("scmBranchMeta")) {
      const ahead = Number(result.data.ahead || 0);
      const behind = Number(result.data.behind || 0);
      $("scmBranchMeta").textContent = `${ahead ? `↑${ahead}` : ""}${behind ? ` ↓${behind}` : ""}`.trim();
    }
    setBranchStatus(result.data.branch);
    const changes = result.data.changes || [];
    if (!changes.length) {
      box.innerHTML = '<p class="empty-note">Working tree clean.</p>';
      setScmHint("");
      return;
    }
    box.innerHTML = "";
    const conflicted = changes.filter((c) => c.conflicted);
    const staged = changes.filter((c) => c.staged && !c.conflicted);
    const untracked = changes.filter((c) => !c.staged && !c.conflicted && c.untracked);
    const unstaged = changes.filter((c) => !c.staged && !c.conflicted && !c.untracked);
    appendScmGroup(box, "Merge conflicts", conflicted, "conflict");
    appendScmGroup(box, "Staged", staged, "staged");
    appendScmGroup(box, "Changes", unstaged, "work");
    appendScmGroup(box, "Untracked", untracked, "work");
    setScmHint(staged.length ? `${staged.length} staged · commit when ready` : "");
  }

  function setScmHint(text) {
    if ($("scmHint")) $("scmHint").textContent = text;
  }

  function setScmRemoteState(text) {
    if ($("scmSyncState")) $("scmSyncState").textContent = text;
  }

  /* Fetch, pull and push are the only SCM operations that leave this machine.
     Pull is fast-forward only on the server side, so the confirm says what it
     will not do: merge, rebase, or overwrite local commits. */
  async function gitRemote(kind) {
    const labels = { fetch: "Fetch", pull: "Pull", push: "Push" };
    const label = labels[kind] || kind;
    if (!IDE.vcs || !IDE.vcs.available) { toast("Open a Git repository first."); return; }
    if (kind === "pull" && !window.confirm("Pull the current branch?\n\nOnly a fast-forward is applied; local commits are never merged or overwritten.")) return;
    const buttons = ["scmFetch", "scmPull", "scmPush"].map((id) => $(id)).filter(Boolean);
    buttons.forEach((button) => { button.disabled = true; });
    setScmRemoteState(`${label}…`);
    try {
      const result = await api(`/api/workspace/git/${kind}`, {
        method: "POST",
        body: JSON.stringify({}),
        confirmApproval: true,
      });
      if (!result.ok) {
        const message = result.data.error || `${label} failed.`;
        setScmRemoteState(message);
        toast(message);
        return;
      }
      const data = result.data;
      const where = data.upstream || data.remote || "";
      const message = kind === "fetch" ? `Fetched ${data.remote}`
        : kind === "pull" ? `Pulled ${data.branch}${where ? ` from ${where}` : ""}`
        : `Pushed ${data.branch}${where ? ` to ${where}` : ""}`;
      toast(message);
      const summary = (data.summary || []).filter((line) => !line.startsWith("From "));
      setScmRemoteState(summary.length ? summary[summary.length - 1] : message);
      await refreshScm();
      if (kind === "pull") setScmRemoteState(`${message} — reload an open file to see the changes`);
    } finally {
      buttons.forEach((button) => { button.disabled = false; });
    }
  }

  function scmAction(glyph, title, run) {
    const button = el("button", "scm-act", escapeHtml(glyph));
    button.type = "button";
    button.title = title;
    button.setAttribute("aria-label", title);
    button.addEventListener("click", (event) => { event.stopPropagation(); void run(); });
    return button;
  }

  function appendScmGroup(box, label, changes, group) {
    if (!changes.length) return;
    box.appendChild(el("div", "scm-group-label", `${label} <span class="count">${changes.length}</span>`));
    changes.forEach((change) => {
      const code = change.code === "?" ? "U" : change.code.replace(/\./g, "") || "M";
      const cls = change.conflicted ? " conflict"
        : code === "D" ? " del"
        : (code === "?" || code === "A" || change.untracked) ? " add" : "";
      const row = el("div", "scm-item");
      const open = el("button", "scm-open", `<span class="code${cls}">${escapeHtml(code)}</span><span class="path">${escapeHtml(change.path)}</span>`);
      open.type = "button";
      open.title = `${change.path} — show its diff`;
      open.addEventListener("click", () => { void toggleScmDiff(change, row); });
      row.appendChild(open);
      const actions = el("div", "scm-actions");
      if (change.staged) {
        actions.appendChild(scmAction("−", `Unstage ${change.path}`, () => scmMutate("/api/workspace/git/unstage", { paths: [change.path] }, `Unstaged ${change.path}`)));
      } else {
        actions.appendChild(scmAction("+", `Stage ${change.path}`, () => scmMutate("/api/workspace/git/stage", { paths: [change.path] }, `Staged ${change.path}`)));
      }
      actions.appendChild(scmAction("↺", `Discard changes to ${change.path}`, async () => {
        if (!window.confirm(`Discard the changes to ${change.path}?\n\nThis cannot be undone.`)) return;
        await scmMutate("/api/workspace/git/discard", { paths: [change.path] }, `Discarded ${change.path}`);
      }));
      row.appendChild(actions);
      box.appendChild(row);
    });
  }

  /* Remotes are repository configuration, not history: adding or removing one
     never touches a file or a commit, so the confirm only says what goes away. */
  async function showRemotes() {
    const form = $("scmRemoteForm");
    const list = $("scmRemoteList");
    if (!form || !list) return;
    form.classList.toggle("hidden");
    list.classList.toggle("hidden");
    if (form.classList.contains("hidden")) { setRemoteEditing(""); return; }
    $("scmRemoteName").focus();
    await loadRemotes();
  }

  async function loadRemotes() {
    const list = $("scmRemoteList");
    if (!list) return;
    if (!state.workspace) {
      list.innerHTML = '<p class="empty-note">Open a folder first.</p>';
      return;
    }
    list.innerHTML = '<p class="empty-note">Reading remotes…</p>';
    const result = await api("/api/workspace/git/remotes");
    if (!result.ok) {
      list.innerHTML = `<p class="empty-note">${escapeHtml(result.data.error || "Could not read the remotes.")}</p>`;
      return;
    }
    const remotes = Array.isArray(result.data.remotes) ? result.data.remotes : [];
    if (!remotes.length) {
      list.innerHTML = '<p class="empty-note">No remotes yet. Add one above.</p>';
      return;
    }
    list.innerHTML = "";
    remotes.forEach((remote) => {
      const row = el("div", "scm-remote-row");
      row.appendChild(el("span", "scm-remote-name", remote.name || ""));
      const url = el("span", "scm-remote-url", remote.url || "");
      if (remote.pushUrl && remote.pushUrl !== remote.url) {
        url.title = `Fetches from ${remote.url}\nPushes to ${remote.pushUrl}`;
      }
      row.appendChild(url);
      const edit = el("button", "tiny-toggle", "Edit");
      edit.type = "button";
      edit.title = `Change the URL of ${remote.name}`;
      edit.addEventListener("click", () => {
        $("scmRemoteName").value = remote.name || "";
        $("scmRemoteUrl").value = remote.url || "";
        setRemoteEditing(remote.name || "");
        $("scmRemoteUrl").focus();
      });
      row.appendChild(edit);
      const remove = el("button", "tiny-toggle", "Remove");
      remove.type = "button";
      remove.title = `Remove the remote ${remote.name}`;
      remove.addEventListener("click", () => void removeRemote(remote.name));
      row.appendChild(remove);
      list.appendChild(row);
    });
  }

  /* Editing fills the same two fields, so one button covers both: while a row's
     name still matches the edited remote the form saves its URL instead. */
  function setRemoteEditing(name) {
    IDE.remoteEditing = name || "";
    const button = $("scmRemoteAdd");
    if (!button) return;
    button.textContent = IDE.remoteEditing ? "Save URL" : "Add";
    button.title = IDE.remoteEditing ? `Save the new URL for ${IDE.remoteEditing}` : "Add this remote";
  }

  async function addRemote() {
    const name = ($("scmRemoteName").value || "").trim();
    const url = ($("scmRemoteUrl").value || "").trim();
    if (!name) { toast("Name the remote first."); return; }
    if (!url) { toast("Paste the remote URL."); return; }
    const editing = Boolean(IDE.remoteEditing) && IDE.remoteEditing === name;
    const result = await api(
      editing ? "/api/workspace/git/remotes/set-url" : "/api/workspace/git/remotes/add",
      {
        method: "POST",
        body: JSON.stringify({ name, url, approve: true }),
        confirmApproval: true,
      },
    );
    if (!result.ok) {
      toast(result.data.error || (editing ? "Could not change the remote URL." : "Could not add the remote."));
      return;
    }
    $("scmRemoteName").value = "";
    $("scmRemoteUrl").value = "";
    setRemoteEditing("");
    toast(editing ? `Changed ${name} to ${url}` : `Added ${name}`);
    await loadRemotes();
    await refreshScm();
  }

  async function removeRemote(name) {
    if (!name) return;
    if (!window.confirm(`Remove the remote "${name}"?\n\nNo file or commit is touched; only the saved URL goes away.`)) return;
    const result = await api("/api/workspace/git/remotes/remove", {
      method: "POST",
      body: JSON.stringify({ name, approve: true }),
      confirmApproval: true,
    });
    if (!result.ok) {
      toast(result.data.error || "Could not remove the remote.");
      return;
    }
    if (IDE.remoteEditing === name) {
      $("scmRemoteName").value = "";
      $("scmRemoteUrl").value = "";
      setRemoteEditing("");
    }
    toast(`Removed ${name}`);
    await loadRemotes();
    await refreshScm();
  }

  async function scmMutate(path, body, successMessage) {
    const result = await api(path, { method: "POST", body: JSON.stringify(body), confirmApproval: true });
    if (!result.ok) {
      toast(result.data.error || "Git operation failed.");
      return;
    }
    toast(successMessage);
    await refreshScm();
  }

  async function mergeScm() {
    const input = $("scmMergeInput");
    const branch = input ? input.value.trim() : "";
    if (!branch) { toast("Name the branch to merge in."); return; }
    if (!window.confirm(`Merge "${branch}" into the current branch?\n\nThe working tree must be clean. A conflict is left in place for you to resolve, and Abort merge puts it back.`)) return;
    const result = await api("/api/workspace/git/merge", {
      method: "POST",
      body: JSON.stringify({ branch, approve: true }),
      confirmApproval: true,
    });
    await refreshScm();
    if (input) input.value = "";
    if (!result.ok) {
      const conflicted = Array.isArray(result.data && result.data.conflicted) ? result.data.conflicted : [];
      toast(conflicted.length
        ? `Merge stopped: ${conflicted.length} conflicted file(s) — ${conflicted.slice(0, 3).join(", ")}${conflicted.length > 3 ? "…" : ""}`
        : (result.data.error || "Merge failed."));
      return;
    }
    const data = result.data || {};
    $("scmMergeForm").classList.add("hidden");
    toast(`Merged ${data.branch} into ${data.current}${data.fastForward ? " (fast-forward)" : ""}`);
  }

  async function abortMergeScm() {
    if (!window.confirm("Abort the merge in progress and put the branch back as it was?")) return;
    const result = await api("/api/workspace/git/merge/abort", {
      method: "POST",
      body: JSON.stringify({ approve: true }),
      confirmApproval: true,
    });
    await refreshScm();
    toast(result.ok ? "Merge aborted." : (result.data.error || "Could not abort the merge."));
  }

  async function rebaseScm() {
    const input = $("scmRebaseInput");
    const branch = input ? input.value.trim() : "";
    if (!branch) { toast("Name the branch to rebase onto."); return; }
    if (!window.confirm(`Replay this branch's commits on top of "${branch}"?\n\nThis rewrites local history. The working tree must be clean, a conflict is left in place for you to resolve, and Abort rebase puts the branch back.`)) return;
    const result = await api("/api/workspace/git/rebase", {
      method: "POST",
      body: JSON.stringify({ branch, approve: true }),
      confirmApproval: true,
    });
    await refreshScm();
    if (input) input.value = "";
    if (!result.ok) {
      const conflicted = Array.isArray(result.data && result.data.conflicted) ? result.data.conflicted : [];
      toast(conflicted.length
        ? `Rebase stopped: ${conflicted.length} conflicted file(s) — ${conflicted.slice(0, 3).join(", ")}${conflicted.length > 3 ? "…" : ""}`
        : (result.data.error || "Rebase failed."));
      return;
    }
    const data = result.data || {};
    $("scmRebaseForm").classList.add("hidden");
    toast(`Rebased ${data.current} onto ${data.branch}`);
  }

  async function abortRebaseScm() {
    if (!window.confirm("Abort the rebase in progress and put the branch back where it started?")) return;
    const result = await api("/api/workspace/git/rebase/abort", {
      method: "POST",
      body: JSON.stringify({ approve: true }),
      confirmApproval: true,
    });
    await refreshScm();
    toast(result.ok ? "Rebase aborted." : (result.data.error || "Could not abort the rebase."));
  }

  async function continueRebaseScm() {
    if (!window.confirm("Continue the rebase with the conflicts you resolved and staged?")) return;
    const result = await api("/api/workspace/git/rebase/continue", {
      method: "POST",
      body: JSON.stringify({ approve: true }),
      confirmApproval: true,
    });
    await refreshScm();
    if (!result.ok) {
      toast(result.data.error || "Could not continue the rebase.");
      return;
    }
    const data = result.data || {};
    $("scmRebaseForm").classList.add("hidden");
    toast(`Rebase finished on ${data.branch}`);
  }

  /// A changed line inside a hunk is clickable: selecting it builds the index
  /// list the server turns into a patch of just those lines.
  function scmHunkLineHtml(line, changeIndex) {
    const cls = line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "";
    if (!cls) return scmDiffLineHtml(line);
    return `<span class="${cls} pickable" data-change="${changeIndex}" title="Click, or focus and press Space, to select this line">${escapeHtml(line)}</span>`;
  }

  function toggleScmLine(span) {
    const selected = span.classList.toggle("selected");
    span.setAttribute("aria-pressed", String(selected));
  }

  function setScmHunkSelection(picks, selected) {
    picks.forEach((span) => {
      span.classList.toggle("selected", selected);
      span.setAttribute("aria-pressed", String(selected));
    });
  }

  async function scmHunkLines(change, index, lines) {
    const endpoint = change.staged ? "/api/workspace/git/unstage-hunk" : "/api/workspace/git/stage-hunk";
    const result = await api(endpoint, {
      method: "POST",
      body: JSON.stringify({ path: change.path, hunk: index, lines }),
      confirmApproval: true,
    });
    if (!result.ok) {
      toast(result.data.error || "Could not move those lines.");
      return;
    }
    const moved = result.data.lines || lines.length;
    toast(`${moved} line${moved === 1 ? "" : "s"} ${change.staged ? "unstaged" : "staged"} in ${change.path}`);
    await refreshScm();
  }

  function scmDiffLineHtml(line) {
    const cls = line.startsWith("@@") ? "hunk"
      : line.startsWith("+++") || line.startsWith("---") || line.startsWith("diff ") || line.startsWith("index ") || line.startsWith("new file") ? "meta"
      : line.startsWith("+") ? "add"
      : line.startsWith("-") ? "del" : "";
    return `<span class="${cls}">${escapeHtml(line)}</span>`;
  }

  function splitScmDiff(text) {
    const preamble = [];
    const hunks = [];
    let current = null;
    text.split("\n").forEach((line) => {
      if (line.startsWith("@@ ")) {
        if (current) hunks.push(current);
        current = [line];
      } else if (current) {
        current.push(line);
      } else {
        preamble.push(line);
      }
    });
    if (current) hunks.push(current);
    return { preamble, hunks };
  }

  function renderScmDiff(box, text, change, truncated) {
    const { preamble, hunks } = splitScmDiff(text);
    if (!hunks.length) {
      const body = el("pre", "scm-diff-body");
      body.innerHTML = text.split("\n").slice(0, 400).map(scmDiffLineHtml).join("\n") +
        (truncated ? '\n<span class="meta">… diff truncated …</span>' : "");
      box.appendChild(body);
      return;
    }
    if (preamble.length) {
      const head = el("pre", "scm-diff-body");
      head.innerHTML = preamble.filter((line) => line !== "").map(scmDiffLineHtml).join("\n");
      box.appendChild(head);
    }
    hunks.forEach((lines, index) => {
      const hunk = el("div", "scm-hunk");
      const head = el("div", "scm-hunk-head");
      head.appendChild(el("span", "hunk-index", `Hunk ${index + 1}`));
      const action = el("button", "tiny-toggle", change.staged ? "Unstage hunk" : "Stage hunk");
      action.type = "button";
      action.title = `${change.staged ? "Unstage" : "Stage"} only this hunk of ${change.path}`;
      action.addEventListener("click", () => { void scmHunk(change, index); });
      head.appendChild(action);
      hunk.appendChild(head);
      const body = el("pre", "scm-hunk-body");
      let changeIndex = 0;
      body.innerHTML = lines.map((line) => {
        if (line.startsWith("+") || line.startsWith("-")) {
          const html = scmHunkLineHtml(line, changeIndex);
          changeIndex += 1;
          return html;
        }
        return scmDiffLineHtml(line);
      }).join("\n");
      // Changed lines are focus targets: the hunk is one tab stop, arrows move
      // between the lines, and Space or Enter toggles the one under the caret.
      const picks = Array.from(body.querySelectorAll("span.pickable"));
      picks.forEach((span, index) => {
        span.tabIndex = index === 0 ? 0 : -1;
        span.setAttribute("role", "button");
        span.setAttribute("aria-pressed", "false");
      });
      // A polite live region: the count is the only thing that says how much of
      // the hunk is picked, and the outline alone cannot be heard.
      const count = el("span", "scm-hunk-count", `0 of ${picks.length} selected`);
      count.setAttribute("aria-live", "polite");
      const refreshCount = () => {
        const picked = picks.filter((span) => span.classList.contains("selected")).length;
        count.textContent = `${picked} of ${picks.length} selected`;
      };
      const focusPick = (index) => {
        if (!picks.length) return;
        const at = Math.max(0, Math.min(picks.length - 1, index));
        picks.forEach((span, i) => { span.tabIndex = i === at ? 0 : -1; });
        picks[at].focus();
      };
      body.addEventListener("click", (event) => {
        const line = event.target.closest("span.pickable");
        if (!line) return;
        toggleScmLine(line);
        refreshCount();
        focusPick(picks.indexOf(line));
      });
      body.addEventListener("keydown", (event) => {
        const line = event.target.closest("span.pickable");
        if (!line) return;
        if (event.key === " " || event.key === "Enter") {
          event.preventDefault();
          toggleScmLine(line);
          refreshCount();
          return;
        }
        // A selects every changed line of this hunk, Escape clears it, so a
        // whole hunk moves without clicking each line.
        if (event.key === "a" || event.key === "A" || event.key === "Escape") {
          event.preventDefault();
          setScmHunkSelection(picks, event.key !== "Escape");
          refreshCount();
          return;
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          focusPick(picks.indexOf(line) + (event.key === "ArrowDown" ? 1 : -1));
          return;
        }
        if (event.key === "Home" || event.key === "End") {
          event.preventDefault();
          focusPick(event.key === "Home" ? 0 : picks.length - 1);
        }
      });
      hunk.appendChild(body);
      const picked = el("button", "tiny-toggle", change.staged ? "Unstage selected" : "Stage selected");
      picked.type = "button";
      picked.title = `Move only the lines you selected out of this hunk of ${change.path}`;
      picked.addEventListener("click", () => {
        const selected = Array.from(body.querySelectorAll("span.pickable.selected"))
          .map((span) => Number(span.dataset.change));
        if (!selected.length) { toast("Click the changed lines you want to move first."); return; }
        void scmHunkLines(change, index, selected);
      });
      head.appendChild(picked);
      const all = el("button", "tiny-toggle", "All");
      all.type = "button";
      all.title = "Select every changed line in this hunk (or press A)";
      all.addEventListener("click", () => { setScmHunkSelection(picks, true); refreshCount(); });
      head.appendChild(all);
      const none = el("button", "tiny-toggle", "Clear");
      none.type = "button";
      none.title = "Clear this hunk's selection (or press Escape)";
      none.addEventListener("click", () => { setScmHunkSelection(picks, false); refreshCount(); });
      head.appendChild(none);
      head.appendChild(count);
      box.appendChild(hunk);
    });
    if (truncated) box.insertAdjacentHTML("beforeend", '<p class="scm-hint">… diff truncated; only the hunks above are shown …</p>');
  }

  async function toggleScmDiff(change, row) {
    const key = `${change.staged ? "staged:" : "work:"}${change.path}`;
    const open = document.querySelector(".scm-diff");
    if (IDE.diffKey === key) { IDE.diffKey = null; if (open) open.remove(); return; }
    if (open) open.remove();
    IDE.diffKey = key;
    const query = `/api/workspace/git/diff?path=${encodeURIComponent(change.path)}${change.staged ? "&staged=1" : ""}`;
    const result = await api(query);
    if (!result.ok) {
      IDE.diffKey = null;
      toast(result.data.error || "Could not read the diff.");
      return;
    }
    const box = el("div", "scm-diff");
    const text = String(result.data.diff || "");
    if (!text.trim()) {
      box.innerHTML = `<span class="meta">No textual changes to show for ${escapeHtml(change.path)}.</span>`;
    } else {
      renderScmDiff(box, text, change, Boolean(result.data.truncated));
    }
    row.insertAdjacentElement("afterend", box);
  }

  async function scmHunk(change, index) {
    const path = change.staged ? "/api/workspace/git/unstage-hunk" : "/api/workspace/git/stage-hunk";
    const result = await api(path, {
      method: "POST",
      body: JSON.stringify({ path: change.path, hunk: index }),
      confirmApproval: true,
    });
    if (!result.ok) {
      toast(result.data.error || "Could not move that hunk.");
      return;
    }
    toast(`${change.staged ? "Unstaged" : "Staged"} hunk ${index + 1} of ${change.path}`);
    await refreshScm();
  }

  async function commitScm() {
    const message = $("scmMessage").value.trim();
    if (!message) { setScmHint("Write a commit message first."); $("scmMessage").focus(); return; }
    const button = $("scmCommit");
    button.disabled = true;
    try {
      const result = await api("/api/workspace/git/commit", {
        method: "POST",
        body: JSON.stringify({ message }),
        confirmApproval: true,
      });
      if (!result.ok) {
        setScmHint(result.data.error || "Commit failed.");
        toast(result.data.error || "Commit failed.");
        return;
      }
      $("scmMessage").value = "";
      toast(`Committed ${result.data.commit || ""}`.trim());
      await refreshScm();
      setScmHint(`Committed ${result.data.commit || ""}`.trim());
    } finally {
      button.disabled = false;
    }
  }

  async function toggleScmHistory() {
    const box = $("scmBody");
    if (!box) return;
    if (IDE.scmHistory) { await refreshScm(); return; }
    const result = await api("/api/workspace/git/log?limit=20");
    if (!result.ok) { toast(result.data.error || "Could not read the history."); return; }
    const commits = result.data.commits || [];
    IDE.scmHistory = true;
    IDE.diffKey = null;
    box.innerHTML = "";
    if (!commits.length) {
      box.innerHTML = '<p class="empty-note">No commits yet.</p>';
      return;
    }
    box.appendChild(el("div", "scm-group-label", `Recent commits <span class="count">${commits.length}</span>`));
    box.appendChild(el("p", "scm-hint", "Select a commit to read its diff."));
    commits.forEach((commit) => {
      const row = el("div", "scm-commit-line");
      row.innerHTML = `<span class="short">${escapeHtml(commit.short || "")}</span>` +
        `<span class="subject">${escapeHtml(commit.subject || "")}</span>` +
        `<span class="author">${escapeHtml(commit.author || "")}</span>`;
      row.title = `${commit.hash || ""}\n${commit.author || ""} · ${commit.date || ""}\nOpen this commit's diff`;
      row.tabIndex = 0;
      row.setAttribute("role", "button");
      row.addEventListener("click", () => { void showScmCommit(commit); });
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") { event.preventDefault(); void showScmCommit(commit); }
      });
      box.appendChild(row);
    });
  }

  async function showScmCommit(commit) {
    const result = await api(`/api/workspace/git/show?hash=${encodeURIComponent(commit.hash || "")}`);
    if (!result.ok) { toast(result.data.error || "Could not read that commit."); return; }
    const data = result.data;
    const box = $("scmBody");
    if (!box) return;
    IDE.scmHistory = true;
    IDE.diffKey = null;
    box.innerHTML = "";
    box.appendChild(el("div", "scm-group-label", `Commit ${escapeHtml(data.short || "")}`));
    box.appendChild(el("div", "scm-commit-meta", `${data.subject || ""}\n${data.author || ""} · ${data.date || ""}`));
    const diff = el("pre", "scm-diff");
    const text = String(data.diff || "");
    if (!text.trim()) {
      diff.innerHTML = '<span class="meta">This commit has no textual diff against its parent.</span>';
    } else {
      diff.innerHTML = text.split("\n").map(scmDiffLineHtml).join("\n") +
        (data.truncated ? '\n<span class="meta">… diff truncated …</span>' : "");
    }
    box.appendChild(diff);
    const back = el("button", "tiny-toggle", "Back to history");
    back.type = "button";
    back.addEventListener("click", () => void toggleScmHistory());
    box.appendChild(back);
  }

  async function branchScm(mode) {
    const name = $("scmBranchInput").value.trim();
    if (!name) { toast("Name the branch first."); $("scmBranchInput").focus(); return; }
    const path = mode === "create" ? "/api/workspace/git/branch" : "/api/workspace/git/checkout";
    const result = await api(path, { method: "POST", body: JSON.stringify({ name }), confirmApproval: true });
    if (!result.ok) { toast(result.data.error || "Branch operation failed."); return; }
    $("scmBranchInput").value = "";
    $("scmBranchForm").classList.add("hidden");
    toast(`${mode === "create" ? "Created" : "Switched to"} ${name}`);
    await refreshScm();
  }

  function setBranchStatus(branch) {
    const wrap = $("statusBranchBtn"), label = $("statusBranch");
    if (!wrap || !label) return;
    wrap.classList.toggle("hidden", !branch);
    if (branch) label.textContent = branch;
  }
  function recordProblem(where, message) {
    IDE.problems.unshift({ where: where || "", message: message || "Request failed.", at: Date.now() });
    if (IDE.problems.length > 100) IDE.problems.length = 100;
    updateProblemCounts();
    if (IDE.bottomTab === "problems") renderProblems();
  }

  function renderProblems() {
    const box = $("problemsBody");
    if (!box) return;
    if (!IDE.problems.length) {
      box.innerHTML = '<p class="pane-empty">No problems. Failed saves and requests, and build or test diagnostics, land here.</p>';
      return;
    }
    box.innerHTML = "";
    IDE.problems.forEach((problem) => {
      const severity = problem.severity || "";
      const where = problem.where || "app";
      const at = problem.line ? `:${problem.line}${problem.column ? `:${problem.column}` : ""}` : "";
      const glyph = severity === "warning" ? "!" : severity ? "×" : "!";
      const row = el("button", "problem-row",
        `<span class="sev ${escapeHtml(severity)}">${glyph}</span>` +
        `<span class="where">${escapeHtml(where)}${escapeHtml(at)}</span>` +
        `<span class="what">${escapeHtml(problem.message)}</span>`);
      row.type = "button";
      if (problem.where) {
        row.addEventListener("click", () => {
          void (async () => {
            await openFile(problem.where);
            if (problem.line) focusLine(problem.line, problem.column || 1);
          })();
        });
      }
      box.appendChild(row);
    });
  }

  function setRunnerStatus(text) {
    if ($("runnerStatus")) $("runnerStatus").textContent = text;
  }

  function updateRunnerButtons() {
    [["runBuild", "build"], ["runTest", "test"]].forEach(([id, kind]) => {
      const button = $(id);
      if (!button) return;
      const preset = IDE.runner.presets.find((entry) => entry.kind === kind);
      button.disabled = IDE.runner.running || !preset;
      button.title = preset ? `Run ${preset.command}` : `No ${kind} command detected in this folder`;
    });
  }

  async function loadRunnerPresets() {
    if (!state.workspace) {
      IDE.runner = { presets: [], running: false };
      updateRunnerButtons();
      return;
    }
    const result = await api("/api/workspace/runner");
    if (!result.ok) {
      setRunnerStatus(result.data.error || "Could not detect build commands.");
      return;
    }
    IDE.runner = { presets: result.data.presets || [], running: Boolean(result.data.running) };
    updateRunnerButtons();
    const shown = $("runnerStatus") ? $("runnerStatus").textContent : "";
    const untouched = !shown || shown === "No build run yet" || shown.startsWith("No build or test command");
    if (!IDE.runner.presets.length) {
      setRunnerStatus("No build or test command detected. Add Cargo.toml or a package.json script.");
    } else if (!IDE.runner.running && untouched) {
      setRunnerStatus(IDE.runner.presets.map((preset) => preset.label).join(" · "));
    }
  }

  function showRunnerOutput(data) {
    const pre = $("runnerOutput");
    if (!pre) return;
    const text = String(data.output || "");
    pre.classList.remove("hidden");
    pre.textContent = text
      ? (data.outputTruncated ? "… output truncated; the tail is shown …\n" + text : text)
      : "(the command produced no output)";
    pre.scrollTop = 0;
  }

  async function runBuild(kind) {
    const preset = IDE.runner.presets.find((entry) => entry.kind === kind);
    if (!preset) { toast(`No ${kind} command detected in this folder.`); return; }
    if (IDE.runner.running) { toast("A build or test is already running."); return; }
    IDE.runner.running = true;
    updateRunnerButtons();
    setRunnerStatus(`${preset.command} …`);
    const started = performance.now();
    try {
      const result = await api("/api/workspace/run", {
        method: "POST",
        body: JSON.stringify({ id: preset.id }),
        confirmApproval: true,
      });
      if (!result.ok) {
        setRunnerStatus(result.data.error || `${preset.command} could not start.`);
        toast(result.data.error || "Could not run that command.");
        return;
      }
      const data = result.data;
      const problems = data.problems || [];
      IDE.problems = problems.map((problem) => ({
        where: problem.path,
        line: problem.line,
        column: problem.column,
        severity: problem.severity,
        message: problem.message,
        at: Date.now(),
      }));
      renderProblems();
      updateProblemCounts();
      showRunnerOutput(data);
      const seconds = ((data.durationMs || Math.round(performance.now() - started)) / 1000).toFixed(1);
      const count = `${problems.length} problem${problems.length === 1 ? "" : "s"}`;
      setRunnerStatus(`${data.command} · exit ${data.exitCode}${data.timedOut ? " (timed out)" : ""} · ${count} · ${seconds}s`);
      // Test counts come from the runner's own parse of the tool's summary. No
      // summary line means nothing is shown, never zeroes.
      const tests = data.tests || null;
      const testsBox = $("runnerTests");
      if (testsBox) {
        if (tests && (tests.total || tests.failed)) {
          const facts = [`${tests.passed} passed`, `${tests.failed} failed`];
          if (tests.ignored) facts.push(`${tests.ignored} ignored`);
          testsBox.textContent = ` · ${facts.join(" · ")}`;
          testsBox.title = tests.failures && tests.failures.length
            ? `Failed: ${tests.failures.join(", ")}`
            : `${tests.framework || "test"} summary`;
        } else {
          testsBox.textContent = "";
          testsBox.title = "";
        }
      }
      toast(data.success ? `${data.command} passed` : `${data.command} failed`);
    } finally {
      IDE.runner.running = false;
      updateRunnerButtons();
    }
  }

  function updateProblemCounts() {
    const count = IDE.problems.length;
    if ($("problemCount")) {
      $("problemCount").textContent = String(count);
      $("problemCount").classList.toggle("bad", count > 0);
    }
    if ($("statusProblems")) $("statusProblems").textContent = String(count);
  }

  function recordOutput(text, ok = true) {
    IDE.output.unshift({ stamp: new Date().toLocaleTimeString(), text, ok });
    if (IDE.output.length > 200) IDE.output.length = 200;
    if (IDE.bottomTab === "output") renderOutput();
  }

  function renderOutput() {
    const box = $("outputBody");
    if (!box) return;
    if (!IDE.output.length) { box.innerHTML = '<p class="pane-empty">No requests logged yet.</p>'; return; }
    box.innerHTML = "";
    IDE.output.forEach((line) => {
      box.appendChild(el("div", "output-line",
        `<span class="${line.ok ? "ok" : "bad"}">${escapeHtml(line.stamp)}</span> ${escapeHtml(line.text)}`));
    });
  }

  async function loadRunsPane() {
    const box = $("runsBody");
    if (!box) return;
    const result = await api("/api/runs");
    if (!result.ok) {
      box.innerHTML = `<p class="pane-empty">${escapeHtml(result.data.error || "Could not load runs.")}</p>`;
      return;
    }
    const runs = result.data.runs || [];
    if ($("runsCount")) $("runsCount").textContent = String(runs.length);
    if (!runs.length) {
      box.innerHTML = '<p class="pane-empty">No agent runs yet. Open Run in the activity bar to give the agent a goal for this folder.</p>';
      return;
    }
    box.innerHTML = "";
    runs.slice(0, 40).forEach((run) => {
      const state_ = String(run.status || "").replace(/_/g, " ");
      const row = el("button", "run-line",
        `<span class="state ${escapeHtml(run.status || "")}">${escapeHtml(state_)}</span>` +
        `<span class="goal">${escapeHtml(run.goal || "")}</span>`);
      row.type = "button";
      row.title = "Open the Runs view";
      row.addEventListener("click", () => setView("runs"));
      box.appendChild(row);
    });
  }

  async function loadTasksPane() {
    const box = $("tasksBody");
    if (!box) return;
    const result = await api("/api/workflows");
    if (!result.ok) {
      box.innerHTML = `<p class="pane-empty">${escapeHtml(result.data.error || "Could not load workflows.")}</p>`;
      return;
    }
    const workflows = result.data.workflows || [];
    if (!workflows.length) {
      box.innerHTML = '<p class="pane-empty">No saved workflows yet. Create one in Automation (the Auto button in the sidebar).</p>';
      return;
    }
    box.innerHTML = "";
    workflows.forEach((workflow) => {
      const row = el("button", "task-line",
        `<span class="state">task</span><span class="goal">${escapeHtml(workflow.name || workflow.id || "workflow")}</span>`);
      row.type = "button";
      row.addEventListener("click", () => setView("automation"));
      box.appendChild(row);
    });
  }

  function updateStatus() {
    if ($("statusProject")) {
      $("statusProject").textContent =
        (state.activeProject && state.activeProject.name) || (state.workspace && state.workspace.name) || "No project";
    }
    const model = state.model || "auto";
    if ($("statusModel")) $("statusModel").textContent = model;
    if ($("aiDockModel")) $("aiDockModel").textContent = model;
    const connText = ($("connText") && $("connText").textContent) || "";
    if ($("statusConn")) $("statusConn").textContent = connText.replace(/…/g, "").trim() || "router";
    if ($("statusConnDot")) {
      const ok = /ready|connected|online/i.test(connText);
      const bad = /fail|error|offline|unreachable/i.test(connText);
      $("statusConnDot").className = "dot" + (ok ? " ok" : bad ? " bad" : "");
    }
    const runs = Array.isArray(state.agentRuns) ? state.agentRuns : [];
    const active = runs.filter((run) => run.status === "running" || run.status === "awaiting_approval");
    if ($("statusRun")) $("statusRun").textContent = active.length ? `${active.length} running` : "idle";
    if ($("runsCount") && runs.length) $("runsCount").textContent = String(runs.length);
  }

  function setupSearchInput() {
    const input = $("searchInput");
    let timer = null;
    input.addEventListener("input", () => {
      clearTimeout(timer);
      timer = setTimeout(() => void runSearch(input.value.trim()), 250);
    });
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") { clearTimeout(timer); void runSearch(input.value.trim()); }
      if (event.key === "Escape") { input.value = ""; $("searchResults").replaceChildren(); }
    });
    $("scmRefresh").addEventListener("click", () => void refreshScm());
    $("scmFetch").addEventListener("click", () => void gitRemote("fetch"));
    $("scmPull").addEventListener("click", () => void gitRemote("pull"));
    $("scmPush").addEventListener("click", () => void gitRemote("push"));
    $("scmCommit").addEventListener("click", () => void commitScm());
    $("scmMessage").addEventListener("keydown", (event) => {
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void commitScm(); }
    });
    $("scmBranchBtn").addEventListener("click", () => {
      const form = $("scmBranchForm");
      form.classList.toggle("hidden");
      if (!form.classList.contains("hidden")) $("scmBranchInput").focus();
    });
    $("scmHistoryBtn").addEventListener("click", () => void toggleScmHistory());
    $("scmBranchCreate").addEventListener("click", () => void branchScm("create"));
    $("scmBranchSwitch").addEventListener("click", () => void branchScm("switch"));
    $("scmMergeBtn").addEventListener("click", () => {
      const form = $("scmMergeForm");
      form.classList.toggle("hidden");
      if (!form.classList.contains("hidden")) $("scmMergeInput").focus();
    });
    $("scmMergeGo").addEventListener("click", () => void mergeScm());
    $("scmMergeAbort").addEventListener("click", () => void abortMergeScm());
    $("scmMergeInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); void mergeScm(); }
    });
    $("scmRebaseBtn").addEventListener("click", () => {
      const form = $("scmRebaseForm");
      form.classList.toggle("hidden");
      if (!form.classList.contains("hidden")) $("scmRebaseInput").focus();
    });
    $("scmRebaseGo").addEventListener("click", () => void rebaseScm());
    $("scmRebaseAbort").addEventListener("click", () => void abortRebaseScm());
    $("scmRebaseContinue").addEventListener("click", () => void continueRebaseScm());
    $("scmRemotesBtn").addEventListener("click", () => void showRemotes());
    $("scmRemoteAdd").addEventListener("click", () => void addRemote());
    $("scmRemoteUrl").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); void addRemote(); }
    });
    $("scmRebaseInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); void rebaseScm(); }
    });
    $("scmBranchInput").addEventListener("keydown", (event) => {
      if (event.key === "Enter") { event.preventDefault(); void branchScm("switch"); }
    });
  }
  const KEYWORDS = {
    rust: "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while",
    js: "async await break case catch class const continue default delete do else export extends false finally for function if import in instanceof let new null of return static super switch this throw true try typeof var void while yield",
    py: "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield",
    sh: "case do done elif else esac fi for function if in local return then until while export",
    sql: "select from where insert update delete join on group by order limit as and or not null values into set",
    plain: "",
  };

  const FIND = { matches: [], index: 0 };
  const PATTERNS = new Map();

  function familyFor(language) {
    const lang = String(language || "").toLowerCase();
    if (/rust/.test(lang)) return "rust";
    if (/java|kotlin|gradle|typescript|javascript|tsx|jsx|node/.test(lang)) return "js";
    if (/python/.test(lang)) return "py";
    if (/shell|bash|dockerfile|makefile/.test(lang)) return "sh";
    if (/sql/.test(lang)) return "sql";
    return "plain";
  }

  function patternFor(family) {
    if (PATTERNS.has(family)) return PATTERNS.get(family);
    const comments = [];
    if (family === "py" || family === "sh") comments.push("#[^\n]*");
    else if (family !== "plain") comments.push("\/\*[\s\S]*?\*\/", "\/\/[^\n]*");
    const parts = [];
    if (comments.length) parts.push("(" + comments.join("|") + ")");
    else parts.push("(\u0000)");
    parts.push("(\"(?:[^\"\\\n]|\\.)*\"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`)");
    parts.push("(\b\d[\w.]*\b)");
    parts.push("([A-Za-z_$][\w$]*)(?=\s*\()");
    parts.push("([A-Za-z_$][\w$]*)");
    const pattern = new RegExp(parts.join("|"), "g");
    PATTERNS.set(family, pattern);
    return pattern;
  }

  function highlightHtml(code, language) {
    if (!code) return "";
    if (code.length > 200000) return escapeHtml(code);
    const family = familyFor(language);
    const keywords = new Set((KEYWORDS[family] || "").split(/\s+/).filter(Boolean));
    const pattern = patternFor(family);
    pattern.lastIndex = 0;
    let out = "";
    let last = 0;
    let match;
    while ((match = pattern.exec(code)) !== null) {
      if (!match[0].length) { pattern.lastIndex += 1; continue; }
      out += escapeHtml(code.slice(last, match.index));
      const text = match[0];
      let cls = "";
      if (match[1]) cls = "tok-com";
      else if (match[2]) cls = "tok-str";
      else if (match[3]) cls = "tok-num";
      else if (match[4]) cls = keywords.has(text) ? "tok-key" : "tok-fn";
      else if (keywords.has(text)) cls = "tok-key";
      else if (/^[A-Z][a-z]/.test(text)) cls = "tok-type";
      out += cls ? `<span class="${cls}">${escapeHtml(text)}</span>` : escapeHtml(text);
      last = match.index + text.length;
    }
    return out + escapeHtml(code.slice(last));
  }

  function syncLayerScroll() {
    const area = $("codeArea"), layer = $("codeLayer");
    if (!area || !layer) return;
    layer.scrollTop = area.scrollTop;
    layer.scrollLeft = area.scrollLeft;
  }

  function paintCode() {
    const area = $("codeArea"), layer = $("codeLayer");
    if (!area || !layer) return;
    const file = activeFile();
    let html = highlightHtml(area.value, file && file.language);
    if (/\n$/.test(area.value)) html += " ";
    layer.innerHTML = html;
    syncLayerScroll();
    updateFindCount();
  }

  function setupCodeLayer() {
    const area = $("codeArea");
    if (!area || $("codeLayer")) return;
    const wrap = el("div", "code-wrap");
    wrap.id = "codeWrap";
    const layer = el("pre", "code-layer");
    layer.id = "codeLayer";
    layer.setAttribute("aria-hidden", "true");
    area.parentNode.insertBefore(wrap, area);
    wrap.appendChild(layer);
    wrap.appendChild(area);
    area.addEventListener("scroll", syncLayerScroll);
    area.addEventListener("input", paintCode);
    area.addEventListener("keyup", updateStatusPosition);
    area.addEventListener("click", updateStatusPosition);
    window.addEventListener("resize", syncLayerScroll);
    applyPrefs();
  }
  function indentationLabel(text) {
    const lines = String(text || "").split("\n").slice(0, 300);
    let spaces = 0;
    for (const line of lines) {
      const found = /^(\t| +)\S/.exec(line);
      if (!found) continue;
      if (found[1] === "\t") return "Tabs";
      if (!spaces) spaces = found[1].length;
    }
    return `Spaces: ${spaces || IDE.prefs.tabSize}`;
  }

  function updateStatusPosition() {
    const area = $("codeArea");
    if (!area || !$("statusPos")) return;
    const upto = area.value.slice(0, area.selectionStart);
    const line = upto.split("\n").length;
    const column = upto.length - upto.lastIndexOf("\n");
    $("statusPos").textContent = `Ln ${line}, Col ${column}`;
  }

  function renderBreadcrumbs() {
    const box = $("breadcrumbs");
    if (!box) return;
    const file = activeFile();
    if (!file || file.scratch) { box.classList.add("hidden"); box.replaceChildren(); return; }
    box.classList.remove("hidden");
    box.replaceChildren();
    const parts = String(file.path).split("/").filter(Boolean);
    parts.forEach((part, index) => {
      if (index) box.appendChild(el("span", "crumb-sep", "/"));
      const last = index === parts.length - 1;
      const crumb = el("button", `crumb${last ? " current" : ""}`, escapeHtml(part));
      crumb.type = "button";
      crumb.title = parts.slice(0, index + 1).join("/");
      if (!last) crumb.addEventListener("click", () => openActivity("explorer"));
      box.appendChild(crumb);
    });
  }

  function focusLine(line, column) {
    const area = $("codeArea");
    if (!area) return;
    const lines = area.value.split("\n");
    const index = Math.max(0, Math.min(lines.length - 1, (line || 1) - 1));
    let offset = 0;
    for (let i = 0; i < index; i += 1) offset += lines[i].length + 1;
    const within = Math.max(0, Math.min(lines[index].length, (column || 1) - 1));
    area.focus();
    area.setSelectionRange(offset + within, offset + lines[index].length);
    area.scrollTop = Math.max(0, (index - 4) * 20);
    syncLayerScroll();
    updateStatusPosition();
  }

  function openFind() {
    $("findBar").classList.remove("hidden");
    const input = $("findInput");
    const area = $("codeArea");
    if (area && area.selectionEnd > area.selectionStart) {
      const selected = area.value.slice(area.selectionStart, area.selectionEnd);
      if (selected && !/\n/.test(selected)) input.value = selected;
    }
    input.focus();
    input.select();
    runFind(true);
  }

  function closeFind() {
    $("findBar").classList.add("hidden");
    if ($("codeArea")) $("codeArea").focus();
  }

  function findMatches() {
    const area = $("codeArea");
    const needle = ($("findInput").value || "").toLowerCase();
    const out = [];
    if (!area || !needle) return out;
    const hay = area.value.toLowerCase();
    let index = hay.indexOf(needle);
    while (index !== -1 && out.length < 800) {
      out.push(index);
      index = hay.indexOf(needle, index + Math.max(1, needle.length));
    }
    return out;
  }

  function updateFindCount() {
    if (!$("findCount")) return;
    const total = FIND.matches.length;
    $("findCount").textContent = total ? `${Math.min(FIND.index + 1, total)} of ${total}` : "0 of 0";
  }

  function runFind(reset) {
    FIND.matches = findMatches();
    if (reset) {
      const area = $("codeArea");
      const position = area ? area.selectionStart : 0;
      const next = FIND.matches.findIndex((index) => index >= position);
      FIND.index = next === -1 ? 0 : next;
    }
    updateFindCount();
  }

  function jumpMatch(direction) {
    if ($("findBar").classList.contains("hidden")) { openFind(); return; }
    FIND.matches = findMatches();
    if (!FIND.matches.length) { updateFindCount(); return; }
    FIND.index = ((FIND.index + direction) % FIND.matches.length + FIND.matches.length) % FIND.matches.length;
    const area = $("codeArea");
    const start = FIND.matches[FIND.index];
    area.focus();
    area.setSelectionRange(start, start + $("findInput").value.length);
    updateFindCount();
  }

  function showMenu(x, y, items) {
    closeMenu();
    const menu = el("div", "context-menu");
    menu.id = "contextMenu";
    menu.setAttribute("role", "menu");
    items.forEach((item) => {
      if (item.separator) { menu.appendChild(el("div", "menu-sep")); return; }
      if (item.label && !item.run) { menu.appendChild(el("div", "menu-label", escapeHtml(item.label))); return; }
      const button = el("button", item.danger ? "danger" : "", escapeHtml(item.title));
      button.type = "button";
      button.setAttribute("role", "menuitem");
      button.addEventListener("click", () => { closeMenu(); item.run(); });
      menu.appendChild(button);
    });
    document.body.appendChild(menu);
    const rect = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - rect.width - 8))}px`;
    menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - rect.height - 8))}px`;
  }

  function closeMenu() {
    const menu = $("contextMenu");
    if (menu) menu.remove();
  }

  document.addEventListener("click", (event) => {
    if (!event.target.closest("#contextMenu")) closeMenu();
  });

  function tabKeys() {
    const nodes = Array.from($("tabs").querySelectorAll(".tab")).filter((tab) => !tab.classList.contains("chat-tab"));
    const keys = state.openTabs.filter((key) => key !== "chat");
    return nodes.map((node, index) => ({ node, key: keys[index] })).filter((entry) => entry.key);
  }

  function tabMenu(event, entry) {
    event.preventDefault();
    const path = keyPath(entry.key);
    const file = path ? state.files.get(path) : null;
    showMenu(event.clientX, event.clientY, [
      { title: "Close", run: () => closeTab(entry.key) },
      { title: "Close others", run: () => tabKeys().forEach((other) => { if (other.key !== entry.key) closeTab(other.key); }) },
      { title: "Close all", run: () => closeAllFileTabs(true) },
      { separator: true },
      { title: "Save", run: () => { activateTab(entry.key); void saveActiveFile(); } },
      { title: "Attach to chat", run: () => { activateTab(entry.key); attachActiveToChat(); } },
      { title: "Copy path", run: () => { if (path) copyText(path, "Path copied."); } },
      { separator: true },
      { title: "Reveal in Explorer", run: () => openActivity("explorer") },
    ]);
    return file;
  }

  function setupTabMenus() {
    $("tabs").addEventListener("contextmenu", (event) => {
      const node = event.target.closest(".tab");
      if (!node || node.classList.contains("chat-tab")) return;
      const entry = tabKeys().find((item) => item.node === node);
      if (entry) tabMenu(event, entry);
    });
    $("tabs").addEventListener("auxclick", (event) => {
      if (event.button !== 1) return;
      const node = event.target.closest(".tab");
      if (!node || node.classList.contains("chat-tab")) return;
      const entry = tabKeys().find((item) => item.node === node);
      if (entry) { event.preventDefault(); closeTab(entry.key); }
    });
  }
  function regroupSettings() {
    const nav = $("settingsNav");
    if (!nav) return;
    const labels = {
      connection: "AI providers & router", appearance: "Editor & appearance",
      context: "Workspace & context", search: "Deep search",
      privacy: "Privacy & security", about: "About",
    };
    nav.querySelectorAll("button[data-settings-section]").forEach((button) => {
      const label = labels[button.dataset.settingsSection];
      if (label) button.textContent = label;
    });
    [["Local AI", "local"], ["Agents", "agents"], ["Skills", "skills"]].forEach(([label, view]) => {
      const button = el("button", "", label);
      button.type = "button";
      button.dataset.goto = view;
      button.title = `Open the ${label} page`;
      button.addEventListener("click", () => setView(view));
      nav.appendChild(button);
    });
    const section = $("settings-appearance");
    if (!section || $("editorPrefs")) return;
    const block = el("div", "field");
    block.id = "editorPrefs";
    block.innerHTML = '<span>Editor</span><div class="pref-row">' +
      '<label class="field"><span>Font size</span><input id="prefFontSize" type="number" min="10" max="22" step="0.5"></label>' +
      '<label class="field"><span>Tab size</span><input id="prefTabSize" type="number" min="1" max="8" step="1"></label>' +
      '</div><label class="field checkbox-field"><input id="prefWrap" type="checkbox"><span>Wrap long lines</span></label>';
    section.appendChild(block);
    $("prefFontSize").value = String(IDE.prefs.fontSize);
    $("prefTabSize").value = String(IDE.prefs.tabSize);
    $("prefWrap").checked = Boolean(IDE.prefs.wrap);
    $("prefFontSize").addEventListener("change", (event) => {
      IDE.prefs.fontSize = Math.max(10, Math.min(22, Number(event.target.value) || 12.5));
      savePrefs();
    });
    $("prefTabSize").addEventListener("change", (event) => {
      IDE.prefs.tabSize = Math.max(1, Math.min(8, Number(event.target.value) || 2));
      savePrefs();
    });
    $("prefWrap").addEventListener("change", (event) => {
      IDE.prefs.wrap = event.target.checked;
      savePrefs();
    });
  }

  function patchGlobals() {
    const baseSetView = window.setView;
    window.setView = function (view) {
      baseSetView(view);
      if ($("chatView") && $("aiDock") && !$("aiDock").classList.contains("hidden")) {
        $("chatView").classList.remove("hidden");
      }
      const editorVisible = !$("editorView").classList.contains("hidden");
      const pageVisible = ["settings", "usage", "router", "local", "skills", "workspaces", "agents",
        "integrations", "artifacts", "automation", "runs"]
        .some((name) => $(`${name}View`) && !$(`${name}View`).classList.contains("hidden"));
      if ($("workbenchEmpty")) $("workbenchEmpty").classList.toggle("hidden", editorVisible || pageVisible);
      updateStatus();
    };

    const baseSetSidebarMode = window.setSidebarMode;
    window.setSidebarMode = function (mode) {
      baseSetSidebarMode(mode);
      if ($("searchPane")) $("searchPane").classList.add("hidden");
      if ($("scmPane")) $("scmPane").classList.add("hidden");
      IDE.pane = mode;
    };

    const baseRenderTabs = window.renderTabs;
    window.renderTabs = function () {
      baseRenderTabs();
      const first = $("tabs") && $("tabs").firstElementChild;
      if (first) first.classList.add("chat-tab");
    };

    const baseRenderEditor = window.renderEditor;
    window.renderEditor = function () {
      baseRenderEditor();
      renderBreadcrumbs();
      paintCode();
      const file = activeFile();
      if ($("statusLang")) $("statusLang").textContent = file ? file.language : "text";
      if ($("statusIndent")) $("statusIndent").textContent = indentationLabel(file && file.content);
      updateStatusPosition();
      if ($("findBar") && !$("findBar").classList.contains("hidden")) runFind(false);
    };

    const baseUpdateCursor = window.updateCursor;
    window.updateCursor = function () {
      baseUpdateCursor();
      updateStatusPosition();
    };

    const baseApplyPanels = window.applyPanels;
    window.applyPanels = function () {
      baseApplyPanels();
      const shown = state.panelShow ? state.panelShow.right : true;
      if ($("aiDock")) $("aiDock").classList.toggle("hidden", !shown);
      if (shown && $("chatView")) $("chatView").classList.remove("hidden");
    };

    const baseApi = window.api;
    window.api = async function (path, options) {
      const result = await baseApi(path, options);
      const method = (options && options.method) || "GET";
      if (!result.ok) {
        const message = (result.data && (result.data.error || result.data.message)) || `${method} ${path} failed`;
        let where = "";
        try { where = JSON.parse((options && options.body) || "{}").path || ""; } catch { where = ""; }
        recordProblem(where, String(message));
        recordOutput(`${method} ${path} → failed (${message})`, false);
      } else if (method !== "GET") {
        recordOutput(`${method} ${path} → ok`);
      }
      return result;
    };
  }

  const COMMANDS = [
    { label: "File: Save", hint: "Ctrl+S", run: () => { void saveActiveFile(); } },
    { label: "File: Find in file", hint: "Ctrl+F", run: () => openFind() },
    { label: "File: Open folder…", run: () => openFolderDialog() },
    { label: "File: New file in Explorer", run: () => { openActivity("explorer"); if ($("newFileBtn")) $("newFileBtn").click(); } },
    { label: "Search: Find in files", hint: "Ctrl+Shift+F", run: () => openActivity("search") },
    { label: "View: Explorer", run: () => openActivity("explorer") },
    { label: "View: Source control", run: () => openActivity("scm") },
    { label: "View: Agent runs", run: () => openActivity("run") },
    { label: "View: Problems", run: () => selectBottomTab("problems") },
    { label: "View: Tasks", run: () => selectBottomTab("tasks") },
    { label: "View: Terminal", hint: "Ctrl+J", run: () => selectBottomTab("terminal") },
    { label: "View: Toggle AI panel", run: () => togglePanel("right") },
    { label: "View: Toggle sidebar", hint: "Ctrl+B", run: () => togglePanel("left") },
    { label: "AI: New thread", run: () => { if ($("newChat")) $("newChat").click(); } },
    { label: "Preferences: Open settings", run: () => openActivity("settings") },
    { label: "Preferences: Local AI", run: () => openActivity("local") },
    { label: "Preferences: Toggle theme", run: () => applyTheme(document.documentElement.dataset.theme === "light" ? "dark" : "light") },
  ];

  function openPalette() {
    $("palette").classList.remove("hidden");
    $("paletteInput").value = "";
    IDE.paletteIndex = 0;
    renderPalette();
    $("paletteInput").focus();
  }

  function closePalette() { $("palette").classList.add("hidden"); }

  function renderPalette() {
    const list = $("paletteList");
    const query = ($("paletteInput").value || "").toLowerCase().trim();
    IDE.paletteItems = COMMANDS.filter((cmd) => !query || cmd.label.toLowerCase().includes(query));
    if (!IDE.paletteItems.length) { list.innerHTML = '<p class="palette-empty">No matching command.</p>'; return; }
    if (IDE.paletteIndex >= IDE.paletteItems.length) IDE.paletteIndex = 0;
    list.innerHTML = "";
    IDE.paletteItems.forEach((cmd, index) => {
      const button = el("button", `palette-item${index === IDE.paletteIndex ? " active" : ""}`,
        `<span>${escapeHtml(cmd.label)}</span>${cmd.hint ? `<span class="hint">${escapeHtml(cmd.hint)}</span>` : ""}`);
      button.type = "button";
      button.addEventListener("click", () => { closePalette(); cmd.run(); });
      list.appendChild(button);
    });
  }

  function movePalette(delta) {
    if (!IDE.paletteItems.length) return;
    IDE.paletteIndex = (IDE.paletteIndex + delta + IDE.paletteItems.length) % IDE.paletteItems.length;
    renderPalette();
    const active = $("paletteList").querySelector(".palette-item.active");
    if (active) active.scrollIntoView({ block: "nearest" });
  }

  function runPaletteCommand() {
    const command = IDE.paletteItems[IDE.paletteIndex];
    if (!command) return;
    closePalette();
    command.run();
  }

  function setupKeyboard() {
    document.addEventListener("keydown", (event) => {
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (mod && event.shiftKey && key === "p") { event.preventDefault(); openPalette(); return; }
      if (mod && event.shiftKey && key === "f") { event.preventDefault(); openActivity("search"); return; }
      if (mod && key === "f" && activeFile()) { event.preventDefault(); openFind(); return; }
      if (mod && key === "s") { event.preventDefault(); void saveActiveFile(); return; }
      if (mod && key === "b") { event.preventDefault(); togglePanel("left"); return; }
      if (mod && key === "j") { event.preventDefault(); togglePanel("bottom"); return; }
      if (mod && key === "w" && state.activeKey && state.activeKey !== "chat") { event.preventDefault(); closeTab(state.activeKey); return; }
      if (event.key === "F3") { event.preventDefault(); jumpMatch(event.shiftKey ? -1 : 1); return; }
      if (event.key === "Escape") {
        if ($("palette") && !$("palette").classList.contains("hidden")) { closePalette(); return; }
        if ($("findBar") && !$("findBar").classList.contains("hidden")) closeFind();
      }
    });
  }

  function setupResizer() {
    const handle = document.querySelector(".sidebar-resizer");
    if (!handle) return;
    try {
      const stored = localStorage.getItem("freeai-sidebar-w");
      if (stored) document.documentElement.style.setProperty("--sidebar-w", `${stored}px`);
    } catch { /* private mode */ }
    handle.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      handle.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const startWidth = $("sidebar").getBoundingClientRect().width;
      const onMove = (move) => {
        const width = Math.max(200, Math.min(520, Math.round(startWidth + move.clientX - startX)));
        document.documentElement.style.setProperty("--sidebar-w", `${width}px`);
      };
      const onUp = () => {
        handle.removeEventListener("pointermove", onMove);
        handle.removeEventListener("pointerup", onUp);
        const width = parseInt(getComputedStyle(document.documentElement).getPropertyValue("--sidebar-w"), 10);
        if (width) { try { localStorage.setItem("freeai-sidebar-w", String(width)); } catch { /* private */ } }
      };
      handle.addEventListener("pointermove", onMove);
      handle.addEventListener("pointerup", onUp);
    });
  }

  let lastWorkspace = null;

  function tick() {
    updateStatus();
    const workspace = state.workspace && (state.workspace.path || state.workspace.root || state.workspace.name);
    if (workspace && workspace !== lastWorkspace) {
      lastWorkspace = workspace;
      void refreshScm();
    }
  }

  function boot() {
    injectBranchSymbol();
    buildActivityBar();
    buildSidebarPanes();
    buildDock();
    buildBottom();
    buildStatusBar();
    buildBreadcrumbs();
    buildFindBar();
    buildPalette();
    setupCodeLayer();
    setupSearchInput();
    setupTabMenus();
    setupKeyboard();
    setupResizer();
    regroupSettings();
    patchGlobals();

    const workbench = el("div", "workbench-empty hidden");
    workbench.id = "workbenchEmpty";
    workbench.innerHTML = "<h2>No file open</h2><p>Open a folder, pick a file in the Explorer, or ask the assistant on the right. Everything runs on this machine.</p>";
    const actions = el("div", "empty-actions");
    [["Open folder…", () => openFolderDialog()], ["New thread", () => $("newChat").click()], ["Search files", () => openActivity("search")]].forEach(([label, run]) => {
      const button = el("button", "btn btn-quiet btn-small", label);
      button.type = "button";
      button.addEventListener("click", run);
      actions.appendChild(button);
    });
    workbench.appendChild(actions);
    document.querySelector(".main").appendChild(workbench);

    openActivity("explorer");
    // An AI-first workbench starts with the assistant docked open unless the
    // user explicitly closed it in a previous session.
    if (state.panelShow && !state.panelShow.right) {
      let saved = null;
      try { saved = localStorage.getItem("freeai-panel-show"); } catch { saved = null; }
      if (!saved) togglePanel("right");
    }
    applyPrefs();
    updateStatus();
    updateProblemCounts();
    setInterval(tick, 5000);
    setTimeout(() => { if (state.workspace) void refreshScm(); }, 1500);
  }

  boot();
})();
