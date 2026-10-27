"use strict";

/* Free AI Studio — local-first coding agent UI. */

const $ = (id) => document.getElementById(id);

/* ------------------------------------------------------------------ */
/* theme: persisted with the profile config so it survives restarts    */
/* ------------------------------------------------------------------ */

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("freeai-theme", theme);
}

async function loadTheme() {
  const stored = localStorage.getItem("freeai-theme");
  let theme = stored === "light" || stored === "dark" ? stored : null;
  if (!theme) {
    theme = window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  applyTheme(theme);
}

function setupThemeToggle() {
  $("themeToggle").addEventListener("click", () => {
    const next = document.documentElement.dataset.theme === "light" ? "dark" : "light";
    applyTheme(next);
  });
}

/* ------------------------------------------------------------------ */
/* projects: server-backed switching                                    */
/* ------------------------------------------------------------------ */

async function loadProjects() {
  const result = await api("/api/projects");
  if (!result.ok) {
    $("projectName").textContent = "Projects unavailable";
    return;
  }
  state.projects = Array.isArray(result.data.projects) ? result.data.projects : [];
  state.activeProjectId = result.data.active || null;
  if (state.activeProjectId) {
    const match = state.projects.find((project) => project.id === state.activeProjectId);
    state.activeProject = match || null;
  } else {
    state.activeProject = null;
  }
  renderProjectSwitcher();
}

function activeProjectName() {
  if (!state.activeProject) return "Default project";
  return state.activeProject.name;
}

function renderProjectSwitcher() {
  $("projectName").textContent = activeProjectName();
  const list = $("projectList");
  list.replaceChildren();
  if (!state.projects.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "No projects yet.";
    list.appendChild(note);
    return;
  }
  state.projects.forEach((project) => {
    const item = document.createElement("div");
    item.className = "project-menu-item" + (project.id === state.activeProjectId ? " active" : "");
    item.setAttribute("role", "option");
    item.setAttribute("aria-selected", String(project.id === state.activeProjectId));
    item.innerHTML = `<span class="project-menu-main"><strong></strong><small></small></span>` +
      `<span class="project-menu-actions-row"></span>`;
    item.querySelector("strong").textContent = project.name;
    item.querySelector("small").textContent = project.folder_path || "no folder";
    item.title = project.folder_path || project.name;
    const actions = document.createElement("span");
    actions.className = "project-menu-actions-row";
    const rename = document.createElement("button");
    rename.className = "tree-tool";
    rename.textContent = "Rename";
    rename.addEventListener("click", (event) => {
      event.stopPropagation();
      renameProject(project);
    });
    const archive = document.createElement("button");
    archive.className = "tree-tool";
    archive.textContent = project.archived_at ? "Unarchive" : "Archive";
    archive.addEventListener("click", (event) => {
      event.stopPropagation();
      updateProject(project, { archived: !project.archived_at });
    });
    const remove = document.createElement("button");
    remove.className = "tree-tool danger";
    remove.textContent = "Delete";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      deleteProject(project);
    });
    actions.append(rename, archive, remove);
    item.appendChild(actions);
    list.appendChild(item);
  });
}

function openProjectMenu() {
  $("projectMenu").classList.remove("hidden");
  $("projectSwitcher").setAttribute("aria-expanded", "true");
}

function closeProjectMenu() {
  $("projectMenu").classList.add("hidden");
  $("projectSwitcher").setAttribute("aria-expanded", "false");
}

async function switchProject(id) {
  closeProjectMenu();
  const result = await api(`/api/projects/${encodeURIComponent(id)}/activate`, { method: "POST", body: "{}" });
  if (!result.ok) {
    toast(result.data.error || "Could not switch projects.");
    return;
  }
  await applyProjectSwitch({ id: result.data.id, name: result.data.name });
}

/* After the server activates a project, everything it scopes reloads: the
   conversation list comes from the NEW selection, so history really changes. */
async function applyProjectSwitch(project) {
  state.activeProject = project;
  state.activeProjectId = project ? project.id : null;
  // Close local chat state the old project owns.
  state.currentId = null;
  state.currentTitle = "New thread";
  $("messages").replaceChildren();
  $("welcome").classList.remove("hidden");
  closeAllFileTabs(true);
  state.chatContext = null;
  renderProjectSwitcher();
  await loadConversations();
  if (state.conversations.length) {
    await openConversation(state.conversations[0].id);
  }
  activateTab(chatTabKey());
  toast(`Switched to ${activeProjectName()}.`);
}

