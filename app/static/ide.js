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
      '<div class="pane-head"><p class="pane-label">Working tree</p>',
      '<button id="scmRefresh" class="tiny-toggle" type="button">Refresh</button></div>',
      '<div id="scmBody" class="scm-list"><p class="empty-note">Open a folder to check its Git state.</p></div>',
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
    body.appendChild(makePane("problemsPane", "problemsBody",
      '<span>Failures recorded by this session</span><button type="button" class="btn btn-quiet btn-small" id="problemsClear">Clear</button>',
      "No problems recorded. Failed saves, reads and requests land here."));
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
    if (id === "problems") renderProblems();
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
    if (!state.workspace) { box.innerHTML = '<p class="empty-note">Open a folder to check its Git state.</p>'; setBranchStatus(null); return; }
    box.innerHTML = '<p class="empty-note">Reading git status…</p>';
    const result = await api("/api/workspace/vcs");
    if (!result.ok) { box.innerHTML = `<p class="empty-note">${escapeHtml(result.data.error || "Could not read git status.")}</p>`; return; }
    IDE.vcs = result.data;
    if (!result.data.available) {
      box.innerHTML = `<p class="empty-note">${escapeHtml(result.data.message || "This folder is not a Git repository.")}</p>`;
      setBranchStatus(null);
      return;
    }
    setBranchStatus(result.data.branch);
    const changes = result.data.changes || [];
    if (!changes.length) { box.innerHTML = '<p class="empty-note">Working tree clean.</p>'; return; }
    box.innerHTML = "";
    appendScmGroup(box, "Staged", changes.filter((c) => c.staged));
    appendScmGroup(box, "Changes", changes.filter((c) => !c.staged));
  }

  function appendScmGroup(box, label, changes) {
    if (!changes.length) return;
    box.appendChild(el("div", "scm-group-label", label));
    changes.forEach((change) => {
      const code = change.code === "?" ? "U" : change.code.replace(/\./g, "") || "M";
      const cls = code === "D" ? " del" : (code === "?" || code === "A") ? " add" : "";
      const row = el("button", "scm-item", `<span class="code${cls}">${escapeHtml(code)}</span><span class="path">${escapeHtml(change.path)}</span>`);
      row.type = "button"; row.title = change.path;
      row.addEventListener("click", () => { void openFile(change.path); });
      box.appendChild(row);
    });
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
      box.innerHTML = '<p class="pane-empty">No problems recorded. Failed saves, reads and requests land here.</p>';
      return;
    }
    box.innerHTML = "";
    IDE.problems.forEach((problem) => {
      const row = el("button", "problem-row",
        `<span class="sev">!</span><span class="where">${escapeHtml(problem.where || "app")}</span>` +
        `<span class="what">${escapeHtml(problem.message)}</span>`);
      row.type = "button";
      if (problem.where) row.addEventListener("click", () => { void openFile(problem.where); });
      box.appendChild(row);
    });
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