async function createProject() {
  const folder = state.workspace ? state.workspace.absolute_path : "";
  const name = await askName({
    title: "New project",
    label: "Project name",
    placeholder: "portfolio-relaunch",
    submitLabel: "Create",
  });
  if (!name) return;
  const result = await api("/api/projects", {
    method: "POST",
    body: JSON.stringify({ name, folder_path: folder }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not create that project.");
    return;
  }
  await loadProjects();
  if (result.data.project) await applyProjectSwitch(result.data.project);
}

async function renameProject(project) {
  const name = await askName({
    title: "Rename project",
    label: "New name",
    value: project.name,
    submitLabel: "Rename",
  });
  if (!name || name === project.name) return;
  await updateProject(project, { name });
}

async function updateProject(project, patch) {
  const result = await api(`/api/projects/${encodeURIComponent(project.id)}/update`, {
    method: "POST",
    body: JSON.stringify(patch),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not update that project.");
    return;
  }
  await loadProjects();
  if (state.activeProjectId === project.id) {
    state.activeProject = state.projects.find((item) => item.id === project.id) || null;
    renderProjectSwitcher();
  }
}

async function deleteProject(project) {
  const name = project.name || "this project";
  if (!confirm(`Delete the project "${name}"? Its threads are deleted with it. Files on disk are not touched.`)) return;
  const result = await api(`/api/projects/${encodeURIComponent(project.id)}/delete`, { method: "POST", body: "{}" });
  if (!result.ok) {
    toast(result.data.error || "Could not delete that project.");
    return;
  }
  if (state.activeProjectId === project.id) {
    await applyProjectSwitch(result.data.project ? result.data.project : null);
  }
  await loadProjects();
  renderProjectSwitcher();
}

/* ------------------------------------------------------------------ */
/* skills: local SKILL.md folders                                        */
/* ------------------------------------------------------------------ */

async function loadSkills() {
  const box = $("skillList");
  const result = await api("/api/skills");
  if (!result.ok) {
    box.replaceChildren();
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = result.data.error || "Skills folder not readable.";
    box.appendChild(note);
    return;
  }
  const payload = result.data.skills || result.data;
  state.skills = Array.isArray(payload) ? payload : [];
  renderSkills();
  renderSkillsOverview();
}

function skillById(id) {
  return state.skills.find((skill) => skill.id === id) || null;
}

function renderSkills() {
  const box = $("skillList");
  if (state.openSkillId) {
    renderSkillDetail();
    return;
  }
  box.replaceChildren();
  $("skillBack").parentElement.parentElement.classList.add("hidden");
  box.classList.remove("hidden");
  if (!state.skills.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "No skills found. Add SKILL.md folders in this project, or on the current computer, and they appear here.";
    box.appendChild(note);
    return;
  }
  state.skills.forEach((skill) => {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "skill-card" + (skill.enabled ? "" : " off") + (skill.last_error ? " broken" : "");
    card.innerHTML = '<svg class="icon" aria-hidden="true"><use href="#i-cube"/></svg>' +
      '<span class="skill-card-text"><strong></strong><small></small></span>';
    card.querySelector("strong").textContent = skill.name || skill.id;
    card.querySelector("small").textContent = skill.summary || (skill.last_error ? "Load error" : "SKILL.md");
    card.title = skill.path || skill.id;
    card.addEventListener("click", () => openSkillDetail(skill.id));
    box.appendChild(card);
  });
}

function openSkillDetail(id) {
  state.openSkillId = id;
  renderSkillDetail();
}

function renderSkillDetail() {
  const box = $("skillList");
  const detail = $("skillDetail");
  const skill = skillById(state.openSkillId);
  if (!skill) {
    state.openSkillId = null;
    renderSkills();
    return;
  }
  box.classList.add("hidden");
  detail.classList.remove("hidden");
  $("skillDetailName").textContent = skill.name || skill.id;
  const body = $("skillDetailBody");
  body.replaceChildren();
  if (skill.last_error) {
    const err = document.createElement("p");
    err.className = "state error";
    err.textContent = skill.last_error;
    body.appendChild(err);
  }
  const meta = document.createElement("p");
  meta.textContent = (skill.enabled ? "Enabled — " : "Disabled — ") + (skill.path || "");
  body.appendChild(meta);
  if (skill.description) {
    const desc = document.createElement("p");
    desc.textContent = skill.description;
    body.appendChild(desc);
  }
}

function renderSkillsOverview() {
  const host = $("skillsOverview");
  if (!host) return;
  host.replaceChildren();
  if (!state.skills.length) {
    const note = document.createElement("p");
    note.className = "state";
    note.textContent = "No skills on this computer yet. Create a folder with a SKILL.md file to add one.";
    host.appendChild(note);
  }
  state.skills.forEach((skill) => {
    const row = document.createElement("div");
    row.className = "skill-row" + (skill.enabled ? "" : " off") + (skill.last_error ? " broken" : "");
    const main = document.createElement("div");
    main.className = "skill-main";
    const strong = document.createElement("strong");
    strong.textContent = skill.name || skill.id;
    main.appendChild(strong);
    const desc = document.createElement("p");
    desc.textContent = skill.last_error || skill.summary || skill.description || "No description in SKILL.md.";
    main.appendChild(desc);
    const path = document.createElement("small");
    path.className = "skill-path";
    path.textContent = skill.path || "";
    main.appendChild(path);
    row.appendChild(main);
    const side = document.createElement("div");
    side.className = "skill-side";
    if (!skill.last_error) {
      const badge = document.createElement("span");
      badge.className = "badge" + (skill.enabled ? " on" : "");
      badge.textContent = skill.enabled ? "Enabled" : "Disabled";
      side.appendChild(badge);
    } else {
      const badge = document.createElement("span");
      badge.className = "badge broken";
      badge.textContent = "Failed to load";
      side.appendChild(badge);
    }
    if (!skill.last_error) {
      const toggle = document.createElement("button");
      toggle.className = "btn btn-quiet btn-small";
      toggle.textContent = skill.enabled ? "Disable" : "Enable";
      toggle.addEventListener("click", async () => {
        toggle.disabled = true;
        const result = await api(`/api/skills/${encodeURIComponent(skill.id)}/state`, {
          method: "POST",
          body: JSON.stringify({ enabled: !skill.enabled }),
        });
        if (!result.ok) toast(result.data.error || "Could not change that skill.");
        await loadSkills();
      });
      side.appendChild(toggle);
    }
    row.appendChild(side);
    host.appendChild(row);
  });
}

const state = {
  conversations: [],
  currentId: null,
  currentTitle: "New thread",
  busy: false,
  sidebarMode: "threads",
  view: "chat",
  chatContext: null,
  model: "auto",
  contextBudget: 6000,
  lastContextTokens: null,
  /* projects: real server-side scoping — the active project filters every
     conversation list, so switching changes which history the server serves. */
  projects: [],
  activeProject: null,
  activeProjectId: null,
  /* skills: local SKILL.md folders served by /api/skills. */
  skills: [],
  openSkillId: null,
  workspace: null,
  recentFolders: [],
  home: "~",
  tree: { dirs: new Map(), open: new Set(), selected: null, rootLoaded: false },
  files: new Map(), // path -> { path, name, content, original, language, size, scratch }
  openTabs: [], // "chat" | "file:<path>" | "scratch"
  activeKey: "chat",
};

const SCRATCH_KEY = "scratch";

/* ------------------------------------------------------------------ */
/* helpers                                                             */
/* ------------------------------------------------------------------ */

const ICONS = {
  spark: "i-spark",
  chat: "i-chat",
  folder: "i-folder",
  file: "i-file",
  chart: "i-chart",
  gear: "i-gear",
  plus: "i-plus",
  close: "i-close",
  chevron: "i-chevron",
  copy: "i-copy",
  user: "i-user",
};

function icon(name, extra = "") {
  return `<svg class="icon ${extra}" aria-hidden="true"><use href="#${ICONS[name] || name}"/></svg>`;
}

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

function toast(message) {
  const box = $("toast");
  box.textContent = message;
  box.classList.remove("hidden");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => box.classList.add("hidden"), 4000);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });
  const data = await response.json().catch(() => ({}));
  if (!response.ok && !data.error) data.error = `Request failed (HTTP ${response.status})`;
  return { ok: response.ok, status: response.status, data };
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function basename(path) {
  const clean = String(path || "").replace(/\/+$/, "");
  return clean.split("/").pop() || clean;
}

function parentPath(path) {
  const clean = String(path || "").replace(/\/+$/, "");
  const index = clean.lastIndexOf("/");
  return index <= 0 ? "" : clean.slice(0, index);
}

function setConnection(kind, text) {
  const dot = $("connDot");
  const pillDot = $("connPill").querySelector(".conn-dot");
  dot.classList.toggle("ok", kind === "ok");
  dot.classList.toggle("bad", kind === "bad");
  pillDot.classList.toggle("ok", kind === "ok");
  pillDot.classList.toggle("bad", kind === "bad");
  $("connText").textContent = text;
  $("connPillText").textContent = text;
}

function autoGrow(textarea) {
  if (!textarea) return;
  textarea.style.height = "auto";
  const max = 190;
  textarea.style.height = `${Math.min(textarea.scrollHeight, max)}px`;
  textarea.style.overflowY = textarea.scrollHeight > max ? "auto" : "hidden";
}

/* ------------------------------------------------------------------ */
/* markdown (escaped first, then a small safe subset)                  */
/* ------------------------------------------------------------------ */

function inlineMarkdown(text) {
  const codes = [];
  let html = escapeHtml(text);
  html = html.replace(/`([^`\n]+)`/g, (_, code) => {
    codes.push(code);
    return `\u0000${codes.length - 1}\u0000`;
  });
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  html = html.replace(/(^|[^\w*])\*([^*\n]+)\*(?![\w*])/g, "$1<em>$2</em>");
  html = html.replace(/~~([^~\n]+)~~/g, "<del>$1</del>");
  html = html.replace(
    /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
    '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>',
  );
  html = html.replace(/\n/g, "<br>");
  html = html.replace(/\u0000(\d+)\u0000/g, (_, index) => `<code>${codes[Number(index)]}</code>`);
  return html;
}

function codeBlockHtml(code, language) {
  return `<div class="code-block"><div class="code-head"><span>${escapeHtml(language || "code")}</span>` +
    `<button class="code-copy" data-copy-code>Copy</button></div>` +
    `<pre><code>${escapeHtml(code)}</code></pre></div>`;
}

function renderMarkdown(source) {
  const lines = String(source ?? "").replace(/\r\n?/g, "\n").split("\n");
  const out = [];
  let paragraph = [];
  let list = null;

  const flushParagraph = () => {
    if (paragraph.length) {
      out.push(`<p>${inlineMarkdown(paragraph.join("\n"))}</p>`);
      paragraph = [];
    }
  };
  const flushList = () => {
    if (list) {
      out.push(`<${list.tag}>${list.items.map((item) => `<li>${inlineMarkdown(item)}</li>`).join("")}</${list.tag}>`);
      list = null;
    }
  };
  const flushAll = () => { flushParagraph(); flushList(); };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];

    const fence = line.match(/^\s*```\s*([\w.+#-]*)\s*$/);
    if (fence) {
      flushAll();
      const code = [];
      i += 1;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) {
        code.push(lines[i]);
        i += 1;
      }
      out.push(codeBlockHtml(code.join("\n"), fence[1]));
      continue;
    }

    if (!line.trim()) { flushAll(); continue; }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      flushAll();
      const level = Math.min(heading[1].length + 1, 6);
      out.push(`<h${level}>${inlineMarkdown(heading[2])}</h${level}>`);
      continue;
    }

    if (/^\s*(---|\*\*\*|___)\s*$/.test(line)) { flushAll(); out.push("<hr>"); continue; }

    if (/^>\s?/.test(line)) {
      flushAll();
      const quoted = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        quoted.push(lines[i].replace(/^>\s?/, ""));
        i += 1;
      }
      i -= 1;
      out.push(`<blockquote>${inlineMarkdown(quoted.join("\n"))}</blockquote>`);
      continue;
    }

    const bullet = line.match(/^\s*[-*+]\s+(.*)$/);
    const numbered = line.match(/^\s*\d+[.)]\s+(.*)$/);
    if (bullet || numbered) {
      flushParagraph();
      const tag = bullet ? "ul" : "ol";
      if (!list || list.tag !== tag) { flushList(); list = { tag, items: [] }; }
      list.items.push((bullet || numbered)[1]);
      continue;
    }

    paragraph.push(line);
  }
  flushAll();
  return out.join("");
}

/* ------------------------------------------------------------------ */
/* tabs                                                                */
/* ------------------------------------------------------------------ */

function chatTabKey() { return "chat"; }

function fileKey(path) { return `file:${path}`; }

function keyPath(key) {
  return key.startsWith("file:") ? key.slice(5) : key === SCRATCH_KEY ? SCRATCH_KEY : null;
}

function activeFile() {
  const path = keyPath(state.activeKey);
  return path ? state.files.get(path) || null : null;
}

function renderTabs() {
  const box = $("tabs");
  box.replaceChildren();

  const chatTab = document.createElement("button");
  chatTab.className = "tab" + (state.activeKey === chatTabKey() ? " active" : "");
  chatTab.setAttribute("role", "tab");
  chatTab.innerHTML = `${icon("chat")}<span class="tab-label"></span>`;
  chatTab.querySelector(".tab-label").textContent = state.currentTitle || "New thread";
  chatTab.addEventListener("click", () => activateTab(chatTabKey()));
  box.appendChild(chatTab);

  state.openTabs.filter((key) => key !== chatTabKey()).forEach((key) => {
    const file = state.files.get(keyPath(key));
    if (!file) return;
    const tab = document.createElement("button");
    tab.className = "tab" + (state.activeKey === key ? " active" : "");
    tab.setAttribute("role", "tab");
    tab.innerHTML = `${icon(file.scratch ? "spark" : "file")}<span class="tab-label"></span>` +
      `<span class="tab-dirty hidden"></span><span class="tab-close">${icon("close")}</span>`;
    const label = file.scratch ? "Scratch pad" : basename(file.path);
    tab.querySelector(".tab-label").textContent = label;
    tab.title = file.scratch ? "Scratch pad" : file.path;
    if (file.content !== file.original) tab.querySelector(".tab-dirty").classList.remove("hidden");
    tab.addEventListener("click", (event) => {
      if (event.target.closest(".tab-close")) return;
      activateTab(key);
    });
    tab.querySelector(".tab-close").addEventListener("click", (event) => {
      event.stopPropagation();
      closeTab(key);
    });
    box.appendChild(tab);
  });
}

function setView(view) {
  state.view = view;
  $("chatView").classList.toggle("hidden", view !== "chat");
  $("editorView").classList.toggle("hidden", view !== "editor");
  $("settingsView").classList.toggle("hidden", view !== "settings");
  $("usageView").classList.toggle("hidden", view !== "usage");
  $("routerView").classList.toggle("hidden", view !== "router");
  $("skillsView").classList.toggle("hidden", view !== "skills");
  $("usageBtn").classList.toggle("active", view === "usage");
  $("settingsBtn").classList.toggle("active", view === "settings");
  $("routerBtn").classList.toggle("active", view === "router");
  document.body.classList.remove("nav-open");
  $("scrim").classList.add("hidden");
  if (view === "chat") setTimeout(() => $("prompt").focus(), 30);
  if (view === "usage") loadUsage();
  if (view === "router") loadConsoleStatus();
  if (view === "skills") renderSkillsOverview();
}

function activateTab(key) {
  state.activeKey = key;
  renderTabs();
  if (key === chatTabKey()) {
    setView("chat");
    return;
  }
  renderEditor();
  setView("editor");
}

function closeTab(key) {
  const file = state.files.get(keyPath(key));
  if (file && file.content !== file.original) {
    if (!confirm(`Close "${file.scratch ? "Scratch pad" : basename(file.path)}" without saving?`)) return;
  }
  state.openTabs = state.openTabs.filter((item) => item !== key);
  state.files.delete(keyPath(key));
  if (state.activeKey === key) {
    const next = state.openTabs[state.openTabs.length - 1] || chatTabKey();
    activateTab(next);
  } else {
    renderTabs();
  }
}

/* ------------------------------------------------------------------ */
/* chat                                                                */
/* ------------------------------------------------------------------ */

function addMessage(role, content, options = {}) {
  const { meta = null, error = false, attachment = null } = options;
  $("welcome").classList.add("hidden");

  const row = document.createElement("div");
  row.className = `msg msg-${role}` + (error ? " error" : "");

  const head = document.createElement("div");
  head.className = "msg-head";
  head.innerHTML = `<span class="msg-avatar ${role === "assistant" ? "assistant" : ""}">` +
    `${role === "assistant" ? icon("spark") : icon("user")}</span>` +
    `<span class="msg-author">${role === "assistant" ? "Free AI Studio" : "You"}</span>` +
    `<span class="msg-meta"></span>`;
  if (meta) {
    const parts = [];
    if (meta.model) parts.push(meta.model);
    if (meta.provider) parts.push(meta.provider);
    if (meta.latency_ms != null) parts.push(`${meta.latency_ms} ms`);
    if (meta.total_tokens != null) parts.push(`${meta.total_tokens} tokens`);
    head.querySelector(".msg-meta").textContent = parts.join(" · ");
  } else if (role === "assistant") {
    head.querySelector(".msg-meta").textContent = "";
  }
  row.appendChild(head);

  if (role === "assistant") {
    const body = document.createElement("div");
    body.className = "msg-body";
    body.innerHTML = renderMarkdown(content);
    row.appendChild(body);

    if (!error) appendMessageActions(row, [{ label: "Copy reply", run: () => copyText(content, "Reply copied.") }]);
  } else {
    const card = document.createElement("div");
    card.className = "msg-card";
    card.textContent = content;
    row.appendChild(card);
    if (attachment) {
      const note = document.createElement("div");
      note.className = "msg-attachment";
      note.innerHTML = `${icon("file")}<span></span>`;
      note.querySelector("span").textContent = attachment;
      row.appendChild(note);
    }
    appendMessageActions(row, [
      { label: "Copy", run: () => copyText(content, "Message copied.") },
      { label: "Resend", run: () => sendMessage(content) },
    ]);
  }

  $("messages").appendChild(row);
  scrollChat();
}

function appendMessageActions(row, actions) {
  // A single horizontal row of small buttons under the message. Previously
  // these sat in a separate strip; keeping them in one row means the actions
  // read left-to-right instead of stacking vertically.
  const tools = document.createElement("div");
  tools.className = "msg-actions";
  actions.forEach((action) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "msg-action";
    button.textContent = action.label;
    button.addEventListener("click", action.run);
    tools.appendChild(button);
  });
  row.appendChild(tools);
}

function scrollChat() {
  const box = $("chatScroll");
  box.scrollTop = box.scrollHeight;
}

function setBusy(value) {
  state.busy = value;
  $("sendBtn").disabled = value;
  $("attachContextBtn").disabled = value;
  $("thinking").classList.toggle("hidden", !value);
  if (value) scrollChat();
}

async function copyText(text, message) {
  try {
    await navigator.clipboard.writeText(text);
    toast(message);
  } catch {
    const scratch = document.createElement("textarea");
    scratch.value = text;
    document.body.appendChild(scratch);
    scratch.select();
    document.execCommand("copy");
    scratch.remove();
    toast(message);
  }
}

async function loadConversations() {
  const result = await api("/api/conversations");
  if (!result.ok) {
    toast(result.data.error || "Could not load history.");
    return;
  }
  state.conversations = Array.isArray(result.data) ? result.data : [];
  renderConversations();
}

function renderConversations() {
  const box = $("conversationList");
  box.replaceChildren();
  if (!state.conversations.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "No threads yet. Ask something to start one.";
    box.appendChild(note);
    return;
  }
  state.conversations.forEach((conversation) => {
    const item = document.createElement("div");
    item.className = "thread-item" + (conversation.id === state.currentId ? " active" : "");
    item.innerHTML = `${icon("chat", "thread-icon")}<span class="thread-title"></span>` +
      `<button class="thread-delete" title="Delete thread" aria-label="Delete thread">${icon("close")}</button>`;
    item.querySelector(".thread-title").textContent = conversation.title;
    item.title = conversation.title;
    item.addEventListener("click", (event) => {
      if (event.target.closest(".thread-delete")) return;
      openConversation(conversation.id);
    });
    item.querySelector(".thread-delete").addEventListener("click", async (event) => {
      event.stopPropagation();
      if (!confirm(`Delete "${conversation.title}" and its local history?`)) return;
      await api(`/api/conversations/${encodeURIComponent(conversation.id)}/delete`, { method: "POST", body: "{}" });
      if (state.currentId === conversation.id) {
        state.currentId = null;
        state.currentTitle = "New thread";
        $("messages").replaceChildren();
        $("welcome").classList.remove("hidden");
        renderTabs();
      }
      await loadConversations();
      toast("Thread deleted.");
    });
    box.appendChild(item);
  });
}

async function openConversation(id) {
  const result = await api(`/api/conversations/${encodeURIComponent(id)}/messages`);
  if (!result.ok) {
    toast(result.data.error || "Could not load that thread.");
    return;
  }
  state.currentId = id;
  const match = state.conversations.find((entry) => entry.id === id);
  state.currentTitle = match ? match.title : "Thread";
  $("messages").replaceChildren();
  const messages = Array.isArray(result.data) ? result.data : [];
  $("welcome").classList.toggle("hidden", messages.length > 0);
  messages.forEach((message) => addMessage(message.role === "assistant" ? "assistant" : "user", message.content));
  scrollChat();
  renderConversations();
  renderTabs();
  activateTab(chatTabKey());
}

async function newConversation() {
  const result = await api("/api/conversations", {
    method: "POST",
    body: JSON.stringify({ title: "New thread" }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not create a thread.");
    return;
  }
  state.currentId = result.data.id;
  state.currentTitle = result.data.title || "New thread";
  $("messages").replaceChildren();
  $("welcome").classList.remove("hidden");
  await loadConversations();
  renderTabs();
  activateTab(chatTabKey());
  $("prompt").focus();
}

/* Streaming chat: one AbortController per request. Stop aborts the fetch —
   the server's ReplyRecorder saves the partial reply, so nothing is lost. */
async function sendMessage(preset = null) {
  if (state.busy) return;
  const message = String(preset ?? $("prompt").value).trim();
  if (!message) { $("prompt").focus(); return; }

  if (!state.currentId) {
    const created = await api("/api/conversations", {
      method: "POST",
      body: JSON.stringify({ title: message.slice(0, 64) }),
    });
    if (!created.ok) {
      toast(created.data.error || "Could not create a thread.");
      return;
    }
    state.currentId = created.data.id;
    state.currentTitle = created.data.title || message.slice(0, 64);
  }

  const context = state.chatContext;
  $("prompt").value = "";
  autoGrow($("prompt"));
  addMessage("user", message, { attachment: context ? (context.scratch ? "Scratch pad attached" : context.path) : null });
  // Show the estimate again until the router reports the real count.
  state.lastContextTokens = null;
  updateContextMeter();
  setBusy(true);

  const controller = new AbortController();
  state.abort = controller;
  addStreamRow();

  let answer = "";
  let failed = null;
  try {
    const response = await fetch("/api/chat/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        conversation_id: state.currentId,
        message,
        code: context ? context.content : "",
        language: context ? context.language : "text",
      }),
      signal: controller.signal,
    });
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      failed = data.error || `Request failed (HTTP ${response.status})`;
    } else {
      await consumeStream(response.body, (frame) => {
        if (frame.delta) {
          answer += frame.delta;
          appendStreamText(answer);
        }
        if (frame.error) failed = frame.error;
        if (frame.done && frame.meta) {
          if (frame.meta.context_tokens != null) state.lastContextTokens = Number(frame.meta.context_tokens);
          applyStreamMeta(frame.meta);
        }
      });
    }
  } catch (error) {
    if (error && error.name !== "AbortError") failed = error.message || "The request failed.";
  }
  finishStreamRow(answer, failed);
  state.abort = null;
  state.busy = false;
  $("sendBtn").disabled = false;
  $("attachContextBtn").disabled = false;
  $("thinking").classList.add("hidden");
  $("streamIndicator").classList.add("hidden");
  setConnection(failed ? "bad" : "ok", failed ? "Last request failed" : "Router connected");
  updateContextMeter();
  await loadConversations();
  await loadUsage();
  syncCurrentTitle();
}

/* Parse an SSE body into frames and run callback for each. Plainly reads
   "data: {...}\n\n" events — the only content type this server emits. */
async function consumeStream(body, onFrame) {
  if (!body) return;
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  const emit = () => {
    let index;
    while ((index = buffer.indexOf("\n\n")) !== -1) {
      const event = buffer.slice(0, index);
      buffer = buffer.slice(index + 2);
      const payloadLine = event.split("\n").find((line) => line.startsWith("data:"));
      if (!payloadLine) continue;
      let frame;
      try { frame = JSON.parse(payloadLine.slice(5).trim()); } catch { continue; }
      onFrame(frame);
    }
  };
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    emit();
  }
  buffer += decoder.decode();
  emit();
}

let streamRow = null;

function addStreamRow() {
  streamRow = addMessage("assistant", "", { streaming: true });
}

function appendStreamText(answer) {
  if (!streamRow) return;
  const body = streamRow.querySelector(".msg-body");
  if (body) {
    body.innerHTML = renderMarkdown(answer) + '<span class="stream-caret" aria-hidden="true"></span>';
    scrollChat();
  }
}

function applyStreamMeta(meta) {
  if (!streamRow || !meta) return;
  const head = streamRow.querySelector(".msg-meta");
  if (!head) return;
  const parts = [];
  if (meta.model) parts.push(meta.model);
  if (meta.provider) parts.push(meta.provider);
  if (meta.latency_ms != null) parts.push(`${meta.latency_ms} ms`);
  if (meta.total_tokens != null) parts.push(`${meta.total_tokens} tokens`);
  head.textContent = parts.join(" · ");
}

function finishStreamRow(answer, failure) {
  if (!streamRow) return;
  const row = streamRow;
  streamRow = null;
  const body = row.querySelector(".msg-body");
  const caret = row.querySelector(".stream-caret");
  if (caret) caret.remove();
  if (failure) {
    row.classList.add("error");
    if (body) body.innerHTML = renderMarkdown(answer ? `${answer}\n\n**${failure}**` : failure);
  } else if (!answer.trim()) {
    row.remove();
  } else if (body) {
    body.innerHTML = renderMarkdown(answer);
    appendMessageActions(row, [{ label: "Copy reply", run: () => copyText(answer, "Reply copied.") }]);
  }
  scrollChat();
}

function stopStream() {
  if (state.abort) state.abort.abort();
}

function syncCurrentTitle() {
  const match = state.conversations.find((entry) => entry.id === state.currentId);
  if (match) state.currentTitle = match.title;
  renderTabs();
}

/* ------------------------------------------------------------------ */
/* workspace + explorer                                                */
/* ------------------------------------------------------------------ */

function setSidebarMode(mode) {
  state.sidebarMode = mode;
  ["threads", "files", "skills"].forEach((name) => {
    const button = $(`mode${name.charAt(0).toUpperCase()}${name.slice(1)}`);
    if (!button) return;
    const active = mode === name;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", String(active));
  });
  $("threadsPane").classList.toggle("hidden", mode !== "threads");
  $("filesPane").classList.toggle("hidden", mode !== "files");
  if ($("skillsPane")) $("skillsPane").classList.toggle("hidden", mode !== "skills");
}

async function loadWorkspace() {
  const result = await api("/api/workspace");
  if (!result.ok) return;
  state.workspace = result.data.workspace || null;
  state.recentFolders = Array.isArray(result.data.recent) ? result.data.recent : [];
  state.home = result.data.home || "~";
  renderWorkspaceHeader();
  if (state.workspace) {
    await loadDir("", { refresh: true });
  } else {
    state.tree = { dirs: new Map(), open: new Set(), selected: null, rootLoaded: false };
    renderTree();
  }
}

function renderWorkspaceHeader() {
  const open = Boolean(state.workspace);
  $("wsName").textContent = open ? state.workspace.name : "No folder open";
  $("wsPath").textContent = open ? state.workspace.path : "Open a folder to browse and edit files.";
  $("wsClose").classList.toggle("hidden", !open);
  $("treeTools").classList.toggle("hidden", !open);
  $("openFolderBtn").textContent = open ? "Change folder…" : "Open folder…";

  const recent = $("wsRecent");
  recent.replaceChildren();
  const folders = state.recentFolders.filter((path) => !open || path !== state.workspace.path);
  if (folders.length) {
    recent.classList.remove("hidden");
    folders.slice(0, 4).forEach((path) => {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.textContent = path;
      chip.title = `Open ${path}`;
      chip.addEventListener("click", () => openWorkspacePath(path));
      recent.appendChild(chip);
    });
  } else {
    recent.classList.add("hidden");
  }
  updateTreeTools();
}

function updateTreeTools() {
  const selected = state.tree.selected;
  $("renameBtn").disabled = !selected;
  $("deleteBtn").disabled = !selected;
  $("newFileBtn").disabled = !state.workspace;
  $("newFolderBtn").disabled = !state.workspace;
}

async function loadDir(path, options = {}) {
  if (!state.workspace) return null;
  const result = await api(`/api/workspace/entries?path=${encodeURIComponent(path)}`);
  if (!result.ok) {
    toast(result.data.error || "Could not read that folder.");
    return null;
  }
  state.tree.dirs.set(path, result.data.entries || []);
  if (path === "") state.tree.rootLoaded = true;
  if (options.open !== false) state.tree.open.add(path);
  if (options.refresh) renderTree();
  return result.data;
}

function renderTree() {
  const box = $("fileTree");
  box.replaceChildren();
  if (!state.workspace) {
    const note = document.createElement("p");
    note.className = "tree-empty";
    note.textContent = "Open a folder to browse its files. Nothing outside that folder is readable or writable.";
    box.appendChild(note);
    return;
  }
  if (!state.tree.rootLoaded) {
    const note = document.createElement("p");
    note.className = "tree-empty";
    note.textContent = "Reading folder…";
    box.appendChild(note);
    return;
  }

  const build = (dirPath, depth) => {
    const entries = state.tree.dirs.get(dirPath) || [];
    const fragment = document.createDocumentFragment();
    if (!entries.length && depth === 0) {
      const note = document.createElement("p");
      note.className = "tree-empty";
      note.textContent = "This folder is empty.";
      fragment.appendChild(note);
      return fragment;
    }
    entries.forEach((entry) => {
      const row = document.createElement("div");
      const isOpen = entry.dir && state.tree.open.has(entry.path);
      row.className = "tree-row" + (entry.dir ? " dir" : "") + (isOpen ? " open" : "") +
        (entry.hidden ? " hidden-file" : "") +
        (state.tree.selected && state.tree.selected.path === entry.path ? " selected" : "");
      row.style.paddingLeft = `${6 + depth * 12}px`;
      row.setAttribute("role", "treeitem");
      row.title = entry.path;
      row.innerHTML = (entry.dir ? icon("chevron", "tree-chevron") : `<span class="tree-chevron"></span>`) +
        icon(entry.dir ? "folder" : "file") + `<span class="tree-name"></span>`;
      row.querySelector(".tree-name").textContent = entry.name;

      row.addEventListener("click", (event) => {
        event.stopPropagation();
        state.tree.selected = entry;
        updateTreeTools();
        if (entry.dir) {
          if (state.tree.open.has(entry.path)) {
            state.tree.open.delete(entry.path);
            renderTree();
          } else {
            loadDir(entry.path).then(() => renderTree());
          }
        } else {
          renderTree();
          openFile(entry.path);
        }
      });
      fragment.appendChild(row);
      if (isOpen) fragment.appendChild(build(entry.path, depth + 1));
    });
    return fragment;
  };

  box.appendChild(build("", 0));
}

function resetTree() {
  state.tree = { dirs: new Map(), open: new Set(), selected: null, rootLoaded: false };
  renderTree();
  updateTreeTools();
}

async function applyWorkspacePayload(payload) {
  if (!payload || !payload.workspace) return false;
  state.workspace = payload.workspace;
  state.recentFolders = Array.isArray(payload.recent) ? payload.recent : [];
  resetTree();
  renderWorkspaceHeader();
  await loadDir("", { refresh: true });
  setSidebarMode("files");
  hideModal($("folderModal"));
  toast(`Opened ${state.workspace.name}.`);
  return true;
}

async function openWorkspacePath(path) {
  // Ask before switching folders: the server opens the folder immediately, so
  // the unsaved-tab decision must happen before the request, not after it.
  if (!closeAllFileTabs()) return false;
  const result = await api("/api/workspace/open", {
    method: "POST",
    body: JSON.stringify({ path }),
  });
  if (!result.ok) {
    const message = result.data.error || "Could not open that folder.";
    if ($("folderModal").classList.contains("hidden")) toast(message);
    else showFolderMessage(message, false);
    return false;
  }
  return applyWorkspacePayload(result.data);
}

async function closeWorkspace() {
  if (!state.workspace) return;
  if (!confirm(`Close "${state.workspace.name}"? Unsaved tabs stay open with their content.`)) return;
  await api("/api/workspace/close", { method: "POST", body: "{}" });
  state.workspace = null;
  resetTree();
  renderWorkspaceHeader();
  toast("Folder closed.");
}

function showFolderMessage(message, ok = true) {
  const box = $("folderMessage");
  box.textContent = message;
  box.className = "notice " + (ok ? "success" : "error");
}

function openFolderDialog() {
  $("folderInput").value = state.workspace ? state.workspace.absolute_path : state.home;
  $("folderMessage").className = "notice hidden";
  const recent = $("folderRecent");
  recent.replaceChildren();
  state.recentFolders.slice(0, 5).forEach((path) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = path;
    button.addEventListener("click", () => { $("folderInput").value = path; });
    recent.appendChild(button);
  });
  showModal($("folderModal"));
  setTimeout(() => $("folderInput").focus(), 30);
}

function refreshDir(path) {
  return loadDir(path, { refresh: true });
}

async function createItem(dir) {
  const name = await askName({
    title: dir.isDir ? "New folder" : "New file",
    label: "Name",
    placeholder: dir.isDir ? "components" : "main.rs",
    submitLabel: "Create",
  });
  if (!name) return;
  const path = dir.base ? `${dir.base}/${name}` : name;
  const result = await api("/api/workspace/create", {
    method: "POST",
    body: JSON.stringify({ path, dir: dir.isDir }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not create that item.");
    return;
  }
  if (dir.base && !state.tree.open.has(dir.base)) await loadDir(dir.base, { open: true });
  if (dir.isDir) await loadDir(path, { open: true });
  await refreshDir(dir.base);
  toast(`${dir.isDir ? "Folder" : "File"} created.`);
  if (!dir.isDir) openFile(path);
}

async function renameSelected() {
  const selected = state.tree.selected;
  if (!selected) return;
  const name = await askName({
    title: "Rename",
    label: "New name",
    value: selected.name,
    submitLabel: "Rename",
  });
  if (!name || name === selected.name) return;
  const target = `${parentPath(selected.path)}${parentPath(selected.path) ? "/" : ""}${name}`;
  const result = await api("/api/workspace/rename", {
    method: "POST",
    body: JSON.stringify({ from: selected.path, to: target }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not rename that item.");
    return;
  }
  remapOpenPaths(selected.path, target);
  const parent = parentPath(selected.path);
  for (const key of [...state.tree.dirs.keys()]) {
    if (key === selected.path || key.startsWith(`${selected.path}/`)) state.tree.dirs.delete(key);
  }
  state.tree.selected = null;
  if (parent) await loadDir(parent, { open: true });
  await refreshDir("");
  toast("Renamed.");
}

async function deleteSelected() {
  const selected = state.tree.selected;
  if (!selected) return;
  if (!confirm(`Delete "${selected.name}"? This removes it from disk.`)) return;
  const result = await api("/api/workspace/delete", {
    method: "POST",
    body: JSON.stringify({ path: selected.path }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not delete that item.");
    return;
  }
  closeTabsUnder(selected.path);
  for (const key of [...state.tree.dirs.keys()]) {
    if (key === selected.path || key.startsWith(`${selected.path}/`)) state.tree.dirs.delete(key);
  }
  state.tree.open.delete(selected.path);
  state.tree.selected = null;
  await refreshDir(parentPath(selected.path));
  toast("Deleted.");
}

function remapOpenPaths(from, to) {
  const remapped = new Map();
  state.files.forEach((file, key) => {
    if (key === from) {
      file.path = to;
      file.name = basename(to);
      remapped.set(to, file);
      state.openTabs = state.openTabs.map((item) => (item === fileKey(from) ? fileKey(to) : item));
      if (state.activeKey === fileKey(from)) state.activeKey = fileKey(to);
    } else if (key.startsWith(`${from}/`)) {
      const next = to + key.slice(from.length);
      file.path = next;
      file.name = basename(next);
      remapped.set(next, file);
      state.openTabs = state.openTabs.map((item) => (item === fileKey(key) ? fileKey(next) : item));
      if (state.activeKey === fileKey(key)) state.activeKey = fileKey(next);
    } else {
      remapped.set(key, file);
    }
  });
  state.files = remapped;
  if (state.chatContext && !state.chatContext.scratch && state.chatContext.path === from) {
    state.chatContext.path = to;
    renderContextChip();
  }
  renderTabs();
  renderEditor();
}

function closeTabsUnder(path) {
  const affected = state.openTabs.filter((key) => {
    const file = state.files.get(keyPath(key));
    if (!file) return false;
    return file.path === path || file.path.startsWith(`${path}/`);
  });
  affected.forEach((key) => {
    const file = state.files.get(keyPath(key));
    if (file && file.content !== file.original) {
      if (!confirm(`"${file.name}" has unsaved changes. Discard them?`)) return;
    }
    state.openTabs = state.openTabs.filter((item) => item !== key);
    state.files.delete(keyPath(key));
  });
  if (!state.openTabs.includes(state.activeKey) && state.activeKey !== chatTabKey()) {
    activateTab(chatTabKey());
  }
  renderTabs();
}

function closeAllFileTabs(silent = false) {
  const dirty = [...state.files.values()].filter((file) => file.content !== file.original);
  if (!silent && dirty.length && !confirm(`${dirty.length} tab(s) have unsaved changes. Close them anyway?`)) {
    return false;
  }
  state.files.clear();
  state.openTabs = [chatTabKey()];
  state.activeKey = chatTabKey();
  state.chatContext = null;
  renderContextChip();
  renderTabs();
  setView("chat");
  return true;
}

/* ------------------------------------------------------------------ */
/* editor                                                              */
/* ------------------------------------------------------------------ */

async function openFile(path, options = {}) {
  if (!state.workspace) return;
  const existing = state.files.get(path);
  if (existing) {
    activateTab(fileKey(path));
    return;
  }
  const result = await api(`/api/workspace/file?path=${encodeURIComponent(path)}`);
  if (!result.ok) {
    toast(result.data.error || "Could not open that file.");
    return;
  }
  const file = {
    path: result.data.path || path,
    name: basename(result.data.path || path),
    content: result.data.content,
    original: result.data.content,
    language: result.data.language || "text",
    size: result.data.size || result.data.content.length,
    scratch: false,
  };
  state.files.set(file.path, file);
  state.openTabs.push(fileKey(file.path));
  activateTab(fileKey(file.path));
  if (options.focus !== false) $("codeArea").focus();
}

function openScratchPad() {
  if (!state.files.has(SCRATCH_KEY)) {
    state.files.set(SCRATCH_KEY, {
      path: SCRATCH_KEY,
      name: "Scratch pad",
      content: "",
      original: "",
      language: "text",
      size: 0,
      scratch: true,
    });
    state.openTabs.push(SCRATCH_KEY);
  }
  activateTab(SCRATCH_KEY);
  $("codeArea").focus();
}

let renderedPath = null;

function renderEditor() {
  const file = activeFile();
  if (!file) return;
  $("editorPath").textContent = file.scratch ? "Scratch pad" : file.path;
  $("editorPath").title = file.scratch ? "Scratch pad (not saved to disk)" : file.path;
  $("editorLang").textContent = file.language;
  $("editorSize").textContent = formatBytes(new Blob([file.content]).size);
  if ($("codeArea").value !== file.content) $("codeArea").value = file.content;
  const dirty = file.content !== file.original;
  $("editorDirty").classList.toggle("hidden", !dirty);
  $("editorState").textContent = file.scratch
    ? (dirty ? "Scratch pad (not saved)" : "Scratch pad")
    : (dirty ? "Unsaved changes" : "Saved");
  $("editorState").classList.toggle("dirty", dirty);
  $("saveBtn").disabled = Boolean(file.scratch);
  $("revertBtn").disabled = !dirty;
  updateGutter();
  updateCursor();
  if (renderedPath !== file.path) {
    $("codeArea").scrollTop = 0;
    $("gutter").scrollTop = 0;
    renderedPath = file.path;
  }
}

function updateGutter() {
  const count = Math.max(1, $("codeArea").value.split("\n").length);
  const lines = [];
  for (let i = 1; i <= count; i += 1) lines.push(i);
  $("gutter").textContent = lines.join("\n");
  $("gutter").scrollTop = $("codeArea").scrollTop;
}

function updateCursor() {
  const area = $("codeArea");
  const upto = area.value.slice(0, area.selectionStart);
  const line = upto.split("\n").length;
  const column = upto.length - upto.lastIndexOf("\n");
  $("editorCursor").textContent = `Ln ${line}, Col ${column}`;
}

function onEditorInput() {
  const file = activeFile();
  if (!file) return;
  file.content = $("codeArea").value;
  const dirty = file.content !== file.original;
  $("editorDirty").classList.toggle("hidden", !dirty);
  $("editorState").textContent = file.scratch
    ? (dirty ? "Scratch pad (not saved)" : "Scratch pad")
    : (dirty ? "Unsaved changes" : "Saved");
  $("editorState").classList.toggle("dirty", dirty);
  $("revertBtn").disabled = !dirty;
  $("editorSize").textContent = formatBytes(new Blob([file.content]).size);
  updateGutter();
  updateCursor();
  syncTabDirty();
}

function syncTabDirty() {
  const dot = document.querySelector(".tab.active .tab-dirty");
  const file = activeFile();
  if (dot && file) dot.classList.toggle("hidden", file.content === file.original);
}

async function saveActiveFile() {
  const file = activeFile();
  if (!file || file.scratch) {
    if (file && file.scratch) toast("Scratch pads are not written to disk. Attach one to a chat instead.");
    return;
  }
  const result = await api("/api/workspace/file", {
    method: "PUT",
    body: JSON.stringify({ path: file.path, content: file.content }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not save the file.");
    return;
  }
  file.original = file.content;
  file.size = result.data.size;
  file.language = result.data.language || file.language;
  renderEditor();
  renderTabs();
  toast(`Saved ${basename(file.path)}.`);
  const parent = parentPath(file.path);
  if (state.tree.dirs.has(parent)) await refreshDir(parent);
}

function revertActiveFile() {
  const file = activeFile();
  if (!file) return;
  if (file.content === file.original) return;
  if (!confirm("Discard unsaved changes and restore the last saved version?")) return;
  file.content = file.original;
  if (file.scratch) file.content = "";
  renderEditor();
  renderTabs();
  toast("Changes discarded.");
}

function attachActiveToChat() {
  const file = activeFile();
  if (!file) {
    toast("Open a file first.");
    return;
  }
  state.chatContext = {
    path: file.scratch ? "Scratch pad" : file.path,
    language: file.language,
    content: file.content.slice(0, 50000),
    scratch: Boolean(file.scratch),
  };
  renderContextChip();
  activateTab(chatTabKey());
  toast(`${file.scratch ? "Scratch pad" : basename(file.path)} attached as chat context.`);
}

function renderContextChip() {
  const chip = $("contextChip");
  if (!state.chatContext) {
    chip.classList.add("hidden");
    updateContextMeter();
    return;
  }
  chip.classList.remove("hidden");
  const label = state.chatContext.path;
  $("contextLabel").textContent = label.length > 48 ? `…${label.slice(-46)}` : label;
  $("contextLabel").title = label;
  updateContextMeter();
}

function attachFromComposer() {
  const file = activeFile();
  if (file) {
    attachActiveToChat();
    return;
  }
  if (state.openTabs.length > 1) {
    toast("Activate a file tab to attach it.");
    return;
  }
  toast("Open a file or scratch pad to attach as context.");
}

/* ------------------------------------------------------------------ */
/* dialogs                                                             */
/* ------------------------------------------------------------------ */

function showModal(element) {
  element.classList.remove("hidden");
  const focusable = element.querySelector("input, button");
  if (focusable) setTimeout(() => focusable.focus(), 20);
}

function hideModal(element) {
  element.classList.add("hidden");
}

let nameResolver = null;

function askName(options) {
  const { title, label, value = "", placeholder = "", submitLabel = "OK", hint = "" } = options;
  $("promptTitle").textContent = title;
  $("promptLabel").textContent = label;
  $("promptInput").value = value;
  $("promptInput").placeholder = placeholder;
  $("promptSubmit").textContent = submitLabel;
  const hintBox = $("promptHint");
  hintBox.textContent = hint;
  hintBox.classList.toggle("hidden", !hint);
  $("promptMessage").className = "notice hidden";
  showModal($("promptModal"));
  setTimeout(() => {
    $("promptInput").focus();
    const dot = value.lastIndexOf(".");
    $("promptInput").setSelectionRange(0, dot > 0 ? dot : value.length);
  }, 30);
  return new Promise((resolve) => { nameResolver = resolve; });
}

function settleName(value) {
  const resolver = nameResolver;
  nameResolver = null;
  hideModal($("promptModal"));
  if (resolver) resolver(value);
}

/* ------------------------------------------------------------------ */
/* settings + usage                                                    */
/* ------------------------------------------------------------------ */

async function loadSettings() {
  const result = await api("/api/config");
  if (!result.ok) return;
  $("baseUrl").value = result.data.base_url || "";
  $("model").value = result.data.model || "auto";
  state.model = result.data.model || "auto";
  state.contextBudget = Number(result.data.context_budget_tokens) || 6000;
  $("contextBudget").value = state.contextBudget;
  $("historyMessages").value = Number(result.data.history_messages) || 6;
  $("sendOutline").checked = result.data.send_workspace_outline !== false;
  updateContextMeter();
  $("modelChip").textContent = state.model;
  $("apiKey").value = "";
  $("apiKey").placeholder = result.data.has_api_key ? "Saved key (leave blank to keep it)" : "Paste your router API key";
  $("keyStatus").textContent = result.data.has_api_key
    ? "A key is saved locally. Leave the field blank to keep it, or enter a new key to replace it."
    : "No key saved yet. It is stored in a permission-restricted local config file.";
}

function settingsNotice(message, ok) {
  const box = $("settingsMessage");
  box.textContent = message;
  box.className = "notice " + (ok ? "success" : "error");
}

async function saveSettings(event) {
  if (event) event.preventDefault();
  const payload = {
    base_url: $("baseUrl").value.trim(),
    model: $("model").value.trim() || "auto",
    context_budget_tokens: Number($("contextBudget").value) || state.contextBudget || 6000,
    history_messages: Number($("historyMessages").value) || 6,
    send_workspace_outline: $("sendOutline").checked,
  };
  const key = $("apiKey").value.trim();
  if (key) payload.api_key = key;
  const result = await api("/api/config", { method: "POST", body: JSON.stringify(payload) });
  if (!result.ok) {
    settingsNotice(result.data.error || "Could not save settings.", false);
    return false;
  }
  state.model = result.data.model || payload.model;
  state.contextBudget = Number(result.data.context_budget_tokens) || payload.context_budget_tokens;
  $("modelChip").textContent = state.model;
  updateContextMeter();
  $("apiKey").value = "";
  $("apiKey").placeholder = "Saved key (leave blank to keep it)";
  settingsNotice("Settings saved locally.", true);
  setConnection("ok", "Settings saved");
  return true;
}

async function testConnection() {
  const saved = await saveSettings();
  if (!saved) return;
  const result = await api("/api/test", { method: "POST", body: "{}" });
  if (result.ok && result.data.ok) {
    settingsNotice(result.data.message, true);
    setConnection("ok", "Router connected");
    toast("Router connection successful.");
  } else {
    settingsNotice(result.data.error || "Connection test failed.", false);
    setConnection("bad", "Connection failed");
  }
}

function openProviderDashboard() {
  window.open("http://127.0.0.1:3001", "_blank", "noopener,noreferrer");
}

async function loadUsage() {
  const result = await api("/api/usage");
  if (!result.ok) return;
  const summary = result.data.summary || {};
  $("statRequests").textContent = summary.requests || 0;
  $("statSuccess").textContent = summary.successful || 0;
  $("statErrors").textContent = summary.failed || 0;
  $("statTokens").textContent = Number(summary.total_tokens || 0).toLocaleString();
  $("statLatency").textContent = summary.avg_latency_ms
    ? `${Math.round(summary.avg_latency_ms)} ms`
    : "—";

  const body = $("usageRows");
  body.replaceChildren();
  const requests = Array.isArray(result.data.requests) ? result.data.requests : [];
  if (!requests.length) {
    body.innerHTML = '<tr><td colspan="6" class="muted">No requests yet. Send a message to start recording usage.</td></tr>';
    return;
  }
  requests.forEach((request) => {
    const row = document.createElement("tr");
    const when = new Date(request.created_at * 1000).toLocaleString();
    const tokens = request.total_tokens == null ? "not reported" : Number(request.total_tokens).toLocaleString();
    const detail = request.status === "ok"
      ? (request.fallback_attempts ? `Fallback attempts: ${request.fallback_attempts}` : "Completed")
      : (request.error || "Request failed");
    [when,
      request.status,
      `${request.provider || "unknown"}${request.model ? ` · ${request.model}` : ""}`,
      request.latency_ms == null ? "—" : `${request.latency_ms} ms`,
      tokens,
      detail].forEach((value, index) => {
      const cell = document.createElement("td");
      cell.textContent = value || "—";
      if (index === 1) cell.className = request.status === "ok" ? "status-ok" : "status-error";
      if (index === 5) cell.className = "detail-cell";
      row.appendChild(cell);
    });
    body.appendChild(row);
  });
}

/* ------------------------------------------------------------------ */
/* context meter                                                       */
/* ------------------------------------------------------------------ */

function estimateTokensFromCharacters(characters) {
  return Math.ceil(Math.max(0, characters) / 4);
}

function updateContextMeter() {
  const meter = $("contextMeter");
  if (!meter) return;
  const budget = Number(state.contextBudget) || 6000;
  const typed = $("prompt") ? $("prompt").value.length : 0;
  const attached = state.chatContext ? state.chatContext.content.length : 0;
  const estimate = estimateTokensFromCharacters(typed + attached);
  const last = state.lastContextTokens;
  meter.textContent = last
    ? `~${formatCount(last)} / ${formatCount(budget)}`
    : `~${formatCount(estimate)} / ${formatCount(budget)}`;
  meter.title = last
    ? "Tokens the router reported for the last request (budget: " + formatCount(budget) + ")"
    : "Estimated tokens for the next request (budget: " + formatCount(budget) + ")";
  meter.classList.toggle("over", estimate > budget);
}

function formatCount(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "0";
  if (number >= 1000000) return `${(number / 1000000).toFixed(1)}M`;
  if (number >= 1000) return `${Math.round(number / 100) / 10}k`;
  return String(Math.round(number));
}

/* ------------------------------------------------------------------ */
/* router console                                                      */
/* ------------------------------------------------------------------ */

const consoleState = {
  tab: "keys",
  status: { reachable: false, authenticated: false },
  keys: [],
  models: [],
  routing: {},
};

function consoleArray(data, keys) {
  if (Array.isArray(data)) return data;
  if (!data || typeof data !== "object") return [];
  for (const key of keys) {
    if (Array.isArray(data[key])) return data[key];
  }
  return [];
}

function consoleText(value) {
  return value == null ? "" : String(value);
}

function consoleNumber(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function entryId(entry) {
  return entry && (entry.id ?? entry._id ?? entry.keyId ?? entry.key_id);
}

async function consoleApi(path, options = {}) {
  const result = await api(`/api/router/console/${path}`, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  if (result.status === 401) renderConsoleAuth(false, result.data.error || "Sign in to the router dashboard.");
  return result;
}

function renderConsoleAuth(authenticated, message) {
  $("consoleLogin").classList.toggle("hidden", authenticated);
  $("consoleDot").classList.toggle("ok", authenticated);
  $("consoleDot").classList.toggle("bad", !authenticated && consoleState.status.reachable);
  $("consoleState").textContent = authenticated
    ? "Console connected"
    : consoleState.status.reachable
      ? "Router needs a dashboard sign-in"
      : "Router is not running";
  $("consoleMessage").textContent = message || "";
}

async function loadConsoleStatus() {
  const result = await api("/api/router/console/status");
  if (!result.ok) {
    consoleState.status = { reachable: false, authenticated: false };
    renderConsoleAuth(false, result.data.error || "Could not read the router status.");
    return;
  }
  consoleState.status = result.data;
  renderConsoleAuth(Boolean(result.data.authenticated), result.data.message);
  if (result.data.authenticated) await loadConsolePane(consoleState.tab);
}

async function loadConsolePane(tab) {
  consoleState.tab = tab;
  document.querySelectorAll("[data-console]").forEach((button) => {
    const active = button.dataset.console === tab;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", String(active));
  });
  ["keys", "models", "routing", "analytics", "logs", "budget"].forEach((name) => {
    $(`consolePane${name.charAt(0).toUpperCase()}${name.slice(1)}`).classList.toggle("hidden", name !== tab);
  });
  if (!consoleState.status.authenticated) return;
  if (tab === "keys") await loadConsoleKeys();
  if (tab === "models") await loadConsoleModels();
  if (tab === "routing") await loadConsoleRouting();
  if (tab === "analytics") await loadConsoleAnalytics();
  if (tab === "logs") await loadConsoleLogs();
  if (tab === "budget") await loadConsoleBudget();
}

function consoleKeyMessage(message, ok) {
  const box = $("keyMessage");
  box.textContent = message;
  box.className = "notice " + (ok ? "success" : "error");
}

async function loadConsoleKeys() {
  const list = $("keyList");
  const result = await consoleApi("keys");
  if (!result.ok) {
    list.innerHTML = `<p class="muted">${escapeHtml(result.data.error || "Could not load provider keys.")}</p>`;
    return;
  }
  const keys = consoleArray(result.data, ["keys", "items", "data", "providers"]);
  consoleState.keys = keys;
  if (!keys.length) {
    list.innerHTML = '<p class="muted">No provider keys yet. Add one above to give the router free-tier capacity.</p>';
    return;
  }
  list.replaceChildren();
  keys.forEach((entry) => {
    const id = entryId(entry);
    const enabled = entry.enabled !== false && entry.disabled !== true;
    const card = document.createElement("div");
    card.className = "console-row" + (enabled ? "" : " off");
    const platform = consoleText(entry.platform || entry.provider || entry.name || "provider");
    const label = consoleText(entry.label || "");
    const masked = consoleText(entry.keyPreview || entry.maskedKey || entry.masked || (entry.key ? "••••" : ""));
    const capParts = [];
    if (entry.requestCap != null) capParts.push(`${formatCount(entry.requestCap)} req/mo`);
    if (entry.tokenCap != null) capParts.push(`${formatCount(entry.tokenCap)} tokens/mo`);
    if (entry.requests != null) capParts.push(`${formatCount(entry.requests)} used`);
    card.innerHTML =
      '<div class="console-row-main">' +
      `<strong>${escapeHtml(platform)}</strong>` +
      `${label ? `<span class="console-tag">${escapeHtml(label)}</span>` : ""}` +
      `${masked ? `<code>${escapeHtml(masked)}</code>` : ""}` +
      `<small>${escapeHtml(capParts.join(" · ") || (enabled ? "enabled" : "disabled"))}</small>` +
      "</div><div class=\"console-row-actions\"></div>";
    const actions = card.querySelector(".console-row-actions");
    if (id != null) {
      const toggle = document.createElement("button");
      toggle.className = "btn btn-quiet btn-small";
      toggle.textContent = enabled ? "Disable" : "Enable";
      toggle.addEventListener("click", async () => {
        toggle.disabled = true;
        const updated = await consoleApi(`keys/${encodeURIComponent(id)}`, {
          method: "PATCH",
          body: JSON.stringify({ enabled: !enabled }),
        });
        if (!updated.ok) consoleKeyMessage(updated.data.error || "Could not update that key.", false);
        await loadConsoleKeys();
      });
      actions.appendChild(toggle);

      const remove = document.createElement("button");
      remove.className = "btn btn-quiet btn-small danger";
      remove.textContent = "Delete";
      remove.addEventListener("click", async () => {
        if (!confirm(`Delete the ${platform} key? Providers using it will stop working.`)) return;
        remove.disabled = true;
        const deleted = await consoleApi(`keys/${encodeURIComponent(id)}`, { method: "DELETE" });
        if (!deleted.ok) consoleKeyMessage(deleted.data.error || "Could not delete that key.", false);
        await loadConsoleKeys();
      });
      actions.appendChild(remove);
    }
    list.appendChild(card);
  });
}

async function loadConsoleModels() {
  const result = await consoleApi("models");
  const list = $("modelList");
  if (!result.ok) {
    list.innerHTML = `<p class="muted">${escapeHtml(result.data.error || "Could not load models.")}</p>`;
    return;
  }
  consoleState.models = consoleArray(result.data, ["models", "items", "data"]);
  renderConsoleModels();
}

function renderConsoleModels() {
  const list = $("modelList");
  const filter = ($("modelSearch").value || "").trim().toLowerCase();
  const models = consoleState.models.filter((entry) => {
    const name = consoleText(entry.id || entry.model || entry.name).toLowerCase();
    const provider = consoleText(entry.platform || entry.provider || entry.owned_by).toLowerCase();
    return !filter || name.includes(filter) || provider.includes(filter);
  });
  if (!models.length) {
    list.innerHTML = '<p class="muted">No models to show. Add a provider key first, then reload.</p>';
    return;
  }
  list.replaceChildren();
  models.slice(0, 400).forEach((entry) => {
    const name = consoleText(entry.id || entry.model || entry.name || "model");
    const provider = consoleText(entry.platform || entry.provider || entry.owned_by || "");
    const status = consoleText(entry.execution_status || entry.status || (entry.available === false ? "needsKey" : "ready"));
    const row = document.createElement("div");
    row.className = "console-row";
    row.innerHTML =
      '<div class="console-row-main">' +
      `<strong>${escapeHtml(name)}</strong>` +
      `<small>${escapeHtml(provider)}${status ? ` · ${escapeHtml(status)}` : ""}</small>` +
      "</div><div class=\"console-row-actions\"></div>";
    const use = document.createElement("button");
    use.className = "btn btn-quiet btn-small";
    use.textContent = state.model === name ? "Current" : "Use";
    use.disabled = state.model === name;
    use.addEventListener("click", async () => {
      const result = await api("/api/config", { method: "POST", body: JSON.stringify({ model: name }) });
      if (!result.ok) {
        toast(result.data.error || "Could not set that model.");
        return;
      }
      state.model = name;
      $("model").value = name;
      $("modelChip").textContent = name;
      toast(`${name} is now the default model.`);
      renderConsoleModels();
    });
    row.querySelector(".console-row-actions").appendChild(use);
    list.appendChild(row);
  });
}

async function loadConsoleRouting() {
  const result = await consoleApi("fallback/routing");
  const raw = $("routingRaw");
  if (!result.ok) {
    raw.textContent = result.data.error || "Could not read routing settings.";
    return;
  }
  consoleState.routing = result.data && typeof result.data === "object" ? result.data : {};
  const routing = consoleState.routing;
  const strategy = consoleText(routing.strategy || routing.routingStrategy || "");
  if (strategy) {
    const option = [...$("routingStrategy").options].find((item) => item.value === strategy);
    if (option) $("routingStrategy").value = strategy;
    else {
      const custom = document.createElement("option");
      custom.value = strategy;
      custom.textContent = strategy;
      $("routingStrategy").appendChild(custom);
      $("routingStrategy").value = strategy;
    }
  }
  const chain = consoleArray(routing, ["chain", "models", "order", "fallbackChain", "queue"]);
  $("routingChain").value = chain
    .map((entry) => (typeof entry === "string" ? entry : consoleText(entry.model || entry.id || entry.name)))
    .filter(Boolean)
    .join(" → ");
  raw.textContent = JSON.stringify(routing, null, 2);
}

async function saveConsoleRouting() {
  const strategy = $("routingStrategy").value;
  const merged = Object.assign({}, consoleState.routing, { strategy });
  const result = await consoleApi("fallback/routing", { method: "PUT", body: JSON.stringify(merged) });
  const box = $("routingMessage");
  if (!result.ok) {
    box.textContent = result.data.error || "Could not save the routing strategy.";
    box.className = "notice error";
    return;
  }
  box.textContent = `Routing strategy saved as ${strategy}.`;
  box.className = "notice success";
  await loadConsoleRouting();
}

async function loadConsoleAnalytics() {
  const range = $("analyticsRange").value;
  const summary = await consoleApi(`analytics/summary?range=${encodeURIComponent(range)}`);
  if (summary.ok) {
    const data = summary.data || {};
    const requests = consoleNumber(data.totalRequests ?? data.requests);
    const rate = data.successRate;
    $("raRequests").textContent = formatCount(requests);
    $("raSuccess").textContent =
      rate == null ? "—" : `${(consoleNumber(rate) <= 1 ? consoleNumber(rate) * 100 : consoleNumber(rate)).toFixed(1)}%`;
    $("raInput").textContent = formatCount(data.totalInputTokens ?? data.inputTokens);
    $("raOutput").textContent = formatCount(data.totalOutputTokens ?? data.outputTokens);
    $("raP95").textContent = data.p95LatencyMs == null ? "—" : `${Math.round(consoleNumber(data.p95LatencyMs))} ms`;
  }

  const byModel = await consoleApi(`analytics/by-model?range=${encodeURIComponent(range)}`);
  const body = $("analyticsModels");
  const rows = byModel.ok ? consoleArray(byModel.data, ["models", "rows", "byModel", "data"]) : [];
  if (!rows.length) {
    body.innerHTML = '<tr><td colspan="4" class="muted">No requests in this range.</td></tr>';
    return;
  }
  body.replaceChildren();
  rows.slice(0, 100).forEach((entry) => {
    const row = document.createElement("tr");
    [
      consoleText(entry.model || entry.name || entry.id),
      formatCount(entry.requests ?? entry.count),
      formatCount(entry.failures ?? entry.errors),
      entry.avgLatencyMs == null ? "—" : `${Math.round(consoleNumber(entry.avgLatencyMs))} ms`,
    ].forEach((value) => {
      const cell = document.createElement("td");
      cell.textContent = value || "—";
      row.appendChild(cell);
    });
    body.appendChild(row);
  });
}

async function loadConsoleLogs() {
  const level = $("logLevel").value;
  const query = `logs?limit=150${level ? `&levels=${encodeURIComponent(level)}` : ""}`;
  const result = await consoleApi(query);
  const list = $("logList");
  if (!result.ok) {
    list.innerHTML = `<p class="muted">${escapeHtml(result.data.error || "Could not load logs.")}</p>`;
    return;
  }
  const entries = consoleArray(result.data, ["entries", "logs", "data"]);
  if (!entries.length) {
    list.innerHTML = '<p class="muted">No log entries yet.</p>';
    return;
  }
  list.replaceChildren();
  entries.forEach((entry) => {
    const row = document.createElement("div");
    row.className = "log-row";
    const levelName = consoleText(entry.level || "info").toLowerCase();
    const when = entry.ts ? new Date(entry.ts).toLocaleTimeString() : "";
    row.innerHTML =
      `<span class="log-level ${escapeHtml(levelName)}">${escapeHtml(levelName)}</span>` +
      `<span class="log-time">${escapeHtml(when)}</span>` +
      `<span class="log-text">${escapeHtml(consoleText(entry.message || entry.msg || ""))}</span>`;
    list.appendChild(row);
  });
}

async function loadConsoleBudget() {
  const result = await consoleApi("free-tier");
  const rows = $("budgetRows");
  if (!result.ok) {
    rows.innerHTML = `<tr><td colspan="5" class="muted">${escapeHtml(result.data.error || "Could not read the free-tier budget.")}</td></tr>`;
    return;
  }
  const summary = result.data.summary || {};
  $("fbPools").textContent = formatCount(summary.poolCount);
  $("fbTokens").textContent = formatCount(summary.documentedMonthlyTokens);
  $("fbCredits").textContent = formatCount(summary.creditsBasedPools);
  const pools = consoleArray(result.data, ["pools", "data"]);
  if (!pools.length) {
    rows.innerHTML = '<tr><td colspan="5" class="muted">No provider pools reported.</td></tr>';
    return;
  }
  rows.replaceChildren();
  pools.slice(0, 100).forEach((pool) => {
    const row = document.createElement("tr");
    const quota = pool.quota || {};
    [
      consoleText(pool.platform || pool.poolKey),
      formatCount(pool.modelCount),
      formatCount(pool.keyCount),
      consoleText(pool.bestLabel) || formatCount(pool.documentedBudget),
      quota.remaining == null ? "—" : formatCount(quota.remaining),
    ].forEach((value) => {
      const cell = document.createElement("td");
      cell.textContent = value || "—";
      row.appendChild(cell);
    });
    rows.appendChild(row);
  });
}

/* ------------------------------------------------------------------ */
/* events + init                                                       */
/* ------------------------------------------------------------------ */

function bindEvents() {
  $("modeThreads").addEventListener("click", () => setSidebarMode("threads"));
  $("modeFiles").addEventListener("click", () => setSidebarMode("files"));
  $("modeSkills").addEventListener("click", () => { setSidebarMode("skills"); loadSkills(); });
  $("newChat").addEventListener("click", newConversation);
  $("projectSwitcher").addEventListener("click", (event) => {
    event.stopPropagation();
    if ($("projectMenu").classList.contains("hidden")) openProjectMenu();
    else closeProjectMenu();
  });
  $("projectNew").addEventListener("click", () => {
    closeProjectMenu();
    createProject();
  });
  $("skillsOpenFolder").addEventListener("click", () => setView("skills"));
  $("skillBack").addEventListener("click", () => {
    state.openSkillId = null;
    renderSkills();
  });
  document.addEventListener("click", (event) => {
    // Close the project menu on any outside click (the switcher stops its own).
    if (!$("projectMenu").classList.contains("hidden") && !$("projectMenu").contains(event.target)) {
      closeProjectMenu();
    }
  });
  $("usageBtn").addEventListener("click", () => setView("usage"));
  $("settingsBtn").addEventListener("click", () => setView("settings"));
  $("refreshUsage").addEventListener("click", loadUsage);

  $("sidebarToggle").addEventListener("click", () => {
    const open = document.body.classList.toggle("nav-open");
    $("scrim").classList.toggle("hidden", !open);
  });
  $("scrim").addEventListener("click", () => {
    document.body.classList.remove("nav-open");
    $("scrim").classList.add("hidden");
  });

  $("sendBtn").addEventListener("click", () => sendMessage());
  $("stopStream").addEventListener("click", stopStream);
  $("prompt").addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      sendMessage();
    }
  });
  $("prompt").addEventListener("input", () => { autoGrow($("prompt")); updateContextMeter(); });
  $("attachContextBtn").addEventListener("click", attachFromComposer);
  $("contextClear").addEventListener("click", () => {
    state.chatContext = null;
    renderContextChip();
    updateContextMeter();
  });
  document.querySelectorAll("[data-prompt]").forEach((button) => {
    button.addEventListener("click", () => {
      $("prompt").value = button.dataset.prompt;
      autoGrow($("prompt"));
      $("prompt").focus();
    });
  });

  $("openFolderBtn").addEventListener("click", openFolderDialog);
  $("scratchBtn").addEventListener("click", openScratchPad);
  $("wsClose").addEventListener("click", closeWorkspace);
  $("folderCancel").addEventListener("click", () => hideModal($("folderModal")));
  $("folderCancelIcon").addEventListener("click", () => hideModal($("folderModal")));
  $("folderHome").addEventListener("click", () => { $("folderInput").value = state.home; });
  $("folderBrowse").addEventListener("click", async () => {
    if (!closeAllFileTabs()) return;
    showFolderMessage("Opening the system folder dialog…", true);
    const result = await api("/api/workspace/pick", { method: "POST", body: "{}" });
    if (!result.ok) {
      showFolderMessage(result.data.error || "Could not open the folder dialog.", false);
      return;
    }
    if (result.data.picked) {
      await applyWorkspacePayload(result.data);
      return;
    }
    $("folderMessage").className = "notice hidden";
    if (result.data.message) showFolderMessage(result.data.message, false);
  });
  $("folderOpen").addEventListener("click", () => {
    const path = $("folderInput").value.trim();
    if (!path) {
      showFolderMessage("Enter the absolute path of a folder.", false);
      return;
    }
    openWorkspacePath(path);
  });
  $("folderInput").addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      $("folderOpen").click();
    }
  });

  $("newFileBtn").addEventListener("click", () => {
    const selected = state.tree.selected;
    const base = selected ? (selected.dir ? selected.path : parentPath(selected.path)) : "";
    createItem({ isDir: false, base });
  });
  $("newFolderBtn").addEventListener("click", () => {
    const selected = state.tree.selected;
    const base = selected ? (selected.dir ? selected.path : parentPath(selected.path)) : "";
    createItem({ isDir: true, base });
  });
  $("renameBtn").addEventListener("click", renameSelected);
  $("deleteBtn").addEventListener("click", deleteSelected);

  $("saveBtn").addEventListener("click", saveActiveFile);
  $("revertBtn").addEventListener("click", revertActiveFile);
  $("attachFileBtn").addEventListener("click", attachActiveToChat);
  $("copyPathBtn").addEventListener("click", () => {
    const file = activeFile();
    if (file) copyText(file.scratch ? "Scratch pad" : file.path, "Path copied.");
  });
  $("codeArea").addEventListener("input", onEditorInput);
  $("codeArea").addEventListener("keydown", (event) => {
    if (event.key === "Tab") {
      event.preventDefault();
      const area = $("codeArea");
      const start = area.selectionStart;
      const end = area.selectionEnd;
      if (event.shiftKey) {
        const before = area.value.slice(0, start);
        const remove = before.endsWith("  ") ? 2 : before.endsWith(" ") ? 1 : 0;
        if (remove) area.value = area.value.slice(0, start - remove) + area.value.slice(start);
        area.selectionStart = area.selectionEnd = start - remove;
      } else {
        area.value = `${area.value.slice(0, start)}  ${area.value.slice(end)}`;
        area.selectionStart = area.selectionEnd = start + 2;
      }
      onEditorInput();
      return;
    }
    if (event.key === "s" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      // The document-level shortcut also listens for Ctrl+S; stop the bubbling
      // duplicate so a save is never issued twice.
      event.stopPropagation();
      saveActiveFile();
    }
  });
  ["click", "keyup"].forEach((type) => $("codeArea").addEventListener(type, updateCursor));
  $("codeArea").addEventListener("scroll", () => { $("gutter").scrollTop = $("codeArea").scrollTop; });

  $("messages").addEventListener("click", (event) => {
    const button = event.target.closest("[data-copy-code]");
    if (!button) return;
    const block = button.closest(".code-block");
    const code = block ? block.querySelector("pre code") : null;
    if (code) copyText(code.textContent, "Code copied.");
  });

  $("promptCancel").addEventListener("click", () => settleName(null));
  $("promptCancelIcon").addEventListener("click", () => settleName(null));
  $("promptSubmit").addEventListener("click", () => settleName($("promptInput").value.trim() || null));
  $("promptInput").addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      settleName($("promptInput").value.trim() || null);
    }
  });

  $("settingsForm").addEventListener("submit", saveSettings);
  $("testConnection").addEventListener("click", testConnection);
  $("openProviderDashboardSettings").addEventListener("click", openProviderDashboard);

  $("routerBtn").addEventListener("click", () => setView("router"));
  $("consoleRefresh").addEventListener("click", loadConsoleStatus);
  $("consoleDashboard").addEventListener("click", openProviderDashboard);
  $("consoleStart").addEventListener("click", async () => {
    $("consoleState").textContent = "Starting the router…";
    const result = await api("/api/router/start", { method: "POST", body: "{}" });
    if (!result.ok) $("consoleMessage").textContent = result.data.error || "Could not start the router.";
    await loadConsoleStatus();
  });
  $("consoleStop").addEventListener("click", async () => {
    $("consoleState").textContent = "Stopping the router…";
    const result = await api("/api/router/stop", { method: "POST", body: "{}" });
    if (!result.ok) $("consoleMessage").textContent = result.data.error || "Could not stop the router.";
    await loadConsoleStatus();
  });
  $("consoleLogin").addEventListener("submit", async (event) => {
    event.preventDefault();
    const email = $("consoleEmail").value.trim();
    const password = $("consolePassword").value;
    if (!email || !password) return;
    const result = await api("/api/router/console/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    });
    if (!result.ok) {
      renderConsoleAuth(false, result.data.error || "Sign-in failed.");
      return;
    }
    $("consolePassword").value = "";
    await loadConsoleStatus();
  });
  document.querySelectorAll("[data-console]").forEach((button) => {
    button.addEventListener("click", () => loadConsolePane(button.dataset.console));
  });
  $("keyForm").addEventListener("submit", async (event) => {
    event.preventDefault();
    const platform = $("keyPlatform").value.trim();
    if (!platform) return;
    const payload = { platform };
    const key = $("keyValue").value.trim();
    const label = $("keyLabel").value.trim();
    if (key) payload.key = key;
    if (label) payload.label = label;
    const result = await consoleApi("keys", { method: "POST", body: JSON.stringify(payload) });
    if (!result.ok) {
      consoleKeyMessage(result.data.error || "Could not add that key.", false);
      return;
    }
    $("keyValue").value = "";
    $("keyLabel").value = "";
    consoleKeyMessage(`${platform} key added.`, true);
    await loadConsoleKeys();
  });
  $("modelSearch").addEventListener("input", renderConsoleModels);
  $("modelReload").addEventListener("click", loadConsoleModels);
  $("routingReload").addEventListener("click", loadConsoleRouting);
  $("routingSave").addEventListener("click", saveConsoleRouting);
  $("analyticsReload").addEventListener("click", loadConsoleAnalytics);
  $("analyticsRange").addEventListener("change", loadConsoleAnalytics);
  $("logsReload").addEventListener("click", loadConsoleLogs);
  $("logLevel").addEventListener("change", loadConsoleLogs);
  $("logsClear").addEventListener("click", async () => {
    if (!confirm("Clear the router log buffer?")) return;
    const result = await consoleApi("logs/clear", { method: "POST", body: "{}" });
    if (!result.ok) toast(result.data.error || "Could not clear the logs.");
    await loadConsoleLogs();
  });
  $("budgetReload").addEventListener("click", loadConsoleBudget);

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      if (!$("promptModal").classList.contains("hidden")) settleName(null);
      if (!$("folderModal").classList.contains("hidden")) hideModal($("folderModal"));
      document.body.classList.remove("nav-open");
      $("scrim").classList.add("hidden");
    }
    if (event.key === "s" && (event.ctrlKey || event.metaKey) && state.view === "editor") {
      event.preventDefault();
      saveActiveFile();
    }
  });

  // The VS Code extension frames this UI and forwards editor selections into
  // it. Only accept that message inside a frame, and only from the extension's
  // own wrapper document.
  window.addEventListener("message", (event) => {
    const data = event.data;
    if (!data || data.type !== "freeai:prefill" || data.source !== "free-ai-studio-extension") return;
    if (window.parent === window) return;
    if (data.prompt) {
      $("prompt").value = String(data.prompt);
      autoGrow($("prompt"));
    }
    if (data.code) {
      state.chatContext = {
        path: `selection (${data.language || "text"})`,
        scratch: true,
        content: String(data.code),
        language: String(data.language || "text"),
      };
      renderContextChip();
    }
    updateContextMeter();
    $("prompt").focus();
  });

  window.addEventListener("beforeunload", (event) => {
    const dirty = [...state.files.values()].filter((file) => file.content !== file.original);
    if (dirty.length) {
      event.preventDefault();
      event.returnValue = "";
    }
  });
}

(async function init() {
  loadTheme();
  setupThemeToggle();
  bindEvents();
  setSidebarMode("threads");
  setView("chat");
  autoGrow($("prompt"));
  renderTabs();
  renderContextChip();
  state.lastContextTokens = null;
  setTimeout(updateContextMeter, 50);
  await Promise.all([loadSettings(), loadProjects(), loadConversations(), loadWorkspace()]);
  await Promise.all([loadUsage(), loadSkills()]);
  if (state.conversations.length) {
    await openConversation(state.conversations[0].id);
  }
  if (state.workspace) setSidebarMode("files");
})();
