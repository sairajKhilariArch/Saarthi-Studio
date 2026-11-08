"use strict";

/* Saarthi Studio — local-first coding agent UI. */

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

/* ---- settings: section navigation + appearance ------------------- */

function setSettingsSection(name) {
  let found = false;
  document.querySelectorAll(".settings-section").forEach((section) => {
    const active = section.id === `settings-${name}`;
    section.classList.toggle("hidden", !active);
    if (active) found = true;
  });
  if (!found) return;
  $("settingsNav").querySelectorAll("button").forEach((button) => {
    const active = button.dataset.settingsSection === name;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", String(active));
  });
  // Rules change while the app runs (an agent refusal, a router install), so
  // the panel re-reads both endpoints on every visit instead of caching.
  if (name === "permissions") void loadPermissions();
  if (name === "mcp") void loadMcp();
  if (name === "diagnostics") void loadDiagnostics();
  try { localStorage.setItem("freeai-settings-section", name); } catch { /* private mode */ }
}

const ACCENTS = [
  { id: "classic", label: "Classic orange", color: "#ff8b3d" },
  { id: "black", label: "Black", color: "#e6e6ee" },
  { id: "blue", label: "Blue", color: "#4f8cff" },
  { id: "green", label: "Green", color: "#34d399" },
  { id: "red", label: "Red", color: "#ff5d5d" },
  { id: "white", label: "White", color: "#f4f4f8" },
];

function applyAccent(name) {
  const swatch = ACCENTS.find((a) => a.id === name);
  if (name === "classic" || !swatch) {
    delete document.documentElement.dataset.accent;
    try { localStorage.setItem("freeai-accent", ""); } catch { /* private mode */ }
    return;
  }
  document.documentElement.dataset.accent = name;
  try { localStorage.setItem("freeai-accent", name); } catch { /* private mode */ }
}

function renderAccentPicker() {
  const host = $("accentPicker");
  if (!host) return;
  host.replaceChildren();
  const current = localStorage.getItem("freeai-accent") || "classic";
  ACCENTS.forEach((accent) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "choice-btn accent-swatch";
    button.dataset.accent = accent.id;
    button.setAttribute("role", "option");
    button.title = accent.label;
    button.setAttribute("aria-selected", String(current === accent.id));
    const dot = document.createElement("span");
    dot.className = "accent-dot";
    dot.style.background = accent.color;
    const label = document.createElement("span");
    label.textContent = accent.label;
    button.append(dot, label);
    button.classList.toggle("active", current === accent.id);
    button.addEventListener("click", () => applyAccent(accent.id));
    host.appendChild(button);
  });
}

function setupSettingsNav() {
  $("settingsNav").addEventListener("click", (event) => {
    const button = event.target.closest("button[data-settings-section]");
    if (button) setSettingsSection(button.dataset.settingsSection);
  });
  $("permAdd").addEventListener("click", () => void addPermissionRule());
  $("permRefresh").addEventListener("click", () => void loadPermissions());
  // The MCP panel points at Permissions where a rule would go; the link moves
  // the panel instead of navigating away from the app.
  $("settings-mcp").addEventListener("click", (event) => {
    const link = event.target.closest("[data-goto-settings]");
    if (!link) return;
    event.preventDefault();
    setSettingsSection(link.dataset.gotoSettings);
  });
  $("mcpSave").addEventListener("click", () => void saveMcpServer());
  $("mcpRefresh").addEventListener("click", () => void loadMcp());
  $("mcpCall").addEventListener("click", () => void callMcpTool());
  $("mcpHealth").addEventListener("click", () => void checkMcpHealth());
  $("diagRefresh").addEventListener("click", () => void loadDiagnostics());
  $("diagBundle").addEventListener("click", () => void downloadDiagnosticsBundle());
  $("diagArea").addEventListener("change", () => void loadDiagnostics());
  $("themeLightBtn").addEventListener("click", () => applyTheme("light"));
  $("themeDarkBtn").addEventListener("click", () => applyTheme("dark"));
  // The theme buttons reflect the live theme without a re-render: listening on
  // the theme manager's storage key would be enough, but a MutationObserver
  // on <html data-theme> is the one-liner that covers every writer.
  new MutationObserver(() => {
    const theme = document.documentElement.dataset.theme;
    $("themeLightBtn").classList.toggle("active", theme === "light");
    $("themeDarkBtn").classList.toggle("active", theme === "dark");
  }).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  // applyAccent mutates data-accent, which the accent buttons mirror below.
  new MutationObserver(() => renderAccentPicker()).observe(
    document.documentElement, { attributes: true, attributeFilter: ["data-accent"] }
  );
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
    item.querySelector("small").textContent = [
      project.folder_path || "no folder",
      project.model ? `model: ${project.model}` : "",
    ].filter(Boolean).join(" · ");
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
    // Per-project model default: this projects's turns use it unless the
    // composer or an agent run names a model of its own.
    const model = document.createElement("button");
    model.className = "tree-tool";
    model.textContent = project.model ? `Model: ${project.model}` : "Set model";
    model.addEventListener("click", (event) => {
      event.stopPropagation();
      setProjectModel(project);
    });
    const remove = document.createElement("button");
    remove.className = "tree-tool danger";
    remove.textContent = "Delete";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      deleteProject(project);
    });
    actions.append(rename, model, archive, remove);
    item.appendChild(actions);
    // The item itself switches the project — previously only the action
    // buttons worked, so the picker looked broken.
    item.addEventListener("click", () => {
      if (project.id === state.activeProjectId) {
        closeProjectMenu();
        return;
      }
      switchProject(project.id);
    });
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
  // The file tree and terminal follow the project's own folder, so operations
  // never touch one project's files while another's chat is open.
  if (project && project.folder_path && (!state.workspace || state.workspace.absolute_path !== project.folder_path)) {
    const opened = await api("/api/workspace/open", {
      method: "POST",
      body: JSON.stringify({ path: project.folder_path }),
    });
    if (opened.ok && opened.data.ok) {
      await loadWorkspace();
    } else {
      toast(opened.data.error || "Could not open that project's folder.");
    }
  }
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

/* Empty input clears the default so the project inherits again. */
async function setProjectModel(project) {
  const model = await askName({
    title: "Project model",
    label: "Model id (leave blank to inherit the agent and global defaults)",
    value: project.model || "",
    placeholder: "auto",
    submitLabel: "Save",
  });
  if (model == null) return;
  await updateProject(project, { model: model.trim() });
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
  wireSkillInstall();
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
  if ($("skillMenu") && !$("skillMenu").classList.contains("hidden")) renderSkillMenu();
  renderSkillChip();
}

/* ------------------------------------------------------------------ */
/* memory: records the user asked the project to keep                   */
/* ------------------------------------------------------------------ */

async function loadMemory() {
  wireMemoryForm();
  const box = $("memoryList");
  const result = await api("/api/memory");
  if (!result.ok) {
    box.replaceChildren();
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = result.data.error || "Memory could not be read.";
    box.appendChild(note);
    return;
  }
  state.memory = Array.isArray(result.data.records) ? result.data.records : [];
  renderMemory();
}

function renderMemory() {
  const box = $("memoryList");
  box.replaceChildren();
  const records = state.memory || [];
  if (!records.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "Nothing remembered yet. A record reaches a message only when they share words.";
    box.appendChild(note);
    return;
  }
  records.forEach((record) => {
    const card = document.createElement("div");
    card.className = "skill-card";
    card.innerHTML = '<span class="skill-card-text"><strong></strong><small></small></span>';
    card.querySelector("strong").textContent = record.title;
    const scope = record.global ? "every project" : "this project";
    card.querySelector("small").textContent = `${record.kind} · ${scope} · used ${record.uses || 0}×`;
    card.title = record.body || record.title;
    const forget = document.createElement("button");
    forget.type = "button";
    forget.className = "icon-btn tiny";
    forget.textContent = "×";
    forget.title = "Forget this record";
    forget.addEventListener("click", async (event) => {
      event.stopPropagation();
      const result = await api(`/api/memory/${encodeURIComponent(record.id)}/delete`, { method: "POST" });
      if (result.ok) loadMemory();
      else alert(result.data.error || "Could not forget that record.");
    });
    card.appendChild(forget);
    box.appendChild(card);
  });
}

function wireMemoryForm() {
  if (state.memoryWired) return;
  state.memoryWired = true;
  const save = async () => {
    const title = $("memoryTitle").value.trim();
    if (!title) {
      $("memoryTitle").focus();
      return;
    }
    const body = { kind: $("memoryKind").value, title, body: $("memoryBody").value };
    // An absent project means "the one I am working in"; empty means every one.
    if ($("memoryScope").value === "global") body.project = "";
    const result = await api("/api/memory", { method: "POST", body: JSON.stringify(body) });
    if (!result.ok) {
      alert(result.data.error || "Could not save that record.");
      return;
    }
    $("memoryTitle").value = "";
    $("memoryBody").value = "";
    loadMemory();
  };
  $("memorySave").addEventListener("click", save);
  $("memoryTitle").addEventListener("keydown", (event) => {
    if (event.key === "Enter") save();
  });
}

/* ------------------------------------------------------------------ */
/* code index                                                          */
/* ------------------------------------------------------------------ */

async function loadIndex() {
  const label = $("indexStatus");
  const result = await api("/api/index");
  if (!result.ok) {
    label.textContent = result.data.error || "The index could not be read.";
    return;
  }
  const files = Number(result.data.files || 0);
  const symbols = Number(result.data.symbols || 0);
  const languages = Array.isArray(result.data.languages) ? result.data.languages : [];
  label.textContent = `${files} file${files === 1 ? "" : "s"} · ${symbols} symbol${symbols === 1 ? "" : "s"}`;
  // The index says what it holds, and which languages it found it in, so an empty
  // index looks empty instead of broken.
  label.title = languages.length
    ? languages.map((entry) => `${entry.language}: ${entry.files}`).join(", ")
    : "Refresh to walk this folder.";
  wireIndex();
}

function renderIndexNote(text) {
  const box = $("indexResults");
  box.replaceChildren();
  const note = document.createElement("p");
  note.className = "empty-note";
  note.textContent = text;
  box.appendChild(note);
}

async function runIndexSearch(semantic) {
  const query = $("indexQuery").value.trim();
  if (!query) {
    $("indexQuery").focus();
    return;
  }
  const button = semantic ? $("indexMeaningBtn") : $("indexWordsBtn");
  button.disabled = true;
  const result = await api(`/api/index/${semantic ? "semantic" : "search"}?q=${encodeURIComponent(query)}`);
  button.disabled = false;
  if (!result.ok) {
    renderIndexNote(result.data.error || "The search failed.");
    return;
  }
  const hits = Array.isArray(result.data.hits) ? result.data.hits : [];
  if (!hits.length) {
    renderIndexNote(semantic
      ? "Nothing matched by meaning. A large index may still be embedding — search again in a moment."
      : "Nothing matched those words.");
    return;
  }
  const box = $("indexResults");
  box.replaceChildren();
  // A semantic pass that has not embedded everything says so, so a partial
  // ranking is never mistaken for a complete one.
  const header = document.createElement("p");
  header.className = "empty-note";
  header.textContent = semantic
    ? `${hits.length} by meaning${result.data.pending ? ` · ${result.data.embedded} embedded, ${result.data.pending} still to embed` : ""}`
    : `${hits.length} by words`;
  box.appendChild(header);
  hits.forEach((hit) => {
    const card = document.createElement("div");
    card.className = "skill-card";
    card.innerHTML = '<span class="skill-card-text"><strong></strong><small></small></span>';
    card.querySelector("strong").textContent = hit.name || hit.path;
    card.querySelector("small").textContent = `${hit.path}:${hit.line} · ${hit.kind} · ${hit.score}`;
    card.title = "Open this file";
    card.addEventListener("click", () => { openFile(hit.path); });
    box.appendChild(card);
  });
}

async function refreshIndex() {
  const button = $("indexRefreshBtn");
  button.disabled = true;
  const result = await api("/api/index/refresh", { method: "POST" });
  button.disabled = false;
  if (!result.ok) {
    renderIndexNote(result.data.error || "The index could not be refreshed.");
    return;
  }
  await loadIndex();
  const skipped = (result.data.skipped_secret || 0) + (result.data.skipped_binary || 0)
    + (result.data.skipped_oversize || 0) + (result.data.skipped_excluded || 0);
  toast(`Index refreshed: ${result.data.added || 0} added, ${result.data.updated || 0} updated, ${result.data.removed || 0} removed${skipped ? `, ${skipped} skipped` : ""}.`);
}

async function saveEmbeddingModel() {
  const model = $("indexEmbeddingModel").value.trim();
  const result = await api("/api/config", { method: "POST", body: JSON.stringify({ embedding_model: model }) });
  if (!result.ok) {
    renderIndexNote(result.data.error || "The embedding model could not be saved.");
    return;
  }
  toast(model ? `Search by meaning uses ${model}.` : "Search by meaning is off: no embedding model set.");
}

function wireIndex() {
  if (state.indexWired) return;
  state.indexWired = true;
  $("indexWordsBtn").addEventListener("click", () => runIndexSearch(false));
  $("indexMeaningBtn").addEventListener("click", () => runIndexSearch(true));
  $("indexRefreshBtn").addEventListener("click", refreshIndex);
  $("indexEmbeddingSave").addEventListener("click", saveEmbeddingModel);
  $("indexQuery").addEventListener("keydown", (event) => {
    if (event.key === "Enter") runIndexSearch(false);
  });
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

async function openSkillDetail(id) {
  state.openSkillId = id;
  state.skillDetail = null;
  renderSkillDetail();
  // The list shows a summary; the editor needs the file's real instructions.
  const result = await api(`/api/skills/${encodeURIComponent(id)}`);
  if (state.openSkillId !== id) return;
  if (result.ok && result.data.skill) {
    state.skillDetail = result.data.skill;
    renderSkillDetail();
  }
}

function skillUsageLine(skill) {
  if (!skill.uses) return "never used";
  const when = skill.last_used_at ? new Date(skill.last_used_at * 1000).toLocaleDateString() : "";
  return `used ${skill.uses} time${skill.uses === 1 ? "" : "s"}${when ? `, last ${when}` : ""}`;
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
  meta.className = "skill-meta";
  const facts = [skill.enabled ? "Enabled" : "Disabled", skill.source || "local"];
  if (skill.version) facts.push(`v${skill.version}`);
  facts.push(skillUsageLine(skill));
  if (skill.modified) facts.push("edited since install");
  meta.textContent = facts.join(" · ");
  meta.title = skill.path || "";
  body.appendChild(meta);
  if (skill.description) {
    const desc = document.createElement("p");
    desc.textContent = skill.description;
    body.appendChild(desc);
  }
  if (!skill.writable || skill.last_error) {
    const note = document.createElement("p");
    note.className = "muted";
    note.textContent = skill.last_error
      ? "This skill could not be loaded, so there is nothing to edit."
      : "This skill is not in your user skills folder, so it is read-only here; edit its SKILL.md where it lives.";
    body.appendChild(note);
    return;
  }
  const label = document.createElement("label");
  label.className = "field";
  const caption = document.createElement("span");
  caption.textContent = "Instructions";
  const area = document.createElement("textarea");
  area.rows = 10;
  const loaded = state.skillDetail && state.skillDetail.id === skill.id;
  area.value = loaded ? state.skillDetail.body || "" : "";
  area.placeholder = loaded ? "" : "Loading instructions…";
  label.append(caption, area);
  body.appendChild(label);
  const actions = document.createElement("div");
  actions.className = "form-actions";
  const save = document.createElement("button");
  save.type = "button";
  save.className = "btn btn-accent btn-small";
  save.textContent = "Save instructions";
  save.addEventListener("click", () => saveSkillInstructions(skill.id, area.value));
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "btn btn-quiet btn-small danger";
  remove.textContent = "Remove skill";
  remove.addEventListener("click", () => removeSkill(skill.id));
  actions.append(save, remove);
  body.appendChild(actions);
  const note = document.createElement("p");
  note.className = "muted";
  note.textContent = "Usage is recorded whenever this skill rides along with a message.";
  body.appendChild(note);
}

async function installSkill() {
  const name = $("skillInstallName").value.trim();
  const instructions = $("skillInstallBody").value.trim();
  if (!name || !instructions) {
    toast("A skill needs a name and instructions.");
    return;
  }
  const result = await api("/api/skills/install", {
    method: "POST",
    confirmApproval: true,
    body: JSON.stringify({
      name,
      version: $("skillInstallVersion").value.trim(),
      description: $("skillInstallDescription").value.trim(),
      instructions,
    }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not install that skill.");
    return;
  }
  $("skillInstallName").value = "";
  $("skillInstallVersion").value = "";
  $("skillInstallDescription").value = "";
  $("skillInstallBody").value = "";
  toast(`Installed ${result.data.id} v${result.data.version}.`);
  await loadSkills();
  await openSkillDetail(result.data.id);
}

async function saveSkillInstructions(id, body) {
  if (!body.trim()) {
    toast("The instructions cannot be empty.");
    return;
  }
  const result = await api(`/api/skills/${encodeURIComponent(id)}/update`, {
    method: "POST",
    confirmApproval: true,
    body: JSON.stringify({ instructions: body }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not update that skill.");
    return;
  }
  toast(`Saved ${result.data.id} v${result.data.version}.`);
  await loadSkills();
  await openSkillDetail(result.data.id);
}

async function removeSkill(id) {
  if (!confirm(`Remove the skill "${id}"?\n\nIts folder is moved to .removed beside your other user skills, so it can be restored.`)) return;
  const result = await api(`/api/skills/${encodeURIComponent(id)}/remove`, {
    method: "POST",
    confirmApproval: true,
    body: JSON.stringify({}),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not remove that skill.");
    return;
  }
  toast(`Removed ${id}; it moved to ${result.data.moved_to}.`);
  state.openSkillId = null;
  state.skillDetail = null;
  await loadSkills();
  renderSkills();
}

function wireSkillInstall() {
  const button = $("skillInstallBtn");
  if (!button || button.dataset.wired) return;
  button.dataset.wired = "1";
  button.addEventListener("click", installSkill);
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
    const facts = [];
    if (skill.version) facts.push(`v${skill.version}`);
    if (skill.uses) facts.push(`${skill.uses} use${skill.uses === 1 ? "" : "s"}`);
    if (skill.modified) facts.push("edited since install");
    if (facts.length) {
      const meta = document.createElement("small");
      meta.className = "skill-meta";
      meta.textContent = facts.join(" · ");
      main.appendChild(meta);
    }
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
  mediaAttachments: [],
  /* Thread pane scope: "project" lists the active project's threads, "all"
     lists recent threads from every project, each labelled with its project. */
  threadScope: (() => { try { return localStorage.getItem("freeai-thread-scope") === "all" ? "all" : "project"; } catch { return "project"; } })(),
  model: "auto",
  /* thinking: "" means "model default" (no reasoning parameter). The choice is
     per message and sticky for the session; the server resolves
     message > agent > project > global and reports which scope won. */
  thinking: (() => { try { return localStorage.getItem("freeai-thinking") || ""; } catch { return ""; } })(),
  thinkingLevels: null,
  platformModels: null,
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
  skillDetail: null,
  /* permissions: saved rules plus the audit trail from /api/permissions. */
  permissions: { actions: [], rules: [] },
  /* mcp: the registry, the tools of the last tested server, and the picked
     tool. Nothing is cached server-side, so the panel re-tests on demand. */
  mcp: { servers: [], tools: [], target: null, health: {}, logs: {}, openLogs: new Set(), oauth: {} },
  /* activeSkillId: a skill the user picked in the composer. suggestedSkillId
     is a keyword match the composer offers when nothing was picked; either
     one rides with the next request as `skill`. */
  activeSkillId: null,
  suggestedSkillId: null,
  workspace: null,
  recentFolders: [],
  home: "~",
  usageQuery: { page: 1, sort: "created_at", dir: "desc", model: "", provider: "", from: "", to: "" },
  usageMeta: null,
  usageRows: [],
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
  const { confirmApproval, ...request } = options;
  const send = async (body) => {
    const response = await fetch(path, {
      headers: { "Content-Type": "application/json", ...(request.headers || {}) },
      ...request,
      ...(body === undefined ? {} : { body }),
    });
    const data = await response.json().catch(() => ({}));
    if (!response.ok && !data.error) data.error = `Request failed (HTTP ${response.status})`;
    return { ok: response.ok, status: response.status, data };
  };
  let result = await send(undefined);
  // A saved "ask" rule answers 409 instead of running. The second request is
  // the person's explicit yes — never a silent retry, and a deny rule still
  // wins because the server checks it before reading `approve`.
  if (confirmApproval && result.status === 409 && result.data.needs_approval) {
    if (!window.confirm(`${result.data.error}\n\nApprove this one time?`)) return result;
    let body = request.body;
    try {
      body = JSON.stringify({ ...JSON.parse(request.body || "{}"), approve: true });
    } catch {
      body = request.body;
    }
    result = await send(body);
  }
  return result;
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

const VIEWS = [
  "chat",
  "editor",
  "settings",
  "usage",
  "router",
  "local",
  "skills",
  "workspaces",
  "agents",
  "integrations",
  "artifacts",
  "automation",
  "runs",
];

function setView(view) {
  state.view = view;
  VIEWS.forEach((name) => {
    const section = $(`${name}View`);
    if (section) section.classList.toggle("hidden", view !== name);
    const button = $(`${name}Btn`);
    if (button) button.classList.toggle("active", view === name);
  });
  document.body.classList.remove("nav-open");
  $("scrim").classList.add("hidden");
  if (view === "chat") setTimeout(() => $("prompt").focus(), 30);
  if (view === "usage") loadUsage();
  if (view === "router") {
    loadConsoleStatus();
    loadRouterList();
  }
  if (view === "local") void loadLocal();
  if (view === "skills") renderSkillsOverview();
  if (view === "workspaces") renderWorkspaces();
  if (view === "agents") renderAgents();
  if (view === "integrations") loadIntegrations();
  if (view === "artifacts") renderArtifacts();
  if (view === "automation") loadWorkflows();
  if (view === "runs") void loadRuns();
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
  const { meta = null, error = false, attachment = null, media = null } = options;
  $("welcome").classList.add("hidden");

  const row = document.createElement("div");
  row.className = `msg msg-${role}` + (error ? " error" : "");

  const head = document.createElement("div");
  head.className = "msg-head";
  head.innerHTML = `<span class="msg-avatar ${role === "assistant" ? "assistant" : ""}">` +
    `${role === "assistant" ? icon("spark") : icon("user")}</span>` +
    `<span class="msg-author">${role === "assistant" ? "Saarthi Studio" : "You"}</span>` +
    `<span class="msg-meta"></span>`;
  if (meta) {
    const parts = [];
    if (meta.model) parts.push(meta.model);
    if (meta.provider) parts.push(meta.provider);
    if (Array.isArray(meta.fallbacks) && meta.fallbacks.length) {
      parts.push(`${meta.fallbacks.length} fallback${meta.fallbacks.length === 1 ? "" : "s"}`);
    }
    // A skill the user did not pick is shown, never silent: the reply says which
    // instructions were attached on its behalf.
    if (Array.isArray(meta.auto_skills) && meta.auto_skills.length) {
      parts.push(`skill ${meta.auto_skills.join(", ")}`);
    }
    if (Array.isArray(meta.memory_used) && meta.memory_used.length) {
      parts.push(`${meta.memory_used.length} memor${meta.memory_used.length === 1 ? "y" : "ies"}`);
    }
    if (Array.isArray(meta.retrieved) && meta.retrieved.length) {
      parts.push(`${meta.retrieved.length} code hit${meta.retrieved.length === 1 ? "" : "s"}`);
    }
    if (meta.latency_ms != null) parts.push(`${meta.latency_ms} ms`);
    if (meta.total_tokens != null) parts.push(`${meta.total_tokens} tokens`);
    pushContextMeta(parts, meta);
    head.querySelector(".msg-meta").textContent = parts.join(" · ");
    head.querySelector(".msg-meta").title = contextMetaTitle(meta);
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
    if (media && media.length) row.appendChild(mediaStrip(media));
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
  // Callers (the live stream row above all) keep updating the row after it
  // is added; without this they silently get undefined.
  return row;
}

/* Context compression is reported, never hidden: condensed or dropped turns
   change what the model could see, so the reply row says so. */
function pushContextMeta(parts, meta) {
  if (meta.context_trimmed) parts.push(`${meta.context_trimmed} condensed`);
  if (meta.context_dropped) parts.push(`${meta.context_dropped} dropped`);
  if (meta.context_saved_tokens) parts.push(`~${formatCount(meta.context_saved_tokens)} saved`);
}

function contextMetaTitle(meta) {
  const bits = [];
  if (Array.isArray(meta.auto_skills) && meta.auto_skills.length) {
    bits.push(`matched skill${meta.auto_skills.length === 1 ? "" : "s"}: ${meta.auto_skills.join(", ")}`);
  }
  if (Array.isArray(meta.memory_used) && meta.memory_used.length) {
    bits.push(`memory given to the model: ${meta.memory_used.join("; ")}`);
  }
  if (Array.isArray(meta.retrieved) && meta.retrieved.length) {
    bits.push(`code the index matched: ${meta.retrieved.join("; ")}`);
  }
  if (meta.context_tokens != null) bits.push(`${formatCount(meta.context_tokens)} tokens sent`);
  if (meta.context_budget) bits.push(`budget ${formatCount(meta.context_budget)}`);
  if (meta.context_saved_tokens) bits.push(`${formatCount(meta.context_saved_tokens)} estimated tokens kept out`);
  if (meta.context_dropped) bits.push(`${meta.context_dropped} message(s) not sent at all`);
  return bits.join(" · ");
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
  $("mediaAttachBtn").disabled = value;
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
  const result = await api(`/api/conversations${state.threadScope === "all" ? "?scope=all" : ""}`);
  if (!result.ok) {
    toast(result.data.error || "Could not load history.");
    return;
  }
  state.conversations = Array.isArray(result.data) ? result.data : [];
  renderThreadPaneHead();
  renderConversations();
}

/* The pane header names the thread's context so sidebar threads always read
   as "this project's threads" or "recent threads across projects". */
function renderThreadPaneHead() {
  const label = $("threadPaneLabel");
  if (!label) return;
  if (state.threadScope === "all") {
    label.textContent = "Recent — all projects";
  } else {
    const project = activeProjectName();
    label.textContent = `Recent in ${project}`;
  }
  $("threadScopeBtn").textContent = state.threadScope === "all" ? "This project" : "All projects";
  $("threadScopeBtn").title =
    state.threadScope === "all"
      ? "Back to showing this project's threads only"
      : "Show recent threads from every project";
}

/* "3m ago" style badge for thread rows. Unknown timestamps stay blank. */
function relativeTime(epoch, now = Date.now() / 1000) {
  const seconds = Number(epoch);
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  const delta = Math.max(0, now - seconds);
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  if (delta < 7 * 86400) return `${Math.floor(delta / 86400)}d ago`;
  return new Date(seconds * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" });
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
    const isOpen = conversation.id === state.currentId;
    item.className = "thread-item" + (isOpen ? " active" : "");
    item.innerHTML = `${icon("chat", "thread-icon")}<span class="thread-title"></span>` +
      (state.threadScope === "all"
        ? `<span class="thread-project" title="This thread belongs to this project"></span>`
        : "") +
      `<button class="thread-delete" title="Delete thread" aria-label="Delete thread">${icon("close")}</button>` +
      `<span class="thread-time"></span>` +
      (isOpen ? `<span class="thread-open" title="This thread is open in the chat view">open</span>` : "");
    item.querySelector(".thread-title").textContent = conversation.title;
    const projectChip = item.querySelector(".thread-project");
    if (projectChip) projectChip.textContent = conversation.project || "";
    item.querySelector(".thread-time").textContent = relativeTime(conversation.updated_at);
    item.title = `${conversation.title} — ${relativeTime(conversation.updated_at)}`;
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

/* Live thread mirroring: the server pushes a tiny refresh hint whenever a
   conversation changes (another tab sent a message, a stream finished, a
   thread was created or deleted). The affected tab refetches just that
   thread — no full reload, no invented content. */
let mirrorTimer = null;

function setupMirroring() {
  if (typeof EventSource === "undefined") return;
  let source;
  try {
    source = new EventSource("/api/events");
  } catch {
    return; // SSE unavailable in this embedding.
  }
  source.addEventListener("conversation-changed", (ev) => {
    let data = null;
    try {
      data = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (data && data.type === "agent-run-changed") {
      // `state.view` is the left rail (explorer/search/...), never "runs", so a
      // gate on it meant the panel only ever changed on a manual Refresh - and an
      // async pause (the run parks at its next step boundary) never showed up.
      void refreshRunsPanel(data.run_id);
      return;
    }
    if (data && (data.type === "local-pull" || data.type === "local-models-changed")) {
      handleLocalEvent(data);
      return;
    }
    if (!data || !data.conversation_id) return;
    // This tab already shows the live streaming row for that thread.
    if (state.busy && streamTargetId === data.conversation_id) return;
    clearTimeout(mirrorTimer);
    mirrorTimer = setTimeout(() => {
      if (state.currentId === data.conversation_id) void openConversation(data.conversation_id);
      void loadConversations();
    }, 250);
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
  // The thread stores the model that answered it, so reopening it restores that
  // choice instead of silently switching to today's default.
  if (match && match.model) {
    state.model = match.model;
    $("modelChip").textContent = state.model;
  }
  // A live streaming row for this thread keeps its place across re-renders:
  // switching away and back must not hide the in-flight reply.
  const liveRow = state.busy && streamRow && streamRow.isConnected && streamTargetId === id ? streamRow : null;
  $("messages").replaceChildren();
  const messages = Array.isArray(result.data) ? result.data : [];
  $("welcome").classList.toggle("hidden", messages.length > 0 || Boolean(liveRow));
  messages.forEach((message) => addMessage(message.role === "assistant" ? "assistant" : "user", message.content, {
    media: Array.isArray(message.attachments) && message.attachments.length ? message.attachments : null,
  }));
  if (liveRow) $("messages").appendChild(liveRow);
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

/* Deep search toggle: researches the next message on the web with visible
   steps and cited sources. Sticky in localStorage, like a mode switch. */
function setDeepSearch(on) {
  state.deepSearch = Boolean(on);
  try { localStorage.setItem("deepSearch", String(state.deepSearch)); } catch { /* private mode */ }
  $("deepToggle").classList.toggle("active", state.deepSearch);
  $("deepToggle").setAttribute("aria-pressed", String(state.deepSearch));
}

try { setDeepSearch(localStorage.getItem("deepSearch") === "true"); } catch { /* private mode */ }

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
  const skillId = effectiveSkillId();
  // A suggestion is per-message: it is consumed by this send. An explicit
  // pick stays until the user clears it.
  state.suggestedSkillId = null;
  renderSkillChip();
  $("prompt").value = "";
  autoGrow($("prompt"));
  addMessage("user", message, {
    attachment: context ? (context.scratch ? "Scratch pad attached" : context.path) : null,
    media: state.mediaAttachments.slice(),
  });
  const attachmentsToSend = state.mediaAttachments.map(({ kind, ref }) => ({ kind, ref }));
  clearMediaAttachments();
  // Show the estimate again until the router reports the real count.
  state.lastContextTokens = null;
  updateContextMeter();
  setBusy(true);
  streamTargetId = state.currentId;
  const deepSteps = [];
  const deepSources = [];
  const reasoningSummary = [];
  const controller = new AbortController();
  state.abort = controller;
  addStreamRow();

  let answer = "";
  let failed = null;
  let aborted = false;
  try {
    const response = await fetch("/api/chat/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        conversation_id: state.currentId,
        message,
        code: context ? context.content : "",
        language: context ? context.language : "text",
        skill: skillId,
        deepsearch: Boolean(state.deepSearch),
        model: state.chatModel || undefined,
        thinking: state.thinking || undefined,
        mode: state.chatMode === "plan" ? "plan" : undefined,
        attachments: attachmentsToSend.length ? attachmentsToSend : undefined,
      }),
      signal: controller.signal,
    });
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      failed = data.error || `Request failed (HTTP ${response.status})`;
    } else {
      await consumeStream(response.body, (frame) => {
        if (frame.type === "status" || frame.type === "query" || frame.type === "source") {
          setStreamStatus("searching");
        }
        if (frame.type === "status") {
          deepSteps.push({ type: "status", text: frame.message || "" });
          renderDeepSteps(deepSteps, deepSources);
        } else if (frame.type === "query") {
          deepSteps.push({ type: "query", text: frame.text || "" });
          renderDeepSteps(deepSteps, deepSources);
        } else if (frame.type === "source") {
          deepSources.push({ title: frame.title || "Untitled", url: frame.url || "" });
          renderDeepSteps(deepSteps, deepSources);
        }
        if (frame.delta) {
          if (!answer) setStreamStatus("responding");
          answer += frame.delta;
          appendStreamText(answer);
        }
        if (frame.type === "reasoning" && frame.text) {
          reasoningSummary.push(frame.text);
          renderReasoning(reasoningSummary.join(""));
        }
        if (frame.error) failed = frame.error;
        if (frame.done && frame.meta) {
          if (frame.meta.context_tokens != null) state.lastContextTokens = Number(frame.meta.context_tokens);
          applyStreamMeta({ ...frame.meta, deepsearch: frame.meta.deepsearch || Boolean(state.deepSearch) });
        }
      });
    }
  } catch (error) {
    if (error && error.name === "AbortError") aborted = true;
    else if (error) failed = error.message || "The request failed.";
  }
  finishStreamRow(answer, failed, deepSources, aborted);
  streamTargetId = null;
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

/* Live deep-search checklist inside the streaming row: each emitted status,
   query and source line. Built with the DOM so the text is never injected. */
/* Media attached to a message: images render inline, audio and video use
   native controls. Refs are always server-generated names from /api/attachments. */
function mediaStrip(items) {
  const strip = document.createElement("div");
  strip.className = "media-strip";
  for (const item of items) {
    const url = `/attachments/${encodeURIComponent(item.ref || "")}`;
    if ((item.kind || "") === "image") {
      const img = document.createElement("img");
      img.className = "media-thumb";
      img.src = url;
      img.alt = "Attached image";
      img.loading = "lazy";
      strip.appendChild(img);
    } else if (item.kind === "audio") {
      const audio = document.createElement("audio");
      audio.controls = true;
      audio.src = url;
      strip.appendChild(audio);
    } else if (item.kind === "video") {
      const video = document.createElement("video");
      video.className = "media-thumb";
      video.controls = true;
      video.src = url;
      strip.appendChild(video);
    }
  }
  return strip;
}

/* Composer media picker: files are read as data URLs, uploaded once to
   /api/attachments, and listed as removable chips until the message is sent. */
async function uploadMediaFiles(fileList) {
  for (const file of Array.from(fileList)) {
    if (state.mediaAttachments.length >= 4) {
      toast("A message can carry at most 4 attachments.");
      break;
    }
    const reader = new FileReader();
    const dataUrl = await new Promise((resolve, reject) => {
      reader.onload = () => resolve(reader.result);
      reader.onerror = () => reject(new Error(`Could not read ${file.name}.`));
      reader.readAsDataURL(file);
    }).catch(() => null);
    if (!dataUrl) {
      toast(`Could not read ${file.name}.`);
      continue;
    }
    const mime = String(dataUrl).slice(5).split(";base64")[0] || "";
    const kind = mime.startsWith("image/") ? "image"
      : mime.startsWith("audio/") ? "audio"
      : mime.startsWith("video/") ? "video" : "";
    if (!kind) {
      toast(`${file.name}: only image, audio and video files can be attached.`);
      continue;
    }
    const result = await api("/api/attachments", {
      method: "POST",
      body: JSON.stringify({ kind, data_url: dataUrl }),
    });
    if (!result.ok || !result.data.attachment) {
      toast(result.data.error || `Could not upload ${file.name}.`);
      continue;
    }
    state.mediaAttachments.push(result.data.attachment);
  }
  renderMediaChips();
}

function renderMediaChips() {
  const wrap = $("mediaChips");
  wrap.replaceChildren();
  if (!state.mediaAttachments.length) {
    wrap.classList.add("hidden");
    return;
  }
  wrap.classList.remove("hidden");
  state.mediaAttachments.forEach((item, index) => {
    const chip = document.createElement("div");
    chip.className = "media-chip";
    if ((item.kind || "") === "image") {
      const img = document.createElement("img");
      img.src = `/attachments/${encodeURIComponent(item.ref)}`;
      img.alt = "attachment";
      img.loading = "lazy";
      chip.appendChild(img);
    } else {
      const label = document.createElement("span");
      label.className = "media-tag";
      label.textContent = item.kind;
      chip.appendChild(label);
    }
    const remove = document.createElement("button");
    remove.className = "chip-close";
    remove.title = "Remove attachment";
    remove.setAttribute("aria-label", "Remove attachment");
    remove.innerHTML = icon("close");
    remove.addEventListener("click", () => {
      state.mediaAttachments.splice(index, 1);
      renderMediaChips();
    });
    chip.appendChild(remove);
    wrap.appendChild(chip);
  });
}

function clearMediaAttachments() {
  state.mediaAttachments = [];
  renderMediaChips();
}

function renderDeepSteps(steps, sources) {
  if (!streamRow) return;
  let card = streamRow.querySelector(".deep-steps");
  const body = streamRow.querySelector(".msg-body");
  if (!card) {
    card = document.createElement("div");
    card.className = "deep-steps";
    if (body) streamRow.insertBefore(card, body);
    else streamRow.appendChild(card);
  }
  card.replaceChildren();
  const title = document.createElement("span");
  title.className = "deep-title";
  title.textContent = "Deep search";
  card.appendChild(title);
  const list = document.createElement("ul");
  for (const step of steps) {
    const item = document.createElement("li");
    item.className = step.type === "query" ? "deep-query" : "deep-status";
    item.textContent = step.type === "query" ? `searched: ${step.text}` : step.text;
    list.appendChild(item);
  }
  for (const [index, source] of sources.entries()) {
    const item = document.createElement("li");
    item.className = "deep-source-line";
    const link = document.createElement("a");
    link.href = source.url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = `[${index + 1}] ${source.title}`;
    item.appendChild(link);
    list.appendChild(item);
  }
  card.appendChild(list);
  scrollChat();
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
let streamTargetId = null;

function addStreamRow() {
  streamRow = addMessage("assistant", "", { streaming: true });
  const head = streamRow ? streamRow.querySelector(".msg-meta") : null;
  if (head && !head.querySelector(".stream-status")) {
    const chip = document.createElement("span");
    chip.className = "stream-status";
    chip.textContent = "sending";
    head.appendChild(chip);
  }
}

/* Execution status: what the request is doing right now, next to the meta. */
function setStreamStatus(text) {
  const chip = streamRow ? streamRow.querySelector(".stream-status") : null;
  if (chip && text) chip.textContent = text;
}

function clearStreamStatus() {
  const chip = streamRow ? streamRow.querySelector(".stream-status") : null;
  if (chip) chip.remove();
}

/* Concise reasoning summaries the provider chose to publish — the API's own
   summary channel, never raw chain-of-thought. Open while it streams, then
   left collapsed by the user's own toggle so it never hides the reply. */
function renderReasoning(text) {
  if (!streamRow) return;
  let card = streamRow.querySelector(".reasoning-card");
  if (!card) {
    card = document.createElement("details");
    card.className = "reasoning-card";
    card.open = true;
    const summary = document.createElement("summary");
    summary.textContent = "Reasoning summary";
    const body = document.createElement("p");
    body.className = "reasoning-body";
    card.append(summary, body);
    const head = streamRow.querySelector(".msg-meta");
    if (head && head.parentNode === streamRow) streamRow.insertBefore(card, head.nextSibling);
    else streamRow.insertBefore(card, streamRow.firstChild);
  }
  const body = card.querySelector(".reasoning-body");
  if (body) body.textContent = text;
  scrollChat();
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
  if (meta.deepsearch) parts.push("deep search");
  if (meta.model) parts.push(meta.model);
  if (meta.provider) parts.push(meta.provider);
  if (meta.latency_ms != null) parts.push(`${meta.latency_ms} ms`);
  if (meta.total_tokens != null) parts.push(`${meta.total_tokens} tokens`);
  pushContextMeta(parts, meta);
  head.title = contextMetaTitle(meta);
  const chip = head.querySelector(".stream-status");
  head.textContent = parts.join(" · ");
  if (chip) head.appendChild(chip);
}

function finishStreamRow(answer, failure, deepSources = [], aborted = false) {
  if (!streamRow) return;
  clearStreamStatus();
  const row = streamRow;
  streamRow = null;
  const body = row.querySelector(".msg-body");
  const caret = row.querySelector(".stream-caret");
  if (caret) caret.remove();
  if (failure) {
    row.classList.add("error");
    if (body) body.innerHTML = renderMarkdown(answer ? `${answer}\n\n**${failure}**` : failure);
  } else if (!answer.trim()) {
    // Never drop the row silently: an empty reply is a state the user must
    // see, not something to pretend never happened.
    if (body) {
      if (aborted) {
        row.classList.add("stopped");
        body.textContent = "Stopped. The request was cancelled before any reply arrived.";
      } else {
        row.classList.add("error");
        body.textContent = "The model returned an empty response. Try again or pick a different model.";
      }
    }
  } else {
    if (body) body.innerHTML = renderMarkdown(answer);
    if (deepSources.length) {
      const list = document.createElement("div");
      list.className = "deep-sources";
      const label = document.createElement("span");
      label.textContent = "Sources";
      list.appendChild(label);
      const buttons = deepSources.map((source, i) => {
        const link = document.createElement("a");
        link.className = "deep-source";
        link.href = source.url;
        link.target = "_blank";
        link.rel = "noopener noreferrer";
        link.textContent = `[${i + 1}] ${source.title}`;
        link.title = source.url;
        return link;
      });
      list.append(...buttons);
      row.appendChild(list);
    }
    appendMessageActions(row, [{ label: "Copy reply", run: () => copyText(answer, "Reply copied.") }]);
  }
  scrollChat();
}

function stopStream() {
  if (state.abort) state.abort.abort();
}

/* Re-sync when connectivity or focus changes: the open thread re-fetches from
   the server (replaceChildren, so reloading never duplicates messages).
   Skipped mid-stream — the in-flight request owns the view until it ends. */
async function resyncCurrentThread() {
  if (state.busy || !state.currentId) return;
  await openConversation(state.currentId);
  await loadConversations();
}

window.addEventListener("online", () => { void resyncCurrentThread(); });
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") void resyncCurrentThread();
});

function syncCurrentTitle() {
  const match = state.conversations.find((entry) => entry.id === state.currentId);
  if (match) state.currentTitle = match.title;
  renderTabs();
}

/* ---- panel layout helpers ---------------------------------------- */

/* Values are px numbers written straight into custom properties; the grid and
   media queries in styles.css translate them. Min/max clamp both directions
   so panels can never cover the composer or swallow the workspace. */
let PANEL_MIN = null;
let PANEL_MAX = null;
const PANEL_VAR = { left: "--sidebar-w", right: "--right-w", bottom: "--bottom-h" };
const PANEL_DEFAULT_W = { left: 286, right: 300, bottom: 240 };

function writePanelVars() {
  const root = document.documentElement.style;
  for (const [name, size] of Object.entries({ ...PANEL_DEFAULT_W, ...state.panelSize })) {
    root.setProperty(PANEL_VAR[name], `${size}px`);
  }
  PANEL_MIN = PANEL_MIN || { left: 212, right: 240, bottom: 120 };
  PANEL_MAX = PANEL_MAX || { left: 560, right: 640, bottom: 520 };
}

function applyPanels() {
  document.body.classList.toggle("no-left", !state.panelShow.left);
  document.body.classList.toggle("no-right", !state.panelShow.right);
  document.body.classList.toggle("no-bottom", !state.panelShow.bottom);
  for (const name of ["left", "right", "bottom"]) {
    const button = $(`${name}PanelToggle`);
    if (!button) continue;
    button.setAttribute("aria-pressed", String(state.panelShow[name]));
    button.classList.toggle("active", state.panelShow[name]);
  }
  if ($("rightPanel")) $("rightPanel").classList.toggle("hidden", !state.panelShow.right);
  if ($("bottomPanel")) $("bottomPanel").classList.toggle("hidden", !state.panelShow.bottom);
}

function togglePanel(name) {
  state.panelShow[name] = !state.panelShow[name];
  try { localStorage.setItem("freeai-panel-show", JSON.stringify(state.panelShow)); } catch { /* private mode */ }
  applyPanels();
  if (name === "bottom" && state.panelShow.bottom) showTerminalPanel();
  if (name === "right" && state.panelShow.right) refreshRightPanel();
}

function startPanelDrag(event) {
  const side = event.currentTarget.dataset.drag;
  const prop = side === "right" ? "width" : "height";
  const start = side === "right" ? event.clientX : event.clientY;
  let currentPx = getComputedStyle(document.documentElement).getPropertyValue(PANEL_VAR[side]).trim();
  let value = Math.max(PANEL_MIN[side], Math.min(PANEL_MAX[side], parseFloat(currentPx) || PANEL_DEFAULT_W[side]));
  event.currentTarget.setPointerCapture(event.pointerId);

  const onMove = (moveEvent) => {
    moveEvent.preventDefault();
    const delta = side === "right" ? start - moveEvent.clientX : start - moveEvent.clientY;
    value = Math.max(PANEL_MIN[side], Math.min(PANEL_MAX[side], Math.round(value + delta)));
    state.panelSize[side] = value;
    document.documentElement.style.setProperty(PANEL_VAR[side], `${value}px`);
    if (side === "bottom" && terminalFit) terminalFit();
  };
  const onUp = () => {
    document.documentElement.removeEventListener("pointermove", onMove);
    document.documentElement.removeEventListener("pointerup", onUp);
    document.documentElement.removeEventListener("pointercancel", onUp);
    try { localStorage.setItem("freeai-panel-size", JSON.stringify(state.panelSize)); } catch { /* private mode */ }
    if (side === "bottom" && terminalFit) terminalFit();
  };
  document.documentElement.addEventListener("pointermove", onMove);
  document.documentElement.addEventListener("pointerup", onUp);
  document.documentElement.addEventListener("pointercancel", onUp);
}

/* Right panel content: a glance at the router plus the last requests. Rendered
   from the same /api/usage data the Usage page already loads. */
let rightPanelTimer = null;

async function refreshRightPanel() {
  if (!state.panelShow.right) return;
  const result = await api("/api/usage");
  if (!result.ok) return;
  const summary = result.data.summary || {};
  const rows = Array.isArray(result.data.requests) ? result.data.requests : [];
  const lastOk = rows.find((row) => row.status === "ok");
  $("rightModel").textContent = lastOk?.model || state.model || "—";
  $("rightProvider").textContent = lastOk?.provider || "—";
  $("rightRequests").textContent = String(summary.requests ?? "—");
  $("rightTokens").textContent = formatCount(Number(summary.total_tokens) || 0);
  const latency = Math.round(Number(summary.avg_latency_ms) || 0);
  $("rightLatency").textContent = summary.requests ? `${latency} ms` : "—";
  const box = $("rightRecent");
  box.replaceChildren();
  if (!rows.length) {
    const note = document.createElement("span");
    note.className = "empty-note";
    note.textContent = "No requests yet today.";
    box.appendChild(note);
  } else {
    rows.slice(0, 8).forEach((row) => {
      const line = document.createElement("div");
      line.className = "panel-recent-item";
      const label = document.createElement("span");
      label.textContent = row.status === "ok"
        ? `${row.model || "model"} — ${formatCount(Number(row.total_tokens) || 0)} tok`
        : `failed — ${(row.error || "request failed").slice(0, 46)}`;
      const when = document.createElement("small");
      when.textContent = relativeTime(row.created_at);
      line.append(label, when);
      box.appendChild(line);
    });
  }
  }

function scheduleRightPanelTimer() {
  if (rightPanelTimer) clearInterval(rightPanelTimer);
  rightPanelTimer = setInterval(() => {
    if (state.panelShow.right && !document.hidden) void refreshRightPanel();
  }, 15000);
}

/* ------------------------------------------------------------------ */
/* workspace + explorer                                                */
/* ------------------------------------------------------------------ */

function setSidebarMode(mode) {
  state.sidebarMode = mode;
  ["threads", "files", "index", "skills", "memory"].forEach((name) => {
    const button = $(`mode${name.charAt(0).toUpperCase()}${name.slice(1)}`);
    if (!button) return;
    const active = mode === name;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", String(active));
  });
  $("threadsPane").classList.toggle("hidden", mode !== "threads");
  $("filesPane").classList.toggle("hidden", mode !== "files");
  if ($("skillsPane")) $("skillsPane").classList.toggle("hidden", mode !== "skills");
  if ($("indexPane")) $("indexPane").classList.toggle("hidden", mode !== "index");
  if ($("memoryPane")) $("memoryPane").classList.toggle("hidden", mode !== "memory");
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
  terminalFollowWorkspace();
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
    confirmApproval: true,
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
    confirmApproval: true,
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
    confirmApproval: true,
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
    confirmApproval: true,
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

/* The skill attached to the next message: an explicit pick wins, otherwise
   the keyword suggestion. Shows as a chip so it can be cleared before send. */
function effectiveSkillId() {
  return state.activeSkillId || state.suggestedSkillId || "";
}

function renderSkillChip() {
  const chip = $("skillChip");
  if (!chip) return;
  const id = effectiveSkillId();
  if (!id) {
    chip.classList.add("hidden");
    return;
  }
  const skill = skillById(id);
  chip.classList.remove("hidden");
  $("skillChipLabel").textContent = state.activeSkillId
    ? `skill: ${skill ? skill.name : id}`
    : `suggested: ${skill ? skill.name : id}`;
}

function closeSkillMenu() {
  const menu = $("skillMenu");
  if (!menu) return;
  menu.classList.add("hidden");
  $("skillPickerBtn").setAttribute("aria-expanded", "false");
}

function renderSkillMenu() {
  const list = $("skillMenuList");
  if (!list) return;
  list.replaceChildren();
  const usable = state.skills.filter((skill) => !skill.error && skill.enabled !== false);

  if (!usable.length) {
    const empty = document.createElement("p");
    empty.className = "skill-menu-empty";
    empty.textContent =
      "No skills yet. Add a SKILL.md folder to this project, your config folder, or ~/.agents/skills.";
    list.appendChild(empty);
    return;
  }

  const head = document.createElement("p");
  head.className = "skill-menu-head";
  head.textContent = "Attach a skill to this message";
  list.appendChild(head);

  const rows = [{ id: "", name: "No skill", description: "" }, ...usable];
  rows.forEach((skill) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className =
      "skill-menu-item" + (effectiveSkillId() === skill.id ? " active" : "");
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(effectiveSkillId() === skill.id));
    const text = document.createElement("span");
    const strong = document.createElement("strong");
    strong.textContent = skill.name || skill.id || "No skill";
    text.appendChild(strong);
    if (skill.description) {
      const small = document.createElement("small");
      small.textContent = skill.description;
      small.title = skill.description;
      text.appendChild(small);
    }
    row.appendChild(text);
    row.addEventListener("click", () => {
      state.activeSkillId = skill.id || null;
      state.suggestedSkillId = null;
      renderSkillChip();
      renderSkillMenu();
      closeSkillMenu();
      $("prompt").focus();
    });
    list.appendChild(row);
  });
}

function toggleSkillMenu() {
  const menu = $("skillMenu");
  if (!menu) return;
  if (menu.classList.contains("hidden")) {
    renderSkillMenu();
    menu.classList.remove("hidden");
    $("skillPickerBtn").setAttribute("aria-expanded", "true");
  } else {
    closeSkillMenu();
  }
}

/* Keyword auto-suggest: one pass over the enabled skills' names and
   descriptions, first match wins. Deliberately dumb — no scoring, no model. */
function suggestSkill(message) {
  if (state.activeSkillId) return;
  const text = message.toLowerCase();
  if (!text.trim()) {
    state.suggestedSkillId = null;
    renderSkillChip();
    return;
  }
  const match = state.skills.find((skill) => {
    if (skill.error || skill.enabled === false || !skill.description) return false;
    const haystack = `${skill.id} ${skill.name} ${skill.description}`.toLowerCase();
    return haystack.split(/[^\w+-]+/).some((word) => word.length >= 5 && text.includes(word));
  });
  const next = match ? match.id : null;
  if (next !== state.suggestedSkillId) {
    state.suggestedSkillId = next;
    renderSkillChip();
  }
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

/* Backend presets: platforms Studio can point at. Self-hosted gateways first,
   then free/freemium hosted APIs, then paid ones for completeness — each card
   carries an honest free-tier note from that platform's own pricing docs. */
const BACKEND_PRESETS = [
  {
    id: "freellmapi",
    name: "FreeLLMAPI",
    baseUrl: "http://localhost:3001/v1",
    model: "auto",
    note: "Self-hosted FreeLLMAPI router — free to run; it aggregates provider keys you add in its dashboard.",
    keyUrl: "",
  },
  {
    id: "omniroute",
    name: "OmniRoute",
    baseUrl: "http://localhost:20128/v1",
    model: "auto",
    note: "Self-hosted OmniRoute gateway — free to run (npm i -g omniroute or Docker); you bring provider keys.",
    keyUrl: "https://github.com/cheaper-inference/OmniRoute",
  },
  {
    id: "groq",
    name: "Groq",
    baseUrl: "https://api.groq.com/openai/v1",
    model: "llama-3.3-70b-versatile",
    note: "Free rate-limited tier — per-model RPM/day caps shown in your Groq console.",
    keyUrl: "https://console.groq.com/keys",
  },
  {
    id: "gemini",
    name: "Google Gemini",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai/",
    model: "gemini-2.5-flash",
    note: "Free tier on selected models (rate-capped; content may be used to improve Google products).",
    keyUrl: "https://aistudio.google.com/apikey",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "auto",
    note: "50 free-model requests/day (1,000/day after $10 of credit).",
    keyUrl: "https://openrouter.ai/settings/keys",
    dashboardUrl: "https://openrouter.ai/settings/keys",
  },
  {
    id: "mistral",
    name: "Mistral",
    baseUrl: "https://api.mistral.ai/v1",
    model: "mistral-small-latest",
    note: "Studio free mode with API access, no card required; usage limits apply.",
    keyUrl: "https://console.mistral.ai/api-keys",
  },
  {
    id: "cloudflare",
    name: "Cloudflare Workers AI",
    baseUrl: "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1",
    model: "",
    note: "10,000 Neurons/day free — replace {account_id} in the URL with your Cloudflare account id.",
    keyUrl: "https://dash.cloudflare.com/profile/api-tokens",
  },
  {
    id: "huggingface",
    name: "Hugging Face",
    baseUrl: "https://router.huggingface.co/v1",
    model: "",
    note: "Works with an HF token — free accounts get no included inference credits (PRO gets $2/mo).",
    keyUrl: "https://huggingface.co/settings/tokens",
  },
  {
    id: "cerebras",
    name: "Cerebras",
    baseUrl: "https://api.cerebras.ai/v1",
    model: "llama-3.3-70b",
    note: "$5 trial credits after adding a payment method — they expire after 30 days.",
    keyUrl: "https://cloud.cerebras.ai/",
  },
  {
    id: "novita",
    name: "Novita AI",
    baseUrl: "https://api.novita.ai/openai/v1",
    model: "",
    note: "The $100 promo is sandbox-only — check your actual credit balance in the dashboard.",
    keyUrl: "https://novita.ai/",
  },
  {
    id: "together",
    name: "Together AI",
    baseUrl: "https://api.together.ai/v1",
    model: "",
    note: "No free tier (initial credit purchase starts at $15) — listed for completeness.",
    keyUrl: "https://api.together.ai/settings/api-keys",
  },
  {
    id: "deepinfra",
    name: "DeepInfra",
    baseUrl: "https://api.deepinfra.com/v1/openai",
    model: "",
    note: "OpenAI-compatible and cheap, but no verified free tier.",
    keyUrl: "https://deepinfra.com/",
  },
  {
    id: "custom",
    name: "Custom URL",
    baseUrl: "",
    model: "",
    note: "Type any OpenAI-compatible base URL below (HTTPS required off-machine).",
    keyUrl: "",
  },
];

function renderBackendPresets() {
  const host = $("backendPresets");
  if (!host) return;
  host.replaceChildren();
  BACKEND_PRESETS.forEach((preset) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "preset-chip";
    button.textContent = preset.name;
    button.dataset.preset = preset.id;
    button.setAttribute("role", "option");
    button.addEventListener("click", () => applyBackendPreset(preset.id));
    host.appendChild(button);
  });
  markActivePreset();
}

function presetForBaseUrl(url) {
  return BACKEND_PRESETS.find((preset) => preset.baseUrl && preset.baseUrl === url) || null;
}

function markActivePreset() {
  const host = $("backendPresets");
  if (!host) return;
  const active = presetForBaseUrl($("baseUrl").value.trim());
  host.querySelectorAll(".preset-chip").forEach((chip) => {
    const on = Boolean(active) && chip.dataset.preset === active.id;
    chip.classList.toggle("active", on);
    chip.setAttribute("aria-selected", String(on));
  });
  const note = $("presetNote");
  const link = $("presetKeyLink");
  if (active) {
    note.textContent = active.note;
    if (active.keyUrl) {
      link.href = active.keyUrl;
      link.classList.remove("hidden");
    } else {
      link.classList.add("hidden");
    }
  } else {
    note.textContent =
      "Custom URL — type any OpenAI-compatible base URL (HTTPS required off-machine).";
    link.classList.add("hidden");
  }
}

function applyBackendPreset(id) {
  const preset = BACKEND_PRESETS.find((entry) => entry.id === id);
  if (!preset) return;
  $("baseUrl").value = preset.baseUrl;
  if (preset.model) {
    $("model").value = preset.model;
    $("model").placeholder = preset.model;
  } else if (id !== "custom") {
    $("model").value = "auto";
  }
  markActivePreset();
  if (preset.baseUrl) $("apiKey").focus();
  else $("baseUrl").focus();
  if (id === "omniroute") offerOmnirouteStart();
}

/* Picking OmniRoute changes the router URL to localhost:20128, so if its
   gateway is not up, chats would all fail. Offer the one-click start. */
async function offerOmnirouteStart() {
  const status = await api("/api/omniroute");
  if (!status.ok || status.data.running) return;
  if (!confirm("OmniRoute is not running on port 20128. Start its gateway now? (Install once with `npm i -g omniroute`.)")) return;
  const started = await api("/api/omniroute/start", { method: "POST", body: "{}" });
  toast(started.ok ? started.data.message || "OmniRoute start requested." : started.data.error || "Could not start OmniRoute — see the Integrations view.");
}

/* ------------------------------------------------------------------ */
/* model + mode pickers (composer)                                      */
/* ------------------------------------------------------------------ */

const PROVIDER_NAMES = {
  local: "Local",
  openai: "OpenAI",
  anthropic: "Anthropic",
  claude: "Anthropic",
  gemini: "Google",
  google: "Google",
  "meta-llama": "Meta",
  llama: "Meta",
  mistralai: "Mistral",
  mistral: "Mistral",
  deepseek: "DeepSeek",
  qwen: "Qwen",
  groq: "Groq",
  openrouter: "OpenRouter",
  nvidia: "NVIDIA",
  nousresearch: "Nous",
  together: "Together",
  moonshotai: "Moonshot",
  "x-ai": "xAI",
  meta: "Meta",
  microsoft: "Microsoft",
  cohere: "Cohere",
  ai21: "AI21",
  perplexity: "Perplexity",
};

function providerOfModel(id) {
  if (!id || id === "auto") return "router";
  const head = id.split(/[/:@]/)[0].toLowerCase();
  return PROVIDER_NAMES[head] || (head ? head : "router");
}

function providerBadge(name) {
  const letter = (name || "?").charAt(0).toUpperCase();
  let hash = 0;
  for (const ch of name || "") hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  const span = document.createElement("span");
  span.className = `pbadge pbadge-${hash % 6}`;
  span.textContent = letter;
  span.title = name || "provider";
  return span;
}

/* Per-model "today" usage, from a dedicated fetch of today's rows (the
   Usage view's own page slice is too small to count from). Refreshed each
   time the model drawer opens. */
async function refreshModelDailyUsage() {
  const dayStart = new Date();
  dayStart.setHours(0, 0, 0, 0);
  const params = new URLSearchParams({
    from: String(dayStart.getTime() / 1000),
    page_size: "200",
    sort: "created_at",
    dir: "desc",
  });
  const result = await api(`/api/usage?${params}`);
  if (!result.ok || !Array.isArray(result.data.requests)) return;
  const map = new Map();
  for (const row of result.data.requests) {
    const key = row.model || "";
    const entry = map.get(key) || { count: 0, tokens: 0 };
    entry.count += 1;
    entry.tokens += Number(row.total_tokens) || 0;
    map.set(key, entry);
  }
  state.modelDaily = map;
  if (!$("modelMenu").classList.contains("hidden")) renderModelMenu();
}

function modelDailyUsage(id) {
  if (state.modelDaily instanceof Map) {
    const entry = state.modelDaily.get(id);
    if (!entry) return null;
    return `${entry.count} req today · ${formatCount(entry.tokens)} tok`;
  }
  return null;
}

async function loadModels() {
  const result = await api("/api/models");
  state.models = result.ok && Array.isArray(result.data.models) ? result.data.models : null;
  state.modelsError = result.ok ? (result.data.error || "") : `Endpoint unavailable (HTTP ${result.status ?? "?"})`;
  renderModelChipLabel();
}

function renderModelChipLabel() {
  const chip = $("modelChip");
  if (!chip) return;
  chip.textContent = state.chatModel || state.model || "auto";
  chip.title = state.chatModel || state.model || "auto";
}

function toggleModelMenu(force) {
  const menu = $("modelMenu");
  const show = force != null ? force : menu.classList.contains("hidden");
  menu.classList.toggle("hidden", !show);
  $("modelChip").setAttribute("aria-expanded", String(show));
  if (show) {
    $("modelFilter").value = "";
    renderModelMenu();
    $("modelFilter").focus();
    void refreshModelDailyUsage();
    // Local models come from the runtime, fetched beside the router catalog.
    void refreshLocalMenuModels();
    // Platform models only appear when a platform key is saved; the endpoint
    // answers 400 otherwise and leaves the list as it was.
    void refreshPlatformMenuModels();
  }
}

async function refreshLocalMenuModels() {
  const result = await api("/api/local/models");
  state.localModels = result.data && result.data.ok !== undefined ? result.data : state.localModels;
  renderModelMenu();
}

function localMenuModels() {
  const payload = state.localModels;
  if (!payload || payload.ok === false) return [];
  return (payload.models || []).map((model) => model.name);
}

function renderModelMenu() {
  const list = $("modelMenuList");
  list.replaceChildren();
  const filter = $("modelFilter").value.trim().toLowerCase();

  // The config's own default model, by contract always first.
  list.appendChild(modelMenuItem({
    id: state.model || "auto",
    label: `Default — ${state.model || "auto"}`,
    chosen: !state.chatModel,
    onPick: () => { state.chatModel = null; },
  }));

  // Local models (from /api/local/models): only real ones the runtime lists,
  // always labelled as local so nobody mistakes them for router picks.
  const localNames = localMenuModels().filter((name) => name.toLowerCase().includes(filter));
  localNames.forEach((name) => {
    list.appendChild(modelMenuItem({
      id: `local:${name}`,
      label: `${name}`,
      chosen: state.chatModel === `local:${name}`,
      onPick: () => {
        state.chatModel = `local:${name}`;
        void loadLocalCapabilities(name);
      },
    }));
  });

  // Native OpenAI Platform models, labelled so a platform pick is never
  // mistaken for a router pick. Only real ids the platform lists.
  const platform = (state.platformModels || [])
    .filter((model) => String(model.label || model.id).toLowerCase().includes(filter));
  platform.slice(0, 200).forEach((model) => {
    list.appendChild(modelMenuItem({
      id: model.id,
      label: `${model.label || model.id} · OpenAI Platform`,
      context: model.context_window || 0,
      capabilities: model.capabilities,
      chosen: state.chatModel === model.id,
      onPick: () => { state.chatModel = model.id; },
    }));
  });

  const note = document.createElement("p");
  note.className = "model-note";
  if (state.models && state.models.length) {
    const matches = state.models.filter((m) => (m.id || "").toLowerCase().includes(filter));
    if (!matches.length) {
      note.textContent = `No model matches "${$("modelFilter").value.trim()}".`;
      list.appendChild(note);
      list.appendChild(capabilityNote());
      return;
    }
    matches.slice(0, 200).forEach((model) => {
      list.appendChild(modelMenuItem({
        id: model.id,
        context: model.context_window || 0,
        capabilities: model.capabilities,
        chosen: state.chatModel === model.id,
        onPick: () => { state.chatModel = model.id; },
      }));
    });
  } else {
    note.textContent = state.modelsError || "Load the catalog from the router to pick a model.";
    list.appendChild(note);
  }
  list.appendChild(capabilityNote());
}

/* Capabilities of the model that would answer the next message. The backend
   only reports what the provider published, so anything it did not publish is
   shown as unknown rather than assumed. */
function chosenCapabilities() {
  const pick = state.chatModel || "";
  if (pick.startsWith("local:")) {
    const name = pick.slice("local:".length);
    if (!state.localCapabilities || state.localCapabilities.name !== name) return { model: name, caps: null };
    return { model: name, caps: state.localCapabilities.capabilities || null };
  }
  const id = pick || state.model;
  const row = (state.models || []).find((model) => model.id === id);
  return { model: id, caps: (row && row.capabilities) || null };
}

function capabilitySummary(caps) {
  if (!caps) return "";
  const parts = [];
  if (caps.context_limit) parts.push(`${formatCount(caps.context_limit)} context`);
  if (caps.tools === true) parts.push("tools");
  if (caps.reasoning === true) parts.push("reasoning");
  if (caps.structured_output === true) parts.push("structured output");
  const extraInputs = (caps.input_modalities || []).filter((kind) => kind !== "text");
  if (extraInputs.length) parts.push(`input: ${extraInputs.join(" + ")}`);
  if (!parts.length) {
    return caps.source === "ollama"
      ? "The runtime published no capabilities for this model."
      : "This provider published no capability metadata for the model.";
  }
  return `Reported by ${caps.source}: ${parts.join(", ")}.`;
}

function capabilityNote() {
  const note = document.createElement("p");
  note.className = "model-note model-capabilities";
  const { model, caps } = chosenCapabilities();
  const summary = capabilitySummary(caps);
  note.textContent = summary
    ? summary
    : model
      ? `Capabilities for ${model} are unknown — the provider published none.`
      : "";
  return note;
}

async function loadLocalCapabilities(name) {
  const result = await api(`/api/local/model-capabilities?name=${encodeURIComponent(name)}`);
  state.localCapabilities = result.ok && result.data && result.data.capabilities
    ? { name, capabilities: result.data.capabilities }
    : { name, capabilities: null };
  if ((state.chatModel || "") === `local:${name}`) renderModelMenu();
}

/* ---- Thinking control -------------------------------------------- */

const THINKING_KEYS = ["off", "low", "medium", "high"];

function thinkingDescription(level) {
  const entry = (state.thinkingLevels || []).find((item) => item.id === level);
  return entry ? entry.description : "";
}

function setThinking(level) {
  state.thinking = THINKING_KEYS.includes(level) ? level : "";
  try { localStorage.setItem("freeai-thinking", state.thinking); } catch { /* private mode */ }
  renderThinkingChip();
}

function renderThinkingChip() {
  const button = $("thinkBtn");
  const label = $("thinkLabel");
  if (!button || !label) return;
  label.textContent = `Think: ${state.thinking || "model default"}`;
  button.classList.toggle("active", Boolean(state.thinking));
  button.setAttribute("aria-pressed", String(Boolean(state.thinking)));
  button.title = state.thinking
    ? thinkingDescription(state.thinking) || `Asks for ${state.thinking} reasoning.`
    : "No reasoning parameter is sent; the model's own default applies.";
}

async function loadThinkingLevels() {
  const result = await api("/api/thinking/levels");
  if (result.ok && Array.isArray(result.data.levels)) state.thinkingLevels = result.data.levels;
  renderThinkingChip();
  if ($("thinkingMenu") && !$("thinkingMenu").classList.contains("hidden")) renderThinkingMenu();
}

function thinkingMenuItems() {
  const levels = state.thinkingLevels && state.thinkingLevels.length
    ? state.thinkingLevels
    : THINKING_KEYS.map((id) => ({ id, description: "" }));
  return [
    { id: "", description: "Send no reasoning parameter; the model decides." },
    ...levels,
  ];
}

function renderThinkingMenu() {
  const list = $("thinkingMenuList");
  if (!list) return;
  list.replaceChildren();
  thinkingMenuItems().forEach((entry) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "skill-menu-item" + (state.thinking === entry.id ? " active" : "");
    item.setAttribute("role", "option");
    item.setAttribute("aria-selected", String(state.thinking === entry.id));
    const main = document.createElement("span");
    main.textContent = entry.id || "Model default";
    item.appendChild(main);
    if (entry.description) {
      const note = document.createElement("small");
      note.textContent = entry.description;
      item.appendChild(note);
    }
    item.addEventListener("click", () => {
      setThinking(entry.id);
      toggleThinkingMenu(false);
    });
    list.appendChild(item);
  });
}

function toggleThinkingMenu(force) {
  const menu = $("thinkingMenu");
  if (!menu) return;
  const show = force != null ? force : menu.classList.contains("hidden");
  menu.classList.toggle("hidden", !show);
  $("thinkBtn").setAttribute("aria-expanded", String(show));
  if (show) renderThinkingMenu();
}

/* `@Thinking` / `@Thinking:high` typed at the start of the prompt is a command,
   and the server strips it before storing the message. `\@Thinking` is the
   literal form. The hint mirrors the server's rules exactly, so it never
   claims a command the backend will not run. */
function directiveLevel(text) {
  const trimmed = text.replace(/^\s+/, "");
  if (trimmed.startsWith("\\@")) return null;
  const match = trimmed.match(/^@thinking(?::([A-Za-z]+))?(?:\s+([A-Za-z]+))?/i);
  if (!match) return null;
  // An unknown level (`@Thinking:banana`) stays prose on the server too.
  if (match[1] && !THINKING_KEYS.includes(match[1].toLowerCase())) return null;
  const word = (match[2] || match[1] || "").toLowerCase();
  return THINKING_KEYS.includes(word) ? word : "";
}

function syncThinkingDirective() {
  const level = directiveLevel($("prompt").value);
  const hint = $("thinkHint");
  if (!hint) return;
  if (level === null) {
    hint.classList.add("hidden");
    return;
  }
  hint.classList.remove("hidden");
  hint.textContent = level
    ? `@Thinking ${level} — this message asks for ${level} reasoning.`
    : "@Thinking — this message asks for reasoning at the level set beside it.";
}

async function refreshPlatformMenuModels() {
  const result = await api("/api/openai/models");
  state.platformModels = result.ok && Array.isArray(result.data.models) ? result.data.models : null;
  renderModelMenu();
}

async function testOpenAIPlatform() {
  const saved = await saveSettings();
  if (!saved) return;
  const result = await api("/api/openai/test", { method: "POST", body: "{}" });
  if (result.ok && result.data.ok) {
    settingsNotice(result.data.message, true);
    return;
  }
  settingsNotice(result.data.error || "The platform key test failed.", false);
}

function modelMenuItem({ id, label, context, capabilities, chosen, onPick }) {
  const item = document.createElement("button");
  item.type = "button";
  item.className = "skill-menu-item model-item" + (chosen ? " active" : "");
  item.setAttribute("role", "option");
  item.setAttribute("aria-selected", String(chosen));
  const provider = providerOfModel(id);
  item.appendChild(providerBadge(provider));
  const main = document.createElement("span");
  main.className = "model-item-main";
  main.textContent = label || id;
  item.appendChild(main);
  const side = document.createElement("small");
  const ctx = context ? formatCount(context) : "";
  const marks = capabilities
    ? [
        capabilities.tools === true ? "tools" : "",
        capabilities.reasoning === true ? "reasoning" : "",
        capabilities.structured_output === true ? "json" : "",
      ].filter(Boolean).join("/")
    : "";
  side.textContent = [ctx, marks, modelDailyUsage(id)].filter(Boolean).join(" · ");
  if (side.textContent) item.appendChild(side);
  item.addEventListener("click", () => {
    onPick();
    try { localStorage.setItem("freeai-chat-model", state.chatModel || ""); } catch { /* private mode */ }
    toggleModelMenu(false);
    renderModelChipLabel();
  });
  return item;
}

function setChatMode(mode) {
  state.chatMode = mode;
  try { localStorage.setItem("freeai-chat-mode", mode); } catch { /* private mode */ }
  const build = mode !== "plan";
  $("modeBuild").classList.toggle("active", build);
  $("modeBuild").setAttribute("aria-pressed", String(build));
  $("modePlan").classList.toggle("active", !build);
  $("modePlan").setAttribute("aria-pressed", String(!build));
}

async function loadSettings() {
  const result = await api("/api/config");
  if (!result.ok) return;
  $("baseUrl").value = result.data.base_url || "";
  $("model").value = result.data.model || "auto";
  state.model = result.data.model || "auto";
  state.contextBudget = Number(result.data.context_budget_tokens) || 6000;
  $("contextBudget").value = state.contextBudget;
  renderModelChipLabel();
  loadModels();
  $("historyMessages").value = Number(result.data.history_messages) || 6;
  $("sendOutline").checked = result.data.send_workspace_outline !== false;
  $("searchProvider").value = result.data.search_provider || "";
  $("privacyExclusions").value = (result.data.privacy_exclusions || []).join(", ");
  if ($("indexEmbeddingModel")) $("indexEmbeddingModel").value = result.data.embedding_model || "";
  const adaptive = result.data.adaptive_routing === true;
  $("adaptiveRouting").checked = adaptive;
  $("cheapModel").value = result.data.cheap_model || "";
  $("strongModel").value = result.data.strong_model || "";
  $("adaptiveTiersRow").classList.toggle("hidden", !adaptive);
  $("openaiBaseUrl").value = result.data.openai_base_url || "https://api.openai.com/v1";
  const keychain = result.data.keychain || "os";
  $("openaiKey").value = "";
  $("openaiKey").placeholder = result.data.has_openai_key
    ? "Saved key (leave blank to keep it)"
    : "Paste an OpenAI Platform key";
  $("openaiKeyStatus").textContent = result.data.has_openai_key
    ? `A platform key is saved ${keychainStoreName(keychain)}. Leave the field blank to keep it.`
    : `No platform key saved. Needed for openai: models. A saved key goes to ${keychainStoreName(keychain)}.`;
  $("thinkingLevel").value = result.data.thinking_level || "";
  $("runThinkingLevel").value = result.data.run_thinking_level || "";
  $("agentModel").value = result.data.agent_model || "";
  $("fallbackModels").value = (result.data.fallback_models || []).join(", ");
  $("dailyBudget").value = result.data.daily_budget_usd ? String(result.data.daily_budget_usd) : "";
  $("maxConcurrentRuns").value = String(result.data.max_concurrent_runs || 1);
  void loadThinkingLevels();
  $("searchApiKey").value = "";
  $("searchApiKey").placeholder = result.data.has_search_key ? "Saved key (leave blank to keep it)" : "Paste an Exa or Tavily key";
  $("searchKeyStatus").textContent = result.data.has_search_key
    ? `A search key is saved ${keychainStoreName(keychain)}. Leave the field blank to keep it.`
    : `Needed for Deep search with Exa or Tavily. A saved key goes to ${keychainStoreName(keychain)}.`;
  updateContextMeter();
  $("modelChip").textContent = state.model;
  $("apiKey").value = "";
  $("apiKey").placeholder = result.data.has_api_key ? "Saved key (leave blank to keep it)" : "Paste your router API key";
  $("keyStatus").textContent = result.data.has_api_key
    ? `A key is saved ${keychainStoreName(keychain)}. Leave the field blank to keep it, or enter a new key to replace it.`
    : `No key saved yet. A saved key goes to ${keychainStoreName(keychain)}.`;
  renderBackendPresets();
  void loadPermissions();
}

// What the server says about where saved keys live, in words the settings
// panel can show. The mock store exists in dev and test runs; the OS store
// is the production case; `off` keeps secrets in the restricted config file.
function keychainStoreName(mode) {
  return { os: "in the OS credential store", mock: "in the dev mock keychain", off: "in the restricted local config file" }[mode] || "in the OS credential store";
}

// Remove one saved credential: the keychain entry and the config field both
// go. Blank inputs always mean "keep the saved key", so removal is its own
// deliberate action, gated on the same config.key rule as a key change.
async function forgetSavedKey(field, button, statusElement) {
  if (!confirm("Remove the saved key? Providers using it will stop working until you paste a new one.")) return;
  button.disabled = true;
  const result = await api("/api/config/forget", {
    method: "POST",
    body: JSON.stringify({ field }),
  });
  button.disabled = false;
  if (!result.ok) {
    // A 409 carries needs_approval the same way the save form does.
    if (result.status === 409) {
      const approved = await api("/api/config/forget", {
        method: "POST",
        body: JSON.stringify({ field, approve: true }),
      });
      if (approved.ok) {
        if (statusElement) statusElement.textContent = "Key removed.";
        toast("Saved key removed.");
        return;
      }
    }
    toast(result.data.error || "Could not remove the saved key.");
    return;
  }
  if (statusElement) statusElement.textContent = "Key removed.";
  toast("Saved key removed.");
}

/* ---- permissions: saved rules and what they decided lately --------- */

function permissionScopeLabel(scope) {
  return { once: "Once", session: "This session", project: "Project", always: "Always" }[scope] || scope;
}

function renderPermissionActions() {
  const select = $("permAction");
  if (!select || !state.permissions.actions.length) return;
  if (select.options.length === state.permissions.actions.length) return;
  select.replaceChildren();
  state.permissions.actions.forEach((action) => {
    const option = document.createElement("option");
    option.value = action.id;
    option.textContent = action.id;
    option.title = `${action.description} Default: ${action.default}.`;
    select.appendChild(option);
  });
}

function renderPermissionRules() {
  const host = $("permRules");
  if (!host) return;
  host.replaceChildren();
  if (!state.permissions.rules.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "No rules saved. Every action runs at its default: reads allowed, writes and commands ask first.";
    host.appendChild(note);
    return;
  }
  state.permissions.rules.forEach((rule) => {
    const row = document.createElement("div");
    row.className = "skill-row";
    const main = document.createElement("div");
    main.className = "skill-main";
    const strong = document.createElement("strong");
    strong.textContent = `${rule.action} · ${rule.target || "any target"}`;
    const detail = document.createElement("p");
    detail.textContent = `${permissionScopeLabel(rule.scope)} · ${rule.project ? `project ${rule.project}` : "every project"}`;
    main.append(strong, detail);
    const side = document.createElement("div");
    side.className = "skill-side";
    const badge = document.createElement("span");
    badge.className = `badge ${rule.decision}`;
    badge.textContent = rule.decision;
    const revoke = document.createElement("button");
    revoke.type = "button";
    revoke.className = "btn btn-quiet btn-small";
    revoke.textContent = "Revoke";
    revoke.addEventListener("click", async () => {
      revoke.disabled = true;
      const result = await api("/api/permissions/revoke", {
        method: "POST",
        body: JSON.stringify({ action: rule.action, target: rule.target, scope: rule.scope, project: rule.project }),
      });
      if (!result.ok) toast(result.data.error || "Could not revoke that rule.");
      else toast(`Revoked ${rule.action}${rule.target ? ` on ${rule.target}` : ""}.`);
      await loadPermissions();
    });
    side.append(badge, revoke);
    row.appendChild(main);
    row.appendChild(side);
    host.appendChild(row);
  });
}

async function loadPermissionAudit() {
  const host = $("permAudit");
  if (!host) return;
  const result = await api("/api/permissions/audit?limit=25");
  host.replaceChildren();
  if (!result.ok) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = result.data.error || "Could not read the audit trail.";
    host.appendChild(note);
    return;
  }
  const entries = result.data.entries || [];
  if (!entries.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "Nothing decided yet.";
    host.appendChild(note);
    return;
  }
  entries.forEach((entry) => {
    const item = document.createElement("li");
    const time = document.createElement("time");
    time.textContent = new Date(entry.created_at * 1000).toLocaleString();
    const what = document.createElement("span");
    what.textContent = `${entry.action} ${entry.target || "any target"} — ${entry.decision}`;
    const detail = document.createElement("span");
    detail.className = "perm-audit-detail";
    detail.textContent = entry.detail;
    item.append(time, what, detail);
    host.appendChild(item);
  });
}

async function loadPermissions() {
  const result = await api("/api/permissions");
  if (!result.ok) {
    const host = $("permRules");
    host.replaceChildren();
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = result.data.error || "Could not read permission rules.";
    host.appendChild(note);
    return;
  }
  state.permissions = { actions: result.data.actions || [], rules: result.data.rules || [] };
  renderPermissionActions();
  renderPermissionRules();
  void loadPermissionAudit();
}

/* ---- Diagnostics: the app's own records, projected ---------------- */

/* Phase 17: one timeline over the tables the app already writes. The server
   does the projection and the redaction; this only formats what it returns, so
   the panel cannot show a fact the backend did not record. */
async function loadDiagnostics() {
  const host = $("diagEvents");
  if (!host) return;
  const limitInput = $("diagLimit");
  const limit = Math.max(10, Math.min(500, Number(limitInput && limitInput.value) || 100));
  if (limitInput) limitInput.value = String(limit);
  const area = $("diagArea") ? $("diagArea").value : "";
  const query = `?limit=${limit}${area ? `&area=${encodeURIComponent(area)}` : ""}`;
  const result = await api(`/api/diagnostics${query}`);
  host.replaceChildren();
  const summaryLine = $("diagSummary");
  if (!result.ok) {
    const message = result.data.error || "Could not read the diagnostics timeline.";
    if (summaryLine) summaryLine.textContent = "";
    host.appendChild(pageNote(message));
    return;
  }
  const summary = result.data.summary || {};
  if (summaryLine) {
    const provider = summary.provider || {};
    const decisions = summary.permission_decisions || {};
    const agent = summary.agent || {};
    summaryLine.textContent = [
      `${provider.errors || 0} of ${provider.requests || 0} provider requests failed in 24 h`,
      `${decisions.deny || 0} denied · ${decisions.ask || 0} asked`,
      `${agent.live || 0} live run${agent.live === 1 ? "" : "s"}`,
    ].join(" · ");
  }
  const events = Array.isArray(result.data.events) ? result.data.events : [];
  if (!events.length) {
    host.appendChild(pageNote("Nothing recorded in this area yet. The timeline fills as you chat, run agents and touch privileged actions."));
    return;
  }
  events.forEach((entry) => {
    const item = document.createElement("li");
    const time = document.createElement("time");
    time.textContent = entry.at ? new Date(entry.at * 1000).toLocaleTimeString() : "";
    const what = document.createElement("span");
    what.className = `diag-row diag-${entry.level || "info"}`;
    what.textContent = `${entry.area} · ${entry.event}`;
    const detail = document.createElement("span");
    detail.className = "perm-audit-detail";
    const short = entry.correlation ? String(entry.correlation).slice(0, 8) : "";
    detail.textContent = [short ? `#${short}` : "", entry.detail].filter(Boolean).join(" · ");
    item.append(time, what, detail);
    host.appendChild(item);
  });
}

/* The bundle is generated server-side: the browser never assembles a payload
   that could then be blamed for what it contains. */
async function downloadDiagnosticsBundle() {
  const result = await api("/api/diagnostics/bundle");
  if (!result.ok) {
    toast(result.data.error || "Could not build a support bundle.");
    return;
  }
  const blob = new Blob([JSON.stringify(result.data, null, 2)], { type: "application/json" });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `saarthi-studio-support-${stamp}.json`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  toast("Support bundle saved. It holds no prompts, file bodies or key values.");
}

/* ---- MCP: servers, discovery and approved tool calls --------------- */

function mcpNotice(message, ok) {
  const box = $("mcpNotice");
  box.textContent = message;
  box.className = "notice " + (ok ? "success" : "error");
  box.classList.remove("hidden");
}

function mcpButton(label, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "btn btn-quiet btn-small";
  button.textContent = label;
  button.addEventListener("click", onClick);
  return button;
}

async function loadMcp() {
  const host = $("mcpServers");
  const [result, health] = await Promise.all([api("/api/mcp"), api("/api/mcp/health")]);
  host.replaceChildren();
  if (!result.ok) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = result.data.error || "Could not read the MCP registry.";
    host.appendChild(note);
    return;
  }
  state.mcp.servers = result.data.servers || [];
  if (health.ok) setMcpHealth(health.data.servers || []);
  renderMcpServers();
}

function setMcpHealth(rows) {
  state.mcp.health = {};
  rows.forEach((row) => {
    state.mcp.health[row.name] = row;
  });
}

function mcpHealthBadge(report) {
  const badge = document.createElement("span");
  const health = report ? report.health : "unknown";
  badge.className = "badge" + (health === "healthy" ? " on" : "");
  badge.textContent =
    health === "healthy" ? "Healthy" : health === "unhealthy" ? "Unhealthy" : "Not checked";
  return badge;
}

/* Health is this process's own observation, so it says what was seen and when,
   never a guess. A failed check keeps the reason the handshake failed. */
function mcpHealthLine(report) {
  const line = document.createElement("small");
  line.className = "skill-path";
  if (!report || report.health === "unknown") {
    line.textContent = "Never contacted in this session. Test it or press Check health.";
    return line;
  }
  if (report.health === "unhealthy") {
    line.textContent = `Last check failed: ${report.error || "no reason given"}`;
    return line;
  }
  const parts = [];
  if (report.version) parts.push(`server ${report.version}`);
  if (report.protocol) parts.push(`protocol ${report.protocol}`);
  parts.push(`${report.tools} tool${report.tools === 1 ? "" : "s"}`);
  if (report.latency_ms) parts.push(`${report.latency_ms} ms`);
  if (report.checked_at) parts.push(new Date(report.checked_at * 1000).toLocaleTimeString());
  line.textContent = parts.join(" · ");
  return line;
}

function mcpLogBlock(name) {
  const entries = state.mcp.logs[name] || [];
  const box = document.createElement("pre");
  box.className = "mcp-result mcp-log";
  box.textContent = entries.length
    ? entries
        .slice(-80)
        .map((entry) => {
          const at = new Date(entry.at * 1000).toLocaleTimeString();
          return `${at} ${entry.source === "server" ? "server" : "client"}: ${entry.text}`;
        })
        .join("\n")
    : "No output captured yet.";
  return box;
}

async function loadMcpLogs(name) {
  const result = await api(`/api/mcp/logs?name=${encodeURIComponent(name)}`);
  if (!result.ok) {
    mcpNotice(result.data.error || "Could not read those logs.", false);
    return;
  }
  state.mcp.logs[name] = result.data.entries || [];
}

async function toggleMcpLogs(name) {
  if (state.mcp.openLogs.has(name)) {
    state.mcp.openLogs.delete(name);
    renderMcpServers();
    return;
  }
  state.mcp.openLogs.add(name);
  await loadMcpLogs(name);
  renderMcpServers();
}

async function checkMcpHealth() {
  const result = await api("/api/mcp/health/check", {
    method: "POST",
    body: JSON.stringify({ approve: true }),
  });
  if (!result.ok) {
    mcpNotice(result.data.error || "The health check did not run.", false);
    return;
  }
  setMcpHealth(result.data.checked || []);
  const results = result.data.results || [];
  const failed = results.filter((entry) => !entry.ok);
  const skipped = result.data.skipped || [];
  for (const name of state.mcp.openLogs) await loadMcpLogs(name);
  renderMcpServers();
  const parts = [`${results.length - failed.length} healthy`];
  if (failed.length) parts.push(`${failed.length} failed`);
  if (skipped.length) parts.push(`${skipped.length} skipped by a saved rule`);
  mcpNotice(`Checked ${results.length} server${results.length === 1 ? "" : "s"}: ${parts.join(", ")}.`, !failed.length);
  if (failed.length) {
    mcpNotice(`Failed: ${failed.map((entry) => `${entry.name} — ${entry.error}`).join("; ")}`, false);
  }
}

async function renderMcpServers() {
  const host = $("mcpServers");
  host.replaceChildren();
  if (!state.mcp.servers.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = "No servers registered. Add one above with the command that starts it.";
    host.appendChild(note);
    return;
  }
  state.mcp.servers.forEach((server) => {
    const row = document.createElement("div");
    row.className = "skill-row" + (server.enabled ? "" : " off");
    const main = document.createElement("div");
    main.className = "skill-main";
    const strong = document.createElement("strong");
    strong.textContent = server.name;
    const line = document.createElement("p");
    line.textContent =
      server.transport && server.transport !== "stdio"
        ? `${server.transport} ${server.url}`
        : [server.command, ...(server.args || [])].join(" ");
    const meta = document.createElement("small");
    meta.className = "skill-path";
    const metaBits = [];
    if ((server.environment || []).length) {
      metaBits.push(`Environment: ${server.environment.join(", ")}`);
    }
    if ((server.headers || []).length) metaBits.push(`Headers: ${server.headers.join(", ")}`);
    meta.textContent = metaBits.length ? metaBits.join(" · ") : "No environment or headers set";
    main.append(strong, line, meta);
    const side = document.createElement("div");
    side.className = "skill-side";
    const report = state.mcp.health[server.name];
    const badge = document.createElement("span");
    badge.className = "badge" + (server.enabled ? " on" : "");
    badge.textContent = server.enabled ? "Enabled" : "Disabled";
    const actions = document.createElement("div");
    actions.className = "mcp-actions";
    actions.append(
      mcpButton("Edit", () => editMcpServer(server)),
      mcpButton("Test", () => void testMcpServer(server.name)),
      mcpButton("Logs", () => void toggleMcpLogs(server.name)),
      mcpButton("Remove", () => void removeMcpServer(server.name)),
    );
    if (server.transport && server.transport !== "stdio") {
      actions.append(oauthButton(server.name, state.mcp.oauth[server.name]));
    }
    main.append(mcpHealthLine(report));
    if (server.transport && server.transport !== "stdio") {
      main.append(mcpOAuthLine(server.name, state.mcp.oauth[server.name]));
    }
    side.append(badge, mcpHealthBadge(report), actions);
    row.append(main, side);
    host.appendChild(row);
    if (state.mcp.openLogs.has(server.name)) host.appendChild(mcpLogBlock(server.name));
  });
  // Fill the OAuth badge without blocking the panel on the round trips.
  for (const server of state.mcp.servers) {
    if (server.transport !== "stdio" && !state.mcp.oauth[server.name]) {
      const result = await api(`/api/mcp/${encodeURIComponent(server.name)}/oauth/status`);
      if (result.ok) {
        state.mcp.oauth[server.name] = result.data;
      }
    }
  }
  if (state.mcp.servers.some((server) => server.transport !== "stdio")) renderMcpServers();
}

/* OAuth status for one remote server, from whatever the panel last learned.
   Until the status round trip lands, unknown means "sign in" shows anyway. */
function oauthState(name) {
  return state.mcp.oauth[name];
}

function oauthButton(name, status) {
  const authorized = status && status.authorized;
  const manual = status && status.manual_header;
  const label = authorized ? "Sign out" : manual ? "Sign in (header set)" : "Sign in";
  const button = mcpButton(label, () =>
    void (authorized ? forgetMcpOAuth(name) : startMcpOAuth(name)),
  );
  if (authorized || manual) button.setAttribute("title", "OAuth stored in this machine's credential store");
  return button;
}

function mcpOAuthLine(name, status) {
  const line = document.createElement("small");
  line.className = "skill-path";
  if (!status) {
    line.textContent = "Checking sign-in status…";
  } else if (status.manual_header) {
    line.textContent = "Using a hand-written authorization header; OAuth sign-in is not used here.";
  } else if (status.authorized) {
    const when = status.expires_at ? `; expires ${new Date(status.expires_at * 1000).toLocaleString()}` : "; expiry unknown";
    line.textContent = `Signed in with OAuth${when}`;
  } else if (status.registered) {
    line.textContent = "Registered with the OAuth server but not signed in.";
  } else if (status.transport) {
    line.textContent = "Not signed in; token-based tools will need a sign-in first.";
  } else {
    line.textContent = "Not signed in.";
  }
  return line;
}

async function startMcpOAuth(name) {
  const result = await api(`/api/mcp/${encodeURIComponent(name)}/oauth/start`, {
    method: "POST",
    body: JSON.stringify({ approve: true }),
  });
  if (!result.ok) {
    mcpNotice(result.data.error || "The sign-in did not finish.", false);
    return;
  }
  mcpNotice(result.data.message || `Signed in to ${name}.`, true);
  state.mcp.oauth[name] = { ...(state.mcp.oauth[name] || {}), ...result.data, authorized: true };
  await loadMcp();
}

async function forgetMcpOAuth(name) {
  const result = await api(`/api/mcp/${encodeURIComponent(name)}/oauth/forget`, {
    method: "POST",
    body: JSON.stringify({ name }),
  });
  if (!result.ok) {
    mcpNotice(result.data.error || "Could not remove those credentials.", false);
    return;
  }
  mcpNotice(`Removed ${result.data.message || "the saved credentials"}.`, true);
  state.mcp.oauth[name] = null;
  await loadMcp();
}

function editMcpServer(server) {
  $("mcpName").value = server.name;
  $("mcpTransport").value = server.transport || "stdio";
  $("mcpCommand").value = server.command;
  $("mcpUrl").value = server.url || "";
  $("mcpArgs").value = (server.args || []).join(" ");
  $("mcpEnv").value = "";
  $("mcpEnv").placeholder = (server.environment || []).length
    ? `Saved: ${server.environment.join(", ")} (leave blank to keep)`
    : "TOKEN=… , LOG=debug";
  $("mcpHeaders").value = "";
  $("mcpHeaders").placeholder = (server.headers || []).length
    ? `Saved: ${server.headers.join(", ")} (leave blank to keep)`
    : "Authorization=Bearer …";
  $("mcpEnabled").checked = server.enabled;
  mcpNotice(
    `Editing ${server.name}. Its saved environment and headers are kept unless you type new ones.`,
    true,
  );
}

/* One parser for both write-only maps: env is KEY=value, headers are
   Name=value, and neither value is ever returned by the API. */
function mcpPairs(text) {
  const pairs = {};
  for (const part of text.split(",")) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const at = trimmed.indexOf("=");
    if (at <= 0) return null;
    pairs[trimmed.slice(0, at).trim()] = trimmed.slice(at + 1);
  }
  return pairs;
}

async function saveMcpServer() {
  const name = $("mcpName").value.trim();
  const transport = $("mcpTransport").value;
  const command = $("mcpCommand").value.trim();
  const url = $("mcpUrl").value.trim();
  if (!name) {
    mcpNotice("A server needs a name.", false);
    return;
  }
  if (transport === "stdio" && !command) {
    mcpNotice("A stdio server needs a command to run.", false);
    return;
  }
  if (transport !== "stdio" && !/^https?:\/\/\S+$/.test(url)) {
    mcpNotice("A remote server needs an http:// or https:// URL.", false);
    return;
  }
  const payload = {
    name,
    transport,
    command,
    url,
    args: $("mcpArgs").value.trim() ? $("mcpArgs").value.trim().split(/\s+/) : [],
    enabled: $("mcpEnabled").checked,
  };
  const envText = $("mcpEnv").value.trim();
  if (envText) {
    const env = mcpPairs(envText);
    if (!env) {
      mcpNotice("Environment entries must look like KEY=value.", false);
      return;
    }
    payload.env = env;
  }
  const headerText = $("mcpHeaders").value.trim();
  if (headerText) {
    const headers = mcpPairs(headerText);
    if (!headers) {
      mcpNotice("Header entries must look like Name=value.", false);
      return;
    }
    payload.headers = headers;
  }
  const result = await api("/api/mcp/servers", { method: "POST", body: JSON.stringify(payload) });
  if (!result.ok) {
    mcpNotice(result.data.error || "Could not save that server.", false);
    return;
  }
  $("mcpEnv").value = "";
  mcpNotice(`Saved ${name}. Press Test to discover its tools.`, true);
  await loadMcp();
}

async function removeMcpServer(name) {
  const result = await api("/api/mcp/servers/remove", { method: "POST", body: JSON.stringify({ name }) });
  if (!result.ok) {
    mcpNotice(result.data.error || "Could not remove that server.", false);
    return;
  }
  if (state.mcp.target && state.mcp.target.server === name) {
    state.mcp.target = null;
    state.mcp.tools = [];
    renderMcpTools();
  }
  mcpNotice(`Removed ${name}.`, true);
  await loadMcp();
}

async function testMcpServer(name) {
  // The Test button is the approval: a deliberate click that starts a process
  // the user registered themselves.
  const result = await api("/api/mcp/test", {
    method: "POST",
    body: JSON.stringify({ name, approve: true }),
  });
  if (!result.ok) {
    mcpNotice(result.data.error || "The server did not answer.", false);
    return;
  }
  state.mcp.tools = result.data.tools || [];
  state.mcp.target = { server: name, tool: null };
  renderMcpTools();
  const count = state.mcp.tools.length;
  mcpNotice(`${name} answered with ${count} tool${count === 1 ? "" : "s"}.`, true);
  await loadMcp();
}

/* Pre-fill only what the schema says is required, with the right shape for
   each declared type, so the field starts as a valid guess, not a blank. */
function mcpArgumentTemplate(schema) {
  const properties = (schema && schema.properties) || {};
  const required = new Set((schema && schema.required) || []);
  const template = {};
  Object.keys(properties).forEach((key) => {
    if (!required.has(key)) return;
    const type = properties[key].type;
    if (type === "number" || type === "integer") template[key] = 0;
    else if (type === "boolean") template[key] = false;
    else if (type === "array") template[key] = [];
    else if (type === "object") template[key] = {};
    else template[key] = "";
  });
  return template;
}

function renderMcpTools() {
  const host = $("mcpTools");
  host.replaceChildren();
  if (!state.mcp.tools.length) {
    const note = document.createElement("p");
    note.className = "empty-note";
    note.textContent = state.mcp.target
      ? "That server listed no tools."
      : "Test a server to see the tools it offers.";
    host.appendChild(note);
    return;
  }
  state.mcp.tools.forEach((tool) => {
    const row = document.createElement("div");
    const picked = Boolean(state.mcp.target && state.mcp.target.tool === tool.name);
    row.className = "mcp-tool" + (picked ? " picked" : "");
    const head = document.createElement("strong");
    head.textContent = tool.name;
    const body = document.createElement("p");
    body.textContent = tool.description || "No description.";
    const required = (tool.input_schema && tool.input_schema.required) || [];
    const meta = document.createElement("small");
    meta.textContent = required.length
      ? `Required: ${required.join(", ")}`
      : "No required arguments";
    row.append(head, body, meta);
    row.addEventListener("click", () => {
      state.mcp.target = {
        server: state.mcp.target ? state.mcp.target.server : null,
        tool: tool.name,
      };
      $("mcpArgsJson").value = JSON.stringify(mcpArgumentTemplate(tool.input_schema), null, 2);
      $("mcpResult").classList.add("hidden");
      const button = $("mcpCall");
      button.textContent = "Call tool";
      delete button.dataset.approve;
      renderMcpTools();
    });
    host.appendChild(row);
  });
}

async function callMcpTool() {
  const target = state.mcp.target || {};
  const button = $("mcpCall");
  const approve = button.dataset.approve === "1";
  if (!target.server || !target.tool) {
    mcpNotice("Test a server and pick a tool first.", false);
    return;
  }
  let args;
  const raw = $("mcpArgsJson").value.trim();
  try {
    args = raw ? JSON.parse(raw) : {};
  } catch {
    mcpNotice("Arguments must be valid JSON.", false);
    return;
  }
  const result = await api("/api/mcp/call", {
    method: "POST",
    body: JSON.stringify({ server: target.server, tool: target.tool, arguments: args, approve }),
  });
  // The store answered "ask": the next click is the approval, not a silent retry.
  if (result.status === 409 && result.data.needs_approval) {
    mcpNotice(`${target.server}.${target.tool} needs approval before it runs.`, false);
    button.textContent = "Approve and call";
    button.dataset.approve = "1";
    return;
  }
  button.textContent = "Call tool";
  delete button.dataset.approve;
  if (!result.ok) {
    mcpNotice(result.data.error || "The tool call failed.", false);
    return;
  }
  const box = $("mcpResult");
  box.textContent = result.data.text || "(the result carried no text)";
  box.classList.remove("hidden");
  mcpNotice(`${target.server}.${target.tool} returned.`, true);
}

async function addPermissionRule() {
  const action = $("permAction").value;
  const decision = $("permDecision").value;
  const scope = $("permScope").value;
  const target = $("permTarget").value.trim();
  if (!action) {
    toast("Pick an action first.");
    return;
  }
  if (scope === "project" && !state.activeProjectId) {
    toast("Open a project before saving a project-scoped rule.");
    return;
  }
  const result = await api("/api/permissions/grant", {
    method: "POST",
    body: JSON.stringify({
      action,
      target,
      decision,
      scope,
      project: scope === "project" ? state.activeProjectId : "",
    }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not save that rule.");
    return;
  }
  $("permTarget").value = "";
  toast(`Saved: ${decision} ${action}${target ? ` on ${target}` : ""}.`);
  await loadPermissions();
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
    search_provider: $("searchProvider").value,
    privacy_exclusions: $("privacyExclusions").value.split(",").map((s) => s.trim()).filter(Boolean),
    adaptive_routing: $("adaptiveRouting").checked,
    cheap_model: $("cheapModel").value.trim(),
    strong_model: $("strongModel").value.trim(),
    openai_base_url: $("openaiBaseUrl").value.trim() || "https://api.openai.com/v1",
    thinking_level: $("thinkingLevel").value,
    run_thinking_level: $("runThinkingLevel").value,
    agent_model: $("agentModel").value.trim(),
    fallback_models: $("fallbackModels").value.split(",").map((s) => s.trim()).filter(Boolean),
    daily_budget_usd: Number($("dailyBudget").value) || 0,
    max_concurrent_runs: Number($("maxConcurrentRuns").value) || 1,
  };
  $("adaptiveTiersRow").classList.toggle("hidden", !$("adaptiveRouting").checked);
  const key = $("apiKey").value.trim();
  if (key) payload.api_key = key;
  const openaiKey = $("openaiKey").value.trim();
  if (openaiKey) payload.openai_api_key = openaiKey;
  const searchKey = $("searchApiKey").value.trim();
  if (searchKey) payload.search_api_key = searchKey;
  const result = await api("/api/config", {
    method: "POST",
    confirmApproval: true,
    body: JSON.stringify(payload),
  });
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
  if ($("openaiKey").value.trim()) {
    $("openaiKey").value = "";
    $("openaiKey").placeholder = "Saved key (leave blank to keep it)";
    $("openaiKeyStatus").textContent = "A platform key is saved locally. Leave the field blank to keep it.";
  }
  if ($("searchApiKey").value.trim()) {
    $("searchApiKey").value = "";
    $("searchApiKey").placeholder = "Saved key (leave blank to keep it)";
    $("searchKeyStatus").textContent = "A search key is saved. Leave the field blank to keep it.";
  }
  settingsNotice("Settings saved locally.", true);
  setConnection("ok", "Settings saved");
  void loadModels();
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

/* Report the router's real state at launch. GET /api/router answers from the
   server's own probe, so a slow or missing upstream never blocks first paint. */
async function probeRouter() {
  const result = await api("/api/router");
  if (!result.ok) {
    setConnection("bad", "Router unreachable");
    return;
  }
  const status = result.data || {};
  if (status.reachable) {
    setConnection("ok", "Router connected");
  } else {
    setConnection("bad", "Router not running");
  }
}

/* Dashboard URL comes from the active preset: hosted providers have a known
   dashboard; self-hosted gateways (FreeLLMAPI, OmniRoute) host theirs at the
   gateway root (base URL without /v1). */
function providerDashboardUrl() {
  const baseUrl = $("baseUrl") ? $("baseUrl").value.trim() : "";
  const preset = presetForBaseUrl(baseUrl);
  if (preset && preset.dashboardUrl) return preset.dashboardUrl;
  if (preset || /:\d+\/v1\/?$/.test(baseUrl)) return baseUrl.replace(/\/v1\/?$/, "");
  if (/localhost|127\.0\.0\.1/.test(baseUrl)) return baseUrl.replace(/\/v1\/?$/, "");
  return null;
}

async function openProviderDashboard() {
  const url = providerDashboardUrl();
  if (!url) {
    toast("Pick a backend with a dashboard first (Settings → Connection).");
    return;
  }
  // window.open works in the browser UI; the desktop window can block popups,
  // so fall back to asking the server to open it.
  const opened = window.open(url, "_blank", "noopener,noreferrer");
  if (!opened) {
    const result = await api("/api/open", { method: "POST", body: JSON.stringify({ url }) });
    if (!result.ok) toast(result.data.error || "Could not open the dashboard.");
  }
}

/* ------------------------------------------------------------------ */
/* extension pages: workspaces, agents, integrations, artifacts, automation */
/* ------------------------------------------------------------------ */

function pageNote(message) {
  const note = document.createElement("p");
  note.className = "state";
  note.textContent = message;
  return note;
}

function rowButton(label, handler, dangerClass = "") {
  const button = document.createElement("button");
  button.type = "button";
  button.className = `tree-tool ${dangerClass}`.trim();
  button.textContent = label;
  button.addEventListener("click", handler);
  return button;
}

/* Workspaces: the same server-backed project list the switcher uses, with
   the open-folder controls from the Files pane collected in one place. */
async function renderWorkspaces() {
  const host = $("workspaceProjects");
  if (!host) return;
  host.replaceChildren();
  const result = await api("/api/projects");
  if (!result.ok) {
    host.appendChild(pageNote(result.data.error || "Projects unavailable."));
    return;
  }
  const projects = Array.isArray(result.data.projects) ? result.data.projects : [];
  if (!projects.length) {
    host.appendChild(pageNote("No projects yet. Create one and its contents are scoped server-side."));
  }
  projects.forEach((project) => {
    const row = document.createElement("div");
    row.className = "project-menu-item" + (project.id === state.activeProjectId ? " active" : "");
    const main = document.createElement("span");
    main.className = "project-menu-main";
    const strong = document.createElement("strong");
    strong.textContent = project.name + (project.archived ? " (archived)" : "");
    main.appendChild(strong);
    const small = document.createElement("small");
    small.textContent = project.folder || "no folder";
    main.appendChild(small);
    row.appendChild(main);
    const actions = document.createElement("span");
    actions.className = "project-menu-actions-row";
    if (!project.archived) actions.appendChild(rowButton("Switch", () => switchProject(project.id)));
    actions.appendChild(rowButton("Rename", () => renameProject(project)));
    actions.appendChild(rowButton(project.archived ? "Unarchive" : "Archive", () => updateProject(project, { archived: !project.archived })));
    actions.appendChild(rowButton("Delete", () => deleteProject(project), "danger"));
    row.appendChild(actions);
    host.appendChild(row);
  });

  const openHost = $("workspaceOpen");
  if (!openHost) return;
  openHost.replaceChildren();
  const openFolderRow = document.createElement("div");
  openFolderRow.className = "project-menu-item";
  const openMain = document.createElement("span");
  openMain.className = "project-menu-main";
  const openStrong = document.createElement("strong");
  openStrong.textContent = state.workspace ? state.workspace.name : "No folder open";
  openMain.appendChild(openStrong);
  const openSmall = document.createElement("small");
  openSmall.textContent = state.workspace ? state.workspace.path : "Browse recent folders to reopen one.";
  openMain.appendChild(openSmall);
  openFolderRow.appendChild(openMain);
  const openActions = document.createElement("span");
  openActions.className = "project-menu-actions-row";
  openActions.appendChild(rowButton("Open folder…", openFolderDialog));
  if (state.workspace) openActions.appendChild(rowButton("Close", closeWorkspace, "danger"));
  openFolderRow.appendChild(openActions);
  openHost.appendChild(openFolderRow);
  state.recentFolders.slice(0, 5).forEach((path) => {
    const row = document.createElement("div");
    row.className = "project-menu-item";
    row.innerHTML = `<span class="project-menu-main"><small title="${path}">${path}</small></span>`;
    row.appendChild(rowButton("Open", () => openWorkspacePath(path)));
    openHost.appendChild(row);
  });
}

/* Agents: the enabled skills as runnable agents. Attaching flips the same
   `activeSkillId` state the composer chip reads, so chat and this page agree. */
function renderAgents() {
  const host = $("agentsList");
  if (!host) return;
  host.replaceChildren();
  const usable = state.skills.filter((skill) => !skill.error && skill.enabled !== false);

  // Controls: search box + per-page count, above the list, survive page and
  // query changes for this session via state.agentsQuery.
  if (state.agentsQuery == null) {
    let perPage = 10;
    try { perPage = Number(localStorage.getItem("freeai-agents-per-page")) || 10; } catch { /* private mode */ }
    state.agentsQuery = { query: "", page: 1, perPage: Math.min(Math.max(perPage, 1), 50) };
  }
  if (!host.querySelector(".agents-controls")) {
    const controls = document.createElement("div");
    controls.className = "agents-controls";
    const search = document.createElement("input");
    search.type = "search";
    search.className = "agents-search";
    search.placeholder = "Search agents by name or description…";
    search.value = state.agentsQuery.query;
    search.setAttribute("aria-label", "Search agents");
    search.addEventListener("input", () => {
      state.agentsQuery.query = search.value.trim().toLowerCase();
      state.agentsQuery.page = 1;
      renderAgents();
      // Keep typing focus sticky despite the re-render.
      const next = host.querySelector(".agents-search");
      if (next) { next.focus(); next.setSelectionRange(next.value.length, next.value.length); }
    });
    const per = document.createElement("label");
    per.className = "agents-per-page";
    per.textContent = "Per page";
    const select = document.createElement("select");
    for (const count of [10, 25, 50]) {
      const option = document.createElement("option");
      option.value = String(count);
      option.textContent = count;
      select.appendChild(option);
    }
    select.value = String(state.agentsQuery.perPage);
    select.addEventListener("change", () => {
      state.agentsQuery.perPage = Number(select.value) || 10;
      state.agentsQuery.page = 1;
      try { localStorage.setItem("freeai-agents-per-page", String(select.value)); } catch { /* private mode */ }
      renderAgents();
    });
    per.appendChild(select);
    controls.appendChild(search);
    controls.appendChild(per);
    host.appendChild(controls);
  }

  const filtered = usable.filter((skill) => {
    if (!state.agentsQuery.query) return true;
    const haystack = `${skill.id} ${skill.name || ""} ${skill.summary || ""} ${skill.description || ""}`.toLowerCase();
    return haystack.includes(state.agentsQuery.query);
  });
  const pages = Math.ceil(filtered.length / state.agentsQuery.perPage) || 1;
  const page = Math.min(state.agentsQuery.page, pages);
  state.agentsQuery.page = page;
  const slice = filtered.slice((page - 1) * state.agentsQuery.perPage, page * state.agentsQuery.perPage);
  const count = document.createElement("p");
  count.className = "muted agents-count";
  count.textContent = `${filtered.length} agent${filtered.length === 1 ? "" : "s"}`;
  host.appendChild(count);

  if (!filtered.length) {
    host.appendChild(pageNote(state.agentsQuery.query
      ? `No agent matches "${state.agentsQuery.query}".`
      : "No enabled skills. Add a SKILL.md folder to this project, your config folder, or ~/.agents/skills, then enable it on the Skills page."));
    const pager = renderAgentsPager(pages, page, host);
    host.appendChild(pager);
    return;
  }
  const list = document.createElement("div");
  list.className = "skill-list";
  host.appendChild(list);
  slice.forEach((skill) => {
    const row = document.createElement("div");
    row.className = "skill-row" + (effectiveSkillId() === skill.id ? " active" : "");
    const main = document.createElement("div");
    main.className = "skill-main";
    const strong = document.createElement("strong");
    strong.textContent = (skill.name || skill.id) + (effectiveSkillId() === skill.id ? " — attached" : "");
    main.appendChild(strong);
    const desc = document.createElement("p");
    desc.textContent = skill.summary || skill.description || "No description in SKILL.md.";
    main.appendChild(desc);
    const path = document.createElement("small");
    path.className = "skill-path";
    path.textContent = skill.path || "";
    main.appendChild(path);
    row.appendChild(main);
    const actions = document.createElement("span");
    actions.className = "project-menu-actions-row";
    actions.appendChild(rowButton("Inspect", () => openSkillDetail(skill.id)));
    actions.appendChild(rowButton(
      effectiveSkillId() === skill.id ? "Detach" : "Attach",
      () => {
        state.activeSkillId = effectiveSkillId() === skill.id ? null : skill.id;
        state.suggestedSkillId = null;
        renderSkillChip();
        renderAgents();
        toast(state.activeSkillId ? `${skill.name || skill.id} attached to your next message.` : "Skill detached.");
        setView("chat");
      },
    ));
    row.appendChild(actions);
    list.appendChild(row);
  });
  host.appendChild(renderAgentsPager(pages, page, host));
}

function renderAgentsPager(pages, page, host) {
  const pager = document.createElement("div");
  pager.className = "usage-pager";
  const goto = (target) => { state.agentsQuery.page = target; renderAgents(); host.scrollIntoView({ block: "start" }); };
  const prev = document.createElement("button");
  prev.className = "pager-btn";
  prev.textContent = "‹ Prev";
  prev.disabled = page <= 1;
  prev.addEventListener("click", () => goto(page - 1));
  pager.appendChild(prev);
  const windowed = new Set([1, pages, page - 1, page, page + 1].filter((n) => n >= 1 && n <= pages));
  let last = 0;
  for (let number = 1; number <= pages; number++) {
    if (!windowed.has(number)) continue;
    if (last && number - last > 1) {
      const gap = document.createElement("span");
      gap.className = "pager-gap";
      gap.textContent = "…";
      pager.appendChild(gap);
    }
    const button = document.createElement("button");
    button.className = "pager-btn" + (number === page ? " active" : "");
    button.textContent = number;
    button.addEventListener("click", () => goto(number));
    pager.appendChild(button);
    last = number;
  }
  const next = document.createElement("button");
  next.className = "pager-btn";
  next.textContent = "Next ›";
  next.disabled = page >= pages;
  next.addEventListener("click", () => goto(page + 1));
  pager.appendChild(next);
  return pager;
}

/* Integrations: live status of everything Studio talks to, one tester per card. */
/* Each card: title, status note, then one or more actions (Test, Start, Stop). */
function integrationCard(title, status, note, actions = []) {
  const card = document.createElement("div");
  card.className = "project-menu-item";
  const main = document.createElement("span");
  main.className = "project-menu-main";
  const strong = document.createElement("strong");
  strong.textContent = title;
  main.appendChild(strong);
  const small = document.createElement("small");
  small.className = status === "ok" ? "ok" : "err";
  small.textContent = note;
  main.appendChild(small);
  card.appendChild(main);
  const row = document.createElement("span");
  row.className = "project-menu-actions-row";
  actions.forEach(([label, handler]) => row.appendChild(rowButton(label, handler)));
  card.appendChild(row);
  return card;
}

function activeBackendName() {
  const preset = presetForBaseUrl($("baseUrl").value.trim());
  return preset ? preset.name : "Custom backend";
}

async function loadIntegrations() {
  const host = $("integrationsList");
  if (!host) return;
  host.replaceChildren();

  const router = await api("/api/router");
  const reachable = router.ok && router.data.reachable;
  const routerActions = [["Test", () => loadIntegrations()]];
  if (!reachable && router.ok && router.data.compose_dir) {
    routerActions.push(["Start", async () => {
      const started = await api("/api/router/start", { method: "POST", body: "{}" });
      toast(started.ok ? "Starting FreeLLMAPI…" : started.data.error || "Could not start.");
      setTimeout(loadIntegrations, 2500);
    }]);
  }
  host.appendChild(integrationCard(
    `${activeBackendName()} (active backend)`,
    reachable ? "ok" : "err",
    reachable ? router.data.message || "reachable" : (router.ok ? router.data.message || "not running" : "status call failed"),
    routerActions,
  ));

  const omni = await api("/api/omniroute");
  const omniUp = omni.ok && omni.data.reachable;
  const omniActions = [];
  if (omni.ok && !omniUp) {
    omniActions.push(["Start", async () => {
      const started = await api("/api/omniroute/start", { method: "POST", body: "{}" });
      toast(started.ok ? "Starting OmniRoute…" : started.data.error || "Could not start OmniRoute.");
      setTimeout(loadIntegrations, 2500);
    }]);
  }
  if (omni.ok && omni.data.owned_by_app) {
    omniActions.push(["Stop", async () => {
      const stopped = await api("/api/omniroute/stop", { method: "POST", body: "{}" });
      toast(stopped.ok ? "Stopping OmniRoute…" : stopped.data.error || "Could not stop OmniRoute.");
      setTimeout(loadIntegrations, 2500);
    }]);
  }
  omniActions.push(["Test", () => loadIntegrations()]);
  host.appendChild(integrationCard(
    "OmniRoute",
    omniUp ? "ok" : "err",
    omni.ok ? omni.data.message || (omniUp ? "reachable" : "not running") : "status call failed",
    omniActions,
  ));

  host.appendChild(integrationCard(
    "Workspace filesystem",
    state.workspace ? "ok" : "err",
    state.workspace ? `open: ${state.workspace.path}` : "no folder open",
    [["Test", () => toast(state.workspace ? `Workspace open at ${state.workspace.path}.` : "Open a folder from the Files page first.")]],
  ));
  const test = await api("/api/test", { method: "POST", body: "{}" });
  host.appendChild(integrationCard(
    "Chat completion round-trip",
    test.ok && test.data.ok ? "ok" : "err",
    test.ok && test.data.ok ? test.data.message || "round-trip succeeded" : test.data.error || "test failed",
    [["Test", testConnection]],
  ));
}

/* Artifacts: flat file list of the open folder, grouped by extension. */
async function renderArtifacts() {
  const host = $("artifactsList");
  if (!host) return;
  host.replaceChildren();
  if (!state.workspace) {
    host.appendChild(pageNote("Open a folder on the Files page to list its files here."));
    return;
  }
  const result = await api("/api/workspace/files");
  if (!result.ok) {
    host.appendChild(pageNote(result.data.error || "File listing failed."));
    return;
  }
  const groups = new Map();
  (result.data.files || []).forEach((file) => {
    const key = file.ext || "(no extension)";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(file);
  });
  if (!groups.size) {
    host.appendChild(pageNote("No readable files in the open folder."));
    return;
  }
  [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0])).forEach(([ext, files]) => {
    const row = document.createElement("div");
    row.className = "project-menu-item";
    const main = document.createElement("span");
    main.className = "project-menu-main";
    const strong = document.createElement("strong");
    strong.textContent = ext;
    main.appendChild(strong);
    const small = document.createElement("small");
    small.textContent = `${files.length} file${files.length === 1 ? "" : "s"}`;
    main.appendChild(small);
    row.appendChild(main);
    files.forEach((file) => {
      const openButton = document.createElement("button");
      openButton.type = "button";
      openButton.className = "tree-tool";
      openButton.textContent = file.path;
      openButton.title = `${file.path} (${file.size} bytes)`;
      openButton.addEventListener("click", () => openFile(file.path, { switchTo: true }));
      row.appendChild(openButton);
    });
    host.appendChild(row);
  });
  if (result.data.truncated) {
    const note = pageNote("Listing was cut at the entry limit — use the Files tree for the full picture.");
    host.appendChild(note);
  }
}

/* Automation: CRUD on saved workflows plus a one-button run that loads the
   prompt and optional skill into a fresh chat round. */
function renderWorkflows(workflows) {
  const host = $("automationList");
  if (!host) return;
  host.replaceChildren();
  if (!workflows.length) {
    host.appendChild(pageNote("No workflows saved yet. Save your first recipe with the button below."));
    return;
  }
  workflows.forEach((workflow) => {
    const row = document.createElement("div");
    row.className = "project-menu-item";
    const main = document.createElement("span");
    main.className = "project-menu-main";
    const strong = document.createElement("strong");
    strong.textContent = workflow.name;
    main.appendChild(strong);
    const small = document.createElement("small");
    small.textContent = workflow.prompt;
    small.title = workflow.prompt;
    main.appendChild(small);
    row.appendChild(main);
    const actions = document.createElement("span");
    actions.className = "project-menu-actions-row";
    actions.appendChild(rowButton("Run", () => runWorkflow(workflow)));
    if (workflow.skill) actions.appendChild(rowButton("Detach skill", () => editWorkflow(workflow, { skill: "" })));
    actions.appendChild(rowButton("Edit", () => editWorkflow(workflow)));
    actions.appendChild(rowButton("Delete", () => deleteWorkflow(workflow), "danger"));
    row.appendChild(actions);
    host.appendChild(row);
  });
}

async function loadWorkflows() {
  const host = $("automationList");
  if (!host) return;
  const result = await api("/api/workflows");
  if (!result.ok) {
    host.replaceChildren();
    host.appendChild(pageNote(result.data.error || "Could not load workflows."));
    return;
  }
  renderWorkflows(Array.isArray(result.data.workflows) ? result.data.workflows : []);
}

function fillSkillOptions(selectedId) {
  const select = $("workflowSkill");
  if (!select) return;
  select.replaceChildren();
  const blank = document.createElement("option");
  blank.value = "";
  blank.textContent = "No skill";
  select.appendChild(blank);
  state.skills.filter((skill) => !skill.error && skill.enabled !== false).forEach((skill) => {
    const option = document.createElement("option");
    option.value = skill.id;
    option.textContent = skill.name || skill.id;
    if (skill.id === selectedId) option.selected = true;
    select.appendChild(option);
  });
}

function editWorkflow(workflow, override = {}) {
  $("workflowName").value = workflow.name || "";
  $("workflowPrompt").value = workflow.prompt || "";
  fillSkillOptions(override.skill != null ? override.skill : workflow.skill || "");
  $("workflowForm").dataset.editing = workflow.id || "";
  $("workflowForm").classList.remove("hidden");
  $("workflowName").focus();
}

async function deleteWorkflow(workflow) {
  if (!confirm(`Delete the workflow "${workflow.name}"?`)) return;
  const result = await api(`/api/workflows/${encodeURIComponent(workflow.id)}/delete`, { method: "POST", body: "{}" });
  if (!result.ok) {
    toast(result.data.error || "Could not delete that workflow.");
    return;
  }
  toast("Workflow deleted.");
  loadWorkflows();
}

async function runWorkflow(workflow) {
  state.activeSkillId = workflow.skill || null;
  state.suggestedSkillId = null;
  renderSkillChip();
  $("prompt").value = workflow.prompt;
  autoGrow($("prompt"));
  setView("chat");
  $("prompt").focus();
  toast(`${workflow.name} loaded — send when ready.`);
}

async function loadUsage() {
  const query = state.usageQuery;
  const params = new URLSearchParams();
  params.set("page", query.page);
  params.set("page_size", "10");
  params.set("sort", query.sort);
  params.set("dir", query.dir);
  for (const key of ["model", "provider"]) if (query[key]) params.set(key, query[key]);
  // Date inputs are day-granular: from includes its whole day, to ends at 23:59:59.
  if (query.from) params.set("from", Date.parse(`${query.from}T00:00:00`) / 1000);
  if (query.to) params.set("to", (Date.parse(`${query.to}T23:59:59`) / 1000) || "");
  const result = await api(`/api/usage?${params}`);
  if (!result.ok) {
    const body = $("usageRows");
    if (body) body.innerHTML = '<tr><td colspan="8" class="muted">Could not load usage records.</td></tr>';
    return;
  }
  const summary = result.data.summary || {};
  $("statRequests").textContent = summary.requests || 0;
  $("statSuccess").textContent = summary.successful || 0;
  $("statErrors").textContent = summary.failed || 0;
  $("statTokens").textContent = Number(summary.total_tokens || 0).toLocaleString();
  $("statLatency").textContent = summary.avg_latency_ms
    ? `${Math.round(summary.avg_latency_ms)} ms`
    : "—";
  // Spend is a floor, not a total: requests whose provider published no price
  // are counted next to it rather than silently costing zero.
  const cost = result.data.cost || {};
  $("statSpend").textContent = cost.usd_24h == null ? "—" : `$${Number(cost.usd_24h).toFixed(4)}`;
  const budget = Number(cost.budget_usd) || 0;
  const spendParts = [budget > 0 ? `of $${budget.toFixed(2)} budget` : "no budget set"];
  if (cost.unpriced_24h) spendParts.push(`${cost.unpriced_24h} unpriced`);
  const spendNote = $("statSpendNote");
  if (spendNote) {
    spendNote.textContent = spendParts.join(" · ");
    spendNote.title = cost.basis || "";
  }
  state.usageMeta = result.data;
  const requests = Array.isArray(result.data.requests) ? result.data.requests : [];
  state.usageRows = requests;
  renderUsageTable();
  renderUsagePager(result.data);
  renderUsageSeries(Array.isArray(result.data.series) ? result.data.series : []);
  renderUsageByModel(Array.isArray(result.data.by_model) ? result.data.by_model : []);
  renderUsageHead();
  loadProviderHealth();
}

function renderUsageTable() {
  const body = $("usageRows");
  if (!body) return;
  body.replaceChildren();
  const requests = state.usageRows;
  if (!requests.length) {
    body.innerHTML = '<tr><td colspan="8" class="muted">No requests match. Send a message to start recording usage, or clear the filters.</td></tr>';
    return;
  }
  requests.forEach((request) => {
    const row = document.createElement("tr");
    const provider = request.provider || "unknown";
    const model = request.model || "";
    const tokens = request.total_tokens == null ? "not reported" : Number(request.total_tokens).toLocaleString();
    const cost = request.cost_usd == null ? "unpriced" : `$${Number(request.cost_usd).toFixed(4)}`;
    const detail = request.status === "ok"
      ? (request.fallback_attempts ? `Fallback attempts: ${request.fallback_attempts}` : "Completed")
      : (request.error || "Request failed");
    const cells = [
      new Date(request.created_at * 1000).toLocaleString(),
      request.status,
      provider,
      model,
      request.latency_ms == null ? "—" : `${request.latency_ms} ms`,
      tokens,
      cost,
      detail,
    ];
    cells.forEach((value, index) => {
      const cell = document.createElement("td");
      cell.textContent = value || "—";
      if (index === 1) cell.className = request.status === "ok" ? "status-ok" : "status-error";
      if (index === 7) cell.className = "detail-cell";
      row.appendChild(cell);
    });
    body.appendChild(row);
  });
}

function renderUsageHead() {
  const head = $("usageHead");
  if (!head) return;
  const query = state.usageQuery;
  head.querySelectorAll("th[data-sort]").forEach((th) => {
    const key = th.dataset.sort;
    const active = query.sort === key;
    th.classList.toggle("sorted", active);
    th.setAttribute("aria-sort", active ? (query.dir === "asc" ? "ascending" : "descending") : "none");
    th.textContent = th.textContent.replace(/ [▲▼]$/, "");
    if (active) th.textContent += query.dir === "asc" ? " ▲" : " ▼";
    th.title = "Sort by " + th.textContent.replace(/ [▲▼]$/, "");
  });
}

/* Numbered pager: 1 … around current … last, prev/next always present. */
function renderUsagePager(meta) {
  const host = $("usagePager");
  if (!host) return;
  host.replaceChildren();
  const pages = meta.pages || 1;
  const page = Math.min(meta.page || 1, pages);
  const goto = (target) => {
    state.usageQuery.page = Math.min(Math.max(target, 1), pages);
    loadUsage();
  };
  const prev = document.createElement("button");
  prev.className = "pager-btn";
  prev.textContent = "‹ Prev";
  prev.disabled = page <= 1;
  prev.addEventListener("click", () => goto(page - 1));
  host.appendChild(prev);
  const windowed = new Set([1, pages, page - 1, page, page + 1].filter((n) => n >= 1 && n <= pages));
  let last = 0;
  for (let number = 1; number <= pages; number++) {
    if (!windowed.has(number)) continue;
    if (last && number - last > 1) {
      const gap = document.createElement("span");
      gap.className = "pager-gap";
      gap.textContent = "…";
      host.appendChild(gap);
    }
    const button = document.createElement("button");
    button.className = "pager-btn" + (number === page ? " active" : "");
    button.textContent = number;
    button.addEventListener("click", () => goto(number));
    host.appendChild(button);
    last = number;
  }
  const next = document.createElement("button");
  next.className = "pager-btn";
  next.textContent = "Next ›";
  next.disabled = page >= pages;
  next.addEventListener("click", () => goto(page + 1));
  host.appendChild(next);
  const note = document.createElement("span");
  note.className = "pager-note muted";
  note.textContent = `${(meta.total || 0).toLocaleString()} requests total · page ${page} of ${pages}`;
  host.appendChild(note);
}

/* Div-bar charts — no canvas, no drawing library, holds on reduced widths. */
function renderUsageSeries(series) {
  const host = $("usageSeries");
  if (!host) return;
  host.replaceChildren();
  if (!series.length) {
    host.appendChild(pageNote("No dated activity for this filter."));
    return;
  }
  const peak = Math.max(...series.map((day) => day.requests || 1));
  series.forEach((day) => {
    const slot = document.createElement("div");
    slot.className = "usage-day";
    const failed = Math.max(0, (day.requests || 0) - (day.ok || 0));
    const okBar = document.createElement("div");
    okBar.className = "usage-bar ok";
    okBar.style.height = `${Math.round(((day.ok || 0) / peak) * 100)}%`;
    const failBar = document.createElement("div");
    failBar.className = "usage-bar failed";
    failBar.style.height = `${Math.round((failed / peak) * 100)}%`;
    const label = document.createElement("small");
    label.textContent = new Date(day.day * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" });
    slot.title = new Date(day.day * 1000).toLocaleDateString(undefined, { month: "long", day: "numeric" })
      + ` — ${day.ok || 0} ok, ${failed} failed, ${Number(day.tokens || 0).toLocaleString()} tokens, avg ${Math.round(day.avg_latency_ms || 0)} ms`;
    slot.appendChild(okBar);
    slot.appendChild(failBar);
    slot.appendChild(label);
    host.appendChild(slot);
  });
}

function renderUsageByModel(rows) {
  const host = $("usageByModel");
  if (!host) return;
  host.replaceChildren();
  if (!rows.length) {
    host.appendChild(pageNote("No requests recorded for this filter."));
    return;
  }
  const peak = Math.max(...rows.map((row) => row.requests || 1));
  rows.slice(0, 12).forEach((row) => {
    const line = document.createElement("div");
    line.className = "usage-model-line";
    const label = document.createElement("span");
    label.className = "usage-model-name";
    label.textContent = row.model;
    const bar = document.createElement("span");
    bar.className = "usage-model-bar";
    bar.style.width = `${Math.round(((row.requests || 0) / peak) * 100)}%`;
    const note = document.createElement("small");
    note.className = "muted";
    note.textContent = `${row.requests} req · ${Number(row.tokens || 0).toLocaleString()} tok · ${Math.round(row.avg_latency_ms || 0)} ms`; 
    line.title = `${row.provider} — ${row.ok || 0} ok of ${row.requests}`;
    line.appendChild(label);
    line.appendChild(bar);
    line.appendChild(note);
    host.appendChild(line);
  });
}

/* Breaker state plus what the log actually saw, on the same row: "open" with no
   history and "healthy but failing one time in three" are different problems. */
async function loadProviderHealth() {
  const host = $("usageHealth");
  if (!host) return;
  const result = await api("/api/health/providers");
  host.replaceChildren();
  if (!result.ok) {
    host.appendChild(pageNote("Provider health is unavailable."));
    return;
  }
  const rows = Array.isArray(result.data.models) ? result.data.models : [];
  host.title = result.data.rule || "";
  if (!rows.length) {
    host.appendChild(pageNote("No requests in the last 24 hours."));
    return;
  }
  rows.slice(0, 12).forEach((row) => {
    const circuit = row.breaker || {};
    const state = circuit.state || "no failures";
    const failed = Math.max(0, (row.requests || 0) - (row.ok || 0));
    const rate = row.requests ? Math.round((failed / row.requests) * 100) : 0;
    const line = document.createElement("div");
    line.className = "usage-model-line";
    const label = document.createElement("span");
    label.className = "usage-model-name";
    label.textContent = row.model;
    const bar = document.createElement("span");
    bar.className = "usage-model-bar" + (state === "open" ? " over" : "");
    bar.style.width = `${row.requests ? Math.max(4, rate) : 0}%`;
    const note = document.createElement("small");
    note.className = "muted";
    const facts = [state];
    if (circuit.retry_in_seconds > 0) facts.push(`retry in ${Math.ceil(circuit.retry_in_seconds)}s`);
    facts.push(`${row.ok || 0}/${row.requests || 0} ok`);
    if (row.requests) facts.push(`${Math.round(row.avg_latency_ms || 0)} ms`);
    note.textContent = facts.join(" · ");
    line.title = circuit.breaker_error || row.last_error || "";
    line.appendChild(label);
    line.appendChild(bar);
    line.appendChild(note);
    host.appendChild(line);
  });
}

function usageFiltersFromInputs() {
  const query = state.usageQuery;
  query.model = $("usageModelFilter").value.trim();
  query.provider = $("usageProviderFilter").value.trim();
  query.from = $("usageFrom").value || "";
  query.to = $("usageTo").value || "";
}

/* ------------------------------------------------------------------ */
/* context meter                                                      */
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
  const used = last || estimate;
  const percent = budget > 0 ? Math.round((used / budget) * 100) : 0;
  meter.textContent = `~${formatCount(used)} / ${formatCount(budget)} · ${percent}%`;
  meter.title = (last
    ? "Tokens the router reported for the last request"
    : "Estimated tokens for the next request")
    + ` — ${percent}% of the ${formatCount(budget)}-token context budget`;
  meter.classList.toggle("over", used > budget);
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

/* Phase 2 router manager: every registered router with the state the server
   measured. /api/routers reuses the same probes as /api/router,
   /api/omniroute and /api/local/status, so this list cannot disagree with the
   console status card above it. */
const ROUTER_ACTIONS = {
  freellmapi: { start: "/api/router/start", stop: "/api/router/stop" },
  omniroute: { start: "/api/omniroute/start", stop: "/api/omniroute/stop" },
};

function routerStateLabel(entry) {
  if (entry.healthy) return "Healthy";
  if (entry.running) return "Up, not answering";
  if (entry.installed) return "Stopped";
  return "Not installed";
}

function routerRow(entry) {
  const row = document.createElement("div");
  row.className = "console-row" + (entry.installed ? "" : " off");

  const main = document.createElement("div");
  main.className = "console-row-main";

  const name = document.createElement("strong");
  name.textContent = entry.name;
  main.appendChild(name);

  const state = document.createElement("span");
  state.className = entry.healthy ? "console-tag" : "muted";
  state.textContent = routerStateLabel(entry);
  main.appendChild(state);

  if (entry.endpoint) {
    const endpoint = document.createElement("code");
    endpoint.textContent = entry.endpoint;
    main.appendChild(endpoint);
  }
  if (entry.version) {
    const version = document.createElement("code");
    version.textContent = `v${entry.version}`;
    main.appendChild(version);
  }

  const detail = document.createElement("small");
  detail.textContent = entry.owned_by_app
    ? "Started by Studio, so Studio may stop it."
    : entry.message || "";
  main.appendChild(detail);

  // Result of the connection tester, replaced on every run: the server's real
  // request, reported with the status and latency it measured.
  const testLine = document.createElement("small");
  testLine.className = "muted";
  main.appendChild(testLine);
  row.appendChild(main);

  const actions = document.createElement("div");
  actions.className = "console-row-actions";
  actions.appendChild(rowButton("Test", async () => {
    testLine.className = "muted";
    testLine.textContent = "Testing…";
    const result = await api(`/api/routers/${entry.id}/test`, {
      method: "POST",
      body: "{}",
    });
    if (!result.ok) {
      testLine.textContent = result.data.error || "Could not run the test.";
      return;
    }
    testLine.className = result.data.ok ? "console-tag" : "muted";
    testLine.textContent = result.data.message || "";
  }));

  // Recent container output. The server refuses for a runtime it does not manage
  // (the local runtime belongs to the user's service manager), so the box shows
  // that reason instead of an empty pane.
  const logBox = document.createElement("pre");
  logBox.className = "console-logs hidden";
  actions.appendChild(rowButton("Logs", async () => {
    if (!logBox.classList.contains("hidden")) {
      logBox.classList.add("hidden");
      return;
    }
    logBox.classList.remove("hidden");
    logBox.textContent = "Reading the router's output…";
    const result = await api(`/api/routers/${entry.id}/logs?tail=200`);
    if (!result.ok) {
      logBox.textContent = result.data.error || "Could not read the router's output.";
      return;
    }
    const lines = result.data.lines || [];
    logBox.textContent = lines.length
      ? lines.join("\n")
      : "The router has not written any output yet.";
  }));

  // Configuration editor. The server decides whether this router has a file to
  // edit or why it does not (a container-only gateway, or the local runtime
  // Studio configures elsewhere), so the button never offers an editor that
  // cannot work.
  const configBox = document.createElement("div");
  configBox.className = "console-logs hidden";
  actions.appendChild(rowButton("Configuration", async () => {
    if (!configBox.classList.contains("hidden")) {
      configBox.classList.add("hidden");
      return;
    }
    await mountRouterConfig(entry, configBox);
  }));
  row.appendChild(configBox);

  // Install: the server builds a plan with no side effects, the panel prints its
  // exact commands and prerequisites, and only that plan's digest can be
  // approved. A blocked plan explains itself and offers nothing to click.
  if (entry.installable_here && !entry.installed) {
    const installBtn = rowButton("Install", async () => {
      const previous = actions.querySelector(".approve-install");
      if (previous) previous.remove();
      logBox.classList.remove("hidden");
      logBox.textContent = "Building the install plan…";
      const result = await api(`/api/routers/${entry.id}/install-plan`);
      if (!result.ok) {
        logBox.textContent = result.data.error || "Could not build an install plan.";
        return;
      }
      const plan = result.data;
      const lines = [
        `${entry.name} · ${plan.method} · ${plan.supported ? "installable here" : "no in-app install"}`,
        `source: ${plan.source}`,
        `verification: ${plan.verification}`,
      ];
      if (plan.writes && plan.writes.length) lines.push(`writes: ${plan.writes.join(", ")}`);
      (plan.prerequisites || []).forEach((item) => {
        lines.push(`${item.satisfied ? "ok      " : "missing "} ${item.what} — ${item.detail}`);
      });
      // A download is not a command, so it is printed as what it is: the URL,
      // the published hash it must match, and how the archive is unpacked.
      (plan.downloads || []).forEach((item, index) => {
        lines.push(`fetch ${index + 1}: ${item.url}`);
        lines.push(`        sha256 ${item.sha256} · ${item.archive}`);
      });
      (plan.steps || []).forEach((step, index) => {
        lines.push(`step ${index + 1}: ${step.program} ${step.args.join(" ")}`);
      });
      if (plan.blocked) lines.push(`blocked: ${plan.blocked}`);
      logBox.textContent = lines.join("\n");
      if (plan.blocked || !plan.supported) return;
      const confirm = rowButton("Approve and install", async () => {
        confirm.disabled = true;
        const done = await api(`/api/routers/${entry.id}/install`, {
          method: "POST",
          body: JSON.stringify({ approve: true, digest: plan.digest }),
        });
        toast(done.ok
          ? done.data.record.message
          : done.data.error || "That install failed.");
        await loadRouterList();
      });
      confirm.classList.add("approve-install");
      actions.insertBefore(confirm, installBtn);
    });
    actions.appendChild(installBtn);
  }

  // Update and Uninstall act on an install that already exists. Both ask the
  // server first: the update plan's digest is what the approval names, and the
  // uninstall is approved explicitly rather than implied by a click.
  if (entry.installable_here && entry.installed) {
    actions.appendChild(rowButton("Update", async () => {
      const status = await api(`/api/routers/${entry.id}/update`);
      if (!status.ok) {
        toast(status.data.error || "Could not check for an update.");
        return;
      }
      if (!status.data.update_available) {
        toast(status.data.message || "Already current.");
        return;
      }
      const plan = status.data.plan || {};
      const proceed = confirm(
        `Update ${entry.name} to ${status.data.latest}?\n\n` +
          `${plan.source || ""}\n\nThe running container is replaced; its data volume is kept.`,
      );
      if (!proceed) return;
      const done = await api(`/api/routers/${entry.id}/update`, {
        method: "POST",
        body: JSON.stringify({ approve: true, digest: plan.digest }),
      });
      toast(done.ok
        ? done.data.record.message
        : done.data.error || "That update failed.");
      await loadRouterList();
    }));
    actions.appendChild(rowButton("Uninstall", async () => {
      if (!confirm(`Remove the ${entry.name} install? Its saved data is kept.`)) return;
      const done = await api(`/api/routers/${entry.id}/uninstall`, {
        method: "POST",
        body: JSON.stringify({ approve: true, purge: false }),
      });
      toast(done.ok ? done.data.message : done.data.error || "That uninstall failed.");
      await loadRouterList();
    }));
  }

  // Only routers with a lifecycle in this build get lifecycle buttons, and only
  // where the registry says a local install is possible; an external runtime is
  // never started or stopped from here.
  const endpoints = ROUTER_ACTIONS[entry.id];
  if (endpoints && entry.installable_here) {
    const stopping = entry.healthy || entry.owned_by_app;
    actions.appendChild(rowButton(stopping ? "Stop" : "Start", async () => {
      const result = await api(stopping ? endpoints.stop : endpoints.start, {
        method: "POST",
        body: "{}",
      });
      if (!result.ok) toast(result.data.error || "That router action failed.");
      await loadRouterList();
    }));
    // Launch preference, stored server-side and honoured by the startup sweep,
    // which still never adopts a router it did not start.
    const toggle = document.createElement("label");
    toggle.className = "console-toggle";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = Boolean(entry.auto_start);
    box.addEventListener("change", async () => {
      const result = await api(`/api/routers/${entry.id}/auto-start`, {
        method: "POST",
        body: JSON.stringify({ enabled: box.checked }),
      });
      if (!result.ok) {
        box.checked = !box.checked;
        toast(result.data.error || "Could not save that preference.");
      }
    });
    const toggleText = document.createElement("span");
    toggleText.textContent = "Start with Studio";
    toggle.append(box, toggleText);
    actions.appendChild(toggle);

    // Restarting a stopped router would only start it, so the button appears
    // when there is something running to restart.
    if (stopping) {
      actions.appendChild(rowButton("Restart", async () => {
        const result = await api(`/api/routers/${entry.id}/restart`, {
          method: "POST",
          body: "{}",
        });
        toast(result.ok
          ? result.data.message || "Restarted."
          : result.data.error || "That router action failed.");
        await loadRouterList();
      }));
    }
  }
  row.appendChild(actions);
  row.appendChild(logBox);
  return row;
}

async function loadRouterList() {
  const list = $("routerList");
  if (!list) return;
  const result = await api("/api/routers");
  if (!result.ok) {
    list.replaceChildren(pageNote(result.data.error || "Could not read the router list."));
    return;
  }
  const entries = (result.data && result.data.routers) || [];
  if (!entries.length) {
    list.replaceChildren(pageNote("No routers are registered in this build."));
    return;
  }
  list.replaceChildren(...entries.map(routerRow));
  const note = $("routerListNote");
  if (note) {
    const platform = result.data.platform || {};
    const env = result.data.environment || {};
    const parts = [`Probed on ${platform.os || "?"}/${platform.arch || "?"}`];
    if (env.os_version) parts.push(env.os_version);
    parts.push(env.docker ? `docker ${env.docker.version || "present"}` : "docker not found on PATH");
    if (env.compose) parts.push(`compose ${env.compose.version || "present"}`);
    if (env.package_managers && env.package_managers.length) {
      parts.push(`package managers: ${env.package_managers.map((item) => item.name).join(", ")}`);
    } else {
      parts.push("no package manager found on PATH");
    }
    note.textContent = parts.join(" · ");
  }
}

// The router configuration editor (Phase 2 close-out). Files come from the
// server's whitelist inside the compose project the install recorded; saves go
// through the same permission store an editor save uses, so a saved fs_write
// deny can freeze the router's compose file.
async function mountRouterConfig(entry, host) {
  host.classList.remove("hidden");
  host.replaceChildren();
  const status = document.createElement("small");
  status.className = "muted";
  const info = await api(`/api/routers/${entry.id}/config`);
  if (!info.ok || info.data.ok === false) {
    status.textContent = info.data.error || "Could not read the router's configuration.";
    host.appendChild(status);
    return;
  }
  const files = info.data.files || [];
  const heading = document.createElement("small");
  heading.className = "muted";
  heading.textContent = files.length
    ? info.data.directory
    : `No configuration file exists yet in ${info.data.directory}.`;
  host.appendChild(heading);
  if (!files.length) return;

  const controls = document.createElement("div");
  controls.className = "form-actions";
  const select = document.createElement("select");
  select.className = "run-crew-input";
  files.forEach((file) => {
    const option = document.createElement("option");
    option.value = file.name;
    option.textContent = `${file.name} (${formatBytes(file.bytes)})`;
    select.appendChild(option);
  });
  const save = document.createElement("button");
  save.type = "button";
  save.className = "btn btn-accent btn-small";
  save.textContent = "Save file";
  controls.append(select, save);

  const area = document.createElement("textarea");
  area.className = "code-area router-config-area";
  area.spellcheck = false;
  area.wrap = "off";
  area.rows = 14;

  const loadFile = async () => {
    area.value = "Loading…";
    const result = await api(`/api/routers/${entry.id}/config/file?name=${encodeURIComponent(select.value)}`);
    if (!result.ok) {
      area.value = "";
      status.textContent = result.data.error || "Could not read that file.";
      return;
    }
    area.value = result.data.content;
    status.textContent = `${result.data.name} · ${formatBytes(result.data.bytes)} loaded`;
  };
  select.addEventListener("change", loadFile);
  save.addEventListener("click", async () => {
    save.disabled = true;
    const result = await api(`/api/routers/${entry.id}/config/file`, {
      method: "POST",
      confirmApproval: true,
      body: JSON.stringify({ name: select.value, content: area.value }),
    });
    save.disabled = false;
    if (!result.ok) {
      status.textContent = result.data.error || "Could not save the file.";
      return;
    }
    status.textContent = `Saved ${result.data.name} · ${formatBytes(result.data.bytes)}`;
    toast("Router configuration saved.");
  });

  host.append(controls, area, status);
  await loadFile();
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
      const result = await api("/api/config", {
        method: "POST",
        confirmApproval: true,
        body: JSON.stringify({ model: name }),
      });
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
  if ($("modeMemory")) $("modeMemory").addEventListener("click", () => { setSidebarMode("memory"); loadMemory(); });
  if ($("modeIndex")) $("modeIndex").addEventListener("click", () => { setSidebarMode("index"); loadIndex(); });
  $("newChat").addEventListener("click", newConversation);
  $("newChatTop").addEventListener("click", newConversation);
  // Thread pane scope: flip between this-project threads and all-project recents.
  $("threadScopeBtn").addEventListener("click", async () => {
    state.threadScope = state.threadScope === "all" ? "project" : "all";
    try { localStorage.setItem("freeai-thread-scope", state.threadScope); } catch { /* private mode */ }
    await loadConversations();
  });
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
  $("usageApply").addEventListener("click", () => { usageFiltersFromInputs(); state.usageQuery.page = 1; loadUsage(); });
  for (const input of ["usageModelFilter", "usageProviderFilter", "usageFrom", "usageTo"]) {
    $(input).addEventListener("keydown", (event) => {
      if (event.key === "Enter") { usageFiltersFromInputs(); state.usageQuery.page = 1; loadUsage(); }
    });
  }
  $("usageReset").addEventListener("click", () => {
    $("usageModelFilter").value = "";
    $("usageProviderFilter").value = "";
    $("usageFrom").value = "";
    $("usageTo").value = "";
    state.usageQuery = { page: 1, sort: "created_at", dir: "desc", model: "", provider: "", from: "", to: "" };
    loadUsage();
  });
  document.querySelectorAll("#usageHead th[data-sort]").forEach((th) => {
    th.addEventListener("click", () => {
      const key = th.dataset.sort;
      const query = state.usageQuery;
      if (query.sort === key) query.dir = query.dir === "asc" ? "desc" : "asc";
      else { query.sort = key; query.dir = key === "created_at" ? "desc" : "asc"; }
      query.page = 1;
      loadUsage();
    });
  });

  $("sidebarToggle").addEventListener("click", () => {
    const open = document.body.classList.toggle("nav-open");
    $("scrim").classList.toggle("hidden", !open);
  });

  /* ---- panels: left / right / bottom ------------------------------ */
  /* Sizes and visibility live in localStorage with the same pattern as the
     theme, and drive :root custom properties — no re-render involved. */
  state.panelSize = JSON.parse(localStorage.getItem("freeai-panel-size") || "{}") || {};
  state.panelShow = JSON.parse(localStorage.getItem("freeai-panel-show") || "{}") || {};
  if (state.panelShow.left == null) { state.panelShow.left = true; }
  if (state.panelShow.right == null) { state.panelShow.right = false; }
  if (state.panelShow.bottom == null) { state.panelShow.bottom = false; }
  PANEL_MIN = { left: 212, right: 240, bottom: 120 };
  PANEL_MAX = { left: 560, right: 640, bottom: 520 };
  writePanelVars();
  applyPanels();
  $("leftPanelToggle").addEventListener("click", () => togglePanel("left"));
  $("rightPanelToggle").addEventListener("click", () => togglePanel("right"));
  $("bottomPanelToggle").addEventListener("click", () => togglePanel("bottom"));
  document.querySelectorAll(".panel-handle").forEach((handle) => {
    handle.addEventListener("pointerdown", startPanelDrag);
  });
  scheduleRightPanelTimer();
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
  $("prompt").addEventListener("input", () => {
    autoGrow($("prompt"));
    updateContextMeter();
    suggestSkill($("prompt").value);
  });
  $("mediaAttachBtn").addEventListener("click", () => $("mediaFileInput").click());
  $("mediaFileInput").addEventListener("change", async (event) => {
    const files = event.target.files;
    if (files && files.length) await uploadMediaFiles(files);
    event.target.value = "";
  });
  $("attachContextBtn").addEventListener("click", attachFromComposer);
  $("deepToggle").addEventListener("click", () => setDeepSearch(!state.deepSearch));
  $("skillPickerBtn").addEventListener("click", (event) => {
    event.stopPropagation();
    toggleSkillMenu();
  });
  $("skillMenu").addEventListener("click", (event) => event.stopPropagation());
  // Model picker: same pattern as the skill menu.
  $("modelChip").addEventListener("click", (event) => {
    event.stopPropagation();
    toggleModelMenu();
  });
  $("modelMenu").addEventListener("click", (event) => event.stopPropagation());
  // Thinking control: same menu pattern as the model picker.
  $("thinkBtn").addEventListener("click", (event) => {
    event.stopPropagation();
    toggleThinkingMenu();
  });
  $("thinkingMenu").addEventListener("click", (event) => event.stopPropagation());
  document.addEventListener("click", () => toggleThinkingMenu(false));
  $("prompt").addEventListener("input", syncThinkingDirective);
  $("testOpenAI").addEventListener("click", () => { void testOpenAIPlatform(); });
  renderThinkingChip();
  $("modelFilter").addEventListener("input", () => renderModelMenu());
  document.addEventListener("click", () => toggleModelMenu(false));
  // Mode picker (Build/Plan); persists the choice like the model chip.
  const restore = (key, fallback) => {
    try { return localStorage.getItem(key); } catch { return fallback; }
  };
  const savedModel = restore("freeai-chat-model", "") || "";
  const savedMode = restore("freeai-chat-mode", "");
  if (savedModel) state.chatModel = savedModel;
  setChatMode(savedMode === "plan" ? "plan" : "build");
  renderModelChipLabel();
  $("skillChipClear").addEventListener("click", () => {
    state.activeSkillId = null;
    state.suggestedSkillId = null;
    renderSkillChip();
    if ($("skillMenu") && !$("skillMenu").classList.contains("hidden")) renderSkillMenu();
  });
  document.addEventListener("click", closeSkillMenu);
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

  $("workflowNew").addEventListener("click", () => {
    $("workflowForm").dataset.editing = "";
    $("workflowName").value = "";
    $("workflowPrompt").value = "";
    fillSkillOptions("");
    $("workflowForm").classList.remove("hidden");
    $("workflowName").focus();
  });
  $("workflowCancel").addEventListener("click", () => $("workflowForm").classList.add("hidden"));
  $("workflowForm").addEventListener("submit", async (event) => {
    event.preventDefault();
    const payload = {
      id: $("workflowForm").dataset.editing || "",
      name: $("workflowName").value.trim(),
      prompt: $("workflowPrompt").value.trim(),
      skill: $("workflowSkill") ? $("workflowSkill").value : "",
    };
    const result = await api("/api/workflows", { method: "POST", body: JSON.stringify(payload) });
    if (!result.ok) {
      toast(result.data.error || "Could not save that workflow.");
      return;
    }
    $("workflowForm").classList.add("hidden");
    toast("Workflow saved.");
    loadWorkflows();
  });

  $("baseUrl").addEventListener("input", markActivePreset);
  $("settingsForm").addEventListener("submit", saveSettings);
  $("forgetApiKey").addEventListener("click", () => forgetSavedKey("api_key", $("forgetApiKey"), $("keyStatus")));
  $("forgetOpenaiKey").addEventListener("click", () => forgetSavedKey("openai_api_key", $("forgetOpenaiKey"), $("openaiKeyStatus")));
  $("forgetSearchKey").addEventListener("click", () => forgetSavedKey("search_api_key", $("forgetSearchKey"), $("searchKeyStatus")));
  $("testConnection").addEventListener("click", testConnection);
  $("openProviderDashboardSettings").addEventListener("click", openProviderDashboard);
  $("adaptiveRouting").addEventListener("change", () => {
    $("adaptiveTiersRow").classList.toggle("hidden", !$("adaptiveRouting").checked);
  });

  $("routerBtn").addEventListener("click", () => setView("router"));
  [["workspacesBtn", "workspaces"], ["agentsBtn", "agents"], ["integrationsBtn", "integrations"], ["artifactsBtn", "artifacts"], ["automationBtn", "automation"], ["localBtn", "local"], ["runsBtn", "runs"]].forEach(([id, view]) => {

    $(id).addEventListener("click", () => setView(view));
  });
  $("consoleRefresh").addEventListener("click", loadConsoleStatus);
  $("routerListRefresh").addEventListener("click", loadRouterList);
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

/* ------------------------------------------------------------------ */
/* local AI: real runtime state from /api/local — nothing fabricated   */
/* ------------------------------------------------------------------ */

state.localStatus = null;
state.localModels = null;
state.localHardware = null;
state.localPulling = null;

async function loadLocal() {
  const [status, models] = await Promise.all([api("/api/local/status"), api("/api/local/models")]);
  state.localStatus = status.ok ? status.data : null;
  if (status.ok) {
    const runtime = status.data.runtime || {};
    state.localPulling = status.data.pulling || null;
    renderLocalStatus();
    if (!runtime.installed) {
      // No runtime: the model list would only echo the failure.
      state.localModels = { ok: false, models: [], error: status.data.runtime && status.data.runtime.error || "" };
    }
  }
  if (models.ok !== undefined) state.localModels = models.data;
  renderLocalModels();
  void refreshLocalHardware();
  const cfg = await api("/api/config");
  if (cfg.ok) {
    $("localBaseUrl").value = cfg.data.local_base_url || "http://127.0.0.1:11434";
    $("localSelectedInput").value = cfg.data.local_model || "";
    setLocalChatToggle(Boolean(cfg.data.local_chat), cfg.data.local_model);
  }
}

function setLocalChatToggle(on, selectedModel) {
  const button = $("localChatToggle");
  if (!button) return;
  button.setAttribute("aria-pressed", String(on));
  button.textContent = `Use for chat & runs: ${on ? "on" : "off"}`;
  button.classList.toggle("btn-accent", on);
  button.classList.toggle("btn-quiet", !on);
  button.disabled = on && !(selectedModel || "").trim();
  button.title = on && !(selectedModel || "").trim()
    ? "Select a local model first"
    : "When on, chat and agent runs go only to this computer's local runtime — never to a remote provider.";
}

function renderLocalStatus() {
  const status = state.localStatus;
  const dot = $("localStatusDot");
  const title = $("localStatusTitle");
  const detail = $("localStatusDetail");
  const hint = $("localStatusHint");
  const setupCard = $("localSetupCard");
  if (!status) {
    dot.className = "status-dot status-off";
    title.textContent = "Local runtime status unknown";
    detail.textContent = "Could not reach the server endpoint. Refresh to retry.";
    hint.classList.add("hidden");
    setupCard.classList.remove("hidden");
    return;
  }
  const runtime = status.runtime || {};
  const pulling = status.pulling;
  if (pulling) {
    dot.className = "status-dot status-busy";
    title.textContent = "Ollama — pulling " + pulling;
    detail.textContent = "Downloading in the background; this panel refreshes as progress lands.";
  } else if (runtime.installed) {
    const selectedOk = status.selected_model_is_installed;
    dot.className = "status-dot status-ok";
    title.textContent = `Ollama ${runtime.version || ""} — detected and reachable`.trim();
    detail.textContent = status.model_count === 1
      ? "1 installed model on this computer."
      : `${status.model_count} installed models on this computer.`;
    if (status.selected_model && selectedOk === false) {
      dot.className = "status-dot status-warn";
      detail.textContent = `The saved model "${status.selected_model}" is not installed on this endpoint. Pull it below or pick another.`;
    }
    setupCard.classList.add("hidden");
  } else {
    dot.className = "status-dot status-off";
    title.textContent = "Local runtime not detected";
    detail.textContent = runtime.error || "Nothing answered on the local endpoint.";
    hint.textContent = runtime.setup_hint || "";
    hint.classList.toggle("hidden", !runtime.setup_hint);
    setupCard.classList.remove("hidden");
  }
}

function fmtBytes(value) {
  if (value == null || Number.isNaN(Number(value))) return null;
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = Number(value);
  let unit = 0;
  while (size >= 1000 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return `${size >= 10 || unit === 0 ? Math.round(size) : size.toFixed(1)} ${units[unit]}`;
}

function renderLocalModels() {
  const list = $("localModelsList");
  const dataset = $("localModelTags");
  list.replaceChildren();
  dataset.replaceChildren();
  const payload = state.localModels;
  if (!payload || payload.ok === false) {
    const empty = document.createElement("p");
    empty.className = "run-empty";
    empty.textContent = payload && payload.error
      ? `Model list unavailable: ${payload.error}`
      : "Start the runtime to see the models installed on this computer.";
    list.appendChild(empty);
    return;
  }
  const models = payload.models || [];
  models.forEach((model) => {
    const option = document.createElement("option");
    option.value = model.name;
    dataset.appendChild(option);
  });
  if (!models.length) {
    const empty = document.createElement("p");
    empty.className = "run-empty";
    empty.textContent = "No models are installed yet. Pull one below — quantized small coding models are the easiest start.";
    list.appendChild(empty);
    return;
  }
  state.localModels = null;
  models.forEach((model) => list.appendChild(localModelCard(model)));
}

function localModelCard(model) {
  const selected = state.localStatus && state.localStatus.selected_model === model.name;
  const card = document.createElement("article");
  card.className = "card local-model-card" + (selected ? " selected" : "");
  const head = document.createElement("div");
  head.className = "local-model-head";
  const name = document.createElement("strong");
  name.textContent = model.name;
  head.appendChild(name);
  if (selected) {
    const pill = document.createElement("span");
    pill.className = "status-pill run-status run-ok";
    pill.textContent = "selected";
    head.appendChild(pill);
  }
  card.appendChild(head);
  const meta = document.createElement("p");
  meta.className = "muted local-model-meta";
  const size = fmtBytes(model.size_bytes) || "size unknown";
  meta.textContent = `${model.family || "unknown"} · ${model.parameter_size || "unknown"} · ${model.quantization || "unknown"} · ${size}`;
  card.appendChild(meta);
  const actions = document.createElement("div");
  actions.className = "local-model-actions";
  const useBtn = document.createElement("button");
  useBtn.type = "button";
  useBtn.className = "btn " + (selected ? "btn-quiet" : "btn-accent") + " btn-small";
  useBtn.textContent = selected ? "Selected" : "Use this model";
  useBtn.disabled = Boolean(selected);
  useBtn.addEventListener("click", async () => {
    const result = await api("/api/local/select", {
      method: "POST",
      body: JSON.stringify({ model: model.name }),
    });
    if (!result.ok) { toast(result.data.error || "Could not select the model."); return; }
    toast(`Selected local model: ${model.name}.`);
    void loadLocal();
  });
  actions.appendChild(useBtn);
  card.appendChild(actions);
  return card;
}

async function refreshLocalHardware() {
  const result = await api("/api/local/hardware");
  if (!result.ok) return;
  state.localHardware = result.data;
  renderLocalHardware();
}

function renderLocalHardware() {
  const hw = state.localHardware;
  const box = $("localHardware");
  if (!box || !hw) return;
  const row = (label, value) => `<div><dt>${label}</dt><dd>${value}</dd></div>`;
  const cores = hw.logical_cores == null ? "Unknown" : `${hw.logical_cores} logical cores`;
  const ram = hw.ram_total_bytes == null ? "Unknown" : fmtBytes(hw.ram_total_bytes);
  const gpu = hw.gpu_list == null ? "Unknown" : hw.gpu_list.join(", ");
  const disk = hw.disk_free_bytes == null ? "Unknown" : `${fmtBytes(hw.disk_free_bytes) || "?"} of ${fmtBytes(hw.disk_total_bytes) || "?"} free`;
  box.innerHTML = [
    row("CPU", `${escLocal(hw.cpu_model || "Unknown")} — ${cores}`),
    row("System RAM", ram),
    row("GPU", gpu),
    row("Disk (workspace drive)", hw.disk_path ? `${disk} at ${escLocal(hw.disk_path)}` : disk),
  ].join("");
  $("localRecommendation").textContent = hw.recommendation || "";
}

function escLocal(text) {
  return String(text).replace(/[&<>"]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[ch]));
}

function setupLocalControls() {
  $("localRefreshBtn").addEventListener("click", () => void loadLocal());
  $("localRefreshModelsBtn").addEventListener("click", () => void loadLocal());
  $("localHardwareBtn").addEventListener("click", () => void refreshLocalHardware());
  $("localEndpointForm").addEventListener("submit", async (event) => {
    event.preventDefault();
    const payload = {
      base_url: $("localBaseUrl").value.trim(),
      model: $("localSelectedInput").value.trim(),
    };
    const result = await api("/api/local/select", { method: "POST", body: JSON.stringify(payload) });
    if (!result.ok) { toast(result.data.error || "Could not save the local selection."); return; }
    toast("Local runtime settings saved.");
    void loadLocal();
  });
  $("localChatToggle").addEventListener("click", async () => {
    const currentlyOn = $("localChatToggle").getAttribute("aria-pressed") === "true";
    const result = await api("/api/local/select", {
      method: "POST",
      body: JSON.stringify({ chat: !currentlyOn }),
    });
    if (!result.ok) { toast(result.data.error || "Could not change the local mode."); return; }
    toast(!currentlyOn
      ? "Local AI mode is on — chat and agent runs now use only the local runtime."
      : "Local AI mode is off — the router answers again.");
    void loadLocal();
  });
  $("localPullForm").addEventListener("submit", async (event) => {
    event.preventDefault();
    const tag = $("localPullModel").value.trim();
    if (!tag) return;
    const result = await api("/api/local/pull", { method: "POST", body: JSON.stringify({ model: tag }) });
    if (!result.ok) { toast(result.data.error || "Could not start the pull."); return; }
    $("localPullStatus").textContent = `Pulling ${tag}…`;
    state.localPulling = tag;
    void loadLocal();
  });
}

function handleLocalEvent(data) {
  if (data.type === "local-pull") {
    const status = $("localPullStatus");
    if (status) {
      if (data.phase === "progress") status.textContent = `Pulling ${data.model}… ${data.detail}`;
      else if (data.phase === "success") { status.textContent = `"${data.model}" downloaded successfully.`; state.localPulling = null; if (state.view === "local") void loadLocal(); }
      else if (data.phase === "failed") { status.textContent = `Pull of ${data.model} failed: ${data.detail}`; state.localPulling = null; if (state.view === "local") void loadLocal(); }
      else if (data.phase === "finish") { if (state.localPulling === data.model) { state.localPulling = null; if (state.view === "local") void loadLocal(); } }
    }
    return;
  }
  if (data.type === "local-models-changed" && state.view === "local") void loadLocal();
}

/* ------------------------------------------------------------------ */
/* agent runs: autonomous goal execution with approvals                */
/* ------------------------------------------------------------------ */

state.agentRuns = [];
state.openRun = null;
state.runDetail = null;

const RUN_STATUS_CLASS = {
  queued: "run-queue",
  running: "run-running",
  awaiting_approval: "run-await",
  paused: "run-pause",
  succeeded: "run-ok",
  failed: "run-fail",
  cancelled: "run-done",
  interrupted: "run-done",
};

function runAgo(seconds) {
  const delta = Math.max(0, Date.now() / 1000 - seconds);
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)} min ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)} h ago`;
  return `${Math.floor(delta / 86400)} d ago`;
}

async function loadRuns() {
  // The board and the cards come from the same query on the server, so a run can
  // never be counted in one and missing from the other.
  const [result, overview] = await Promise.all([api("/api/runs"), api("/api/runs/overview")]);
  if (!result.ok) {
    toast(result.data.error || "Could not load agent runs.");
    return;
  }
  state.agentRuns = result.data.runs || [];
  state.runOverview = overview.ok ? overview.data : null;
  renderRuns();
}

/* An agent event means the panel's numbers changed. The board, the cards and the
   open run's step log all come from the server, so they are re-read instead of
   patched locally: a run that parked, paused or failed must never keep reading as
   still working. */
async function refreshRunsPanel(runId) {
  if (!$("runsList")) return;
  await loadRuns();
  if (state.openRun && (!runId || runId === state.openRun)) await loadRunDetail();
}

function formatDuration(seconds) {
  const total = Math.max(0, Math.round(seconds || 0));
  if (total < 60) return `${total}s`;
  if (total < 3600) return `${Math.floor(total / 60)}m ${total % 60}s`;
  return `${Math.floor(total / 3600)}h ${Math.floor((total % 3600) / 60)}m`;
}

/* Cross-run dashboard: what is live now, what is stuck on a human, and what the
   recent runs have cost in steps and tokens. */
function renderRunBoard() {
  const board = document.createElement("div");
  board.className = "stats-grid run-board";
  const overview = state.runOverview;
  if (!overview) return board;
  const counts = overview.counts || {};
  const totals = overview.totals || {};
  const facts = [
    ["Live runs", totals.live || 0, "working, waiting for approval, or paused"],
    ["Queued", counts.queued || 0, "waiting for a free slot or for a dependency to succeed"],
    ["Awaiting approval", counts.awaiting_approval || 0, "each blocks until answered"],
    ["Paused", counts.paused || 0, "parked between steps; resume when ready"],
    ["Failed", counts.failed || 0, "recent runs that ended badly"],
    ["Tokens", Number(totals.tokens || 0).toLocaleString(), "reported by the run models"],
    ["Steps", Number(totals.steps || 0).toLocaleString(), "across the runs listed"],
  ];
  facts.forEach(([label, value, hint]) => {
    const card = document.createElement("div");
    card.className = "stat-card";
    const name = document.createElement("span");
    name.textContent = label;
    const strong = document.createElement("strong");
    strong.textContent = value;
    card.append(name, strong);
    card.title = hint;
    board.appendChild(card);
  });
  return board;
}

function renderRuns() {
  const box = $("runsList");
  if (!box) return;
  box.innerHTML = "";
  if (state.runOverview) box.appendChild(renderRunBoard());
  if (!state.agentRuns.length) {
    const empty = document.createElement("p");
    empty.className = "run-empty";
    empty.textContent = state.workspace
      ? "No runs yet. Give the agent a goal above — it plans, edits and verifies inside the open workspace."
      : "No runs yet. Open a workspace folder first (Spaces → open a folder) — agent runs work inside it.";
    box.appendChild(empty);
    return;
  }
  state.agentRuns.forEach((run) => box.appendChild(renderRunCard(run)));
}

function renderRunCard(run) {
  const card = document.createElement("article");
  card.className = "card run-card";
  const head = document.createElement("button");
  head.type = "button";
  head.className = "run-head";
  const title = document.createElement("div");
  title.className = "run-title";
  const pill = document.createElement("span");
  pill.className = `status-pill run-status ${(RUN_STATUS_CLASS[run.status] || "")}`;
  pill.textContent = run.status.replace("_", " ");
  title.appendChild(pill);
  const goal = document.createElement("span");
  goal.className = "run-goal";
  goal.textContent = run.goal;
  title.appendChild(goal);
  head.appendChild(title);
  const meta = document.createElement("span");
  meta.className = "run-meta";
  // Adaptive runs record which model answered; base-model runs stay unlabelled.
  const modelTag = run.model ? ` · ${run.model}` : "";
  const modeTag = run.mode === "crew" && Array.isArray(run.crew) && run.crew.length
    ? ` · crew: ${run.crew.join(" → ")}`
    : run.mode ? ` · ${run.mode}` : "";
  const profileTag = run.profile && run.profile !== "coding" ? ` · ${run.profile}` : "";
  // The discovered tool count comes from the run's own catalogue snapshot, so
  // a run that found nothing says so instead of implying it had tools.
  const mcpTag = run.allowMcp ? ` · mcp: ${run.mcpTools || 0} tool${run.mcpTools === 1 ? "" : "s"}` : "";
  const tokenTag = run.tokens ? ` · ${Number(run.tokens).toLocaleString()} tokens` : "";
  const durationTag = run.elapsedSeconds ? ` · ${formatDuration(run.elapsedSeconds)}` : "";
  // A queued run has done no work yet, so it says where it is in line instead of
  // implying progress; a run that waited shows the work time, not the wait.
  const queueTag = run.status === "queued" && run.queuePosition ? ` · queue #${run.queuePosition}` : "";
  const depTag = Array.isArray(run.dependsOn) && run.dependsOn.length ? ` · waits on ${run.dependsOn.length} run${run.dependsOn.length === 1 ? "" : "s"}` : "";
  // A retry links back to the run it was started from, so the card says where
  // the goal and settings came from instead of looking like a duplicate.
  const retryTag = run.retryOf ? ` · retry of ${String(run.retryOf).slice(0, 8)}` : "";
  // What the run was handed before its first step: labels only, because the
  // block itself belongs in the detail view, not on fifty cards.
  const ctx = run.context || {};
  const ctxMemory = Array.isArray(ctx.memory) ? ctx.memory : [];
  const ctxCode = Array.isArray(ctx.code) ? ctx.code : [];
  const ctxParts = [];
  if (ctxMemory.length) ctxParts.push(`${ctxMemory.length} memory`);
  if (ctxCode.length) ctxParts.push(`${ctxCode.length} code`);
  const contextTag = ctxParts.length ? ` · given ${ctxParts.join(" + ")}` : "";
  meta.textContent = `${run.steps} step${run.steps === 1 ? "" : "s"}${durationTag} · ${runAgo(run.updatedAt)}${queueTag}${depTag}${retryTag}${profileTag}${modeTag}${modelTag}${tokenTag}${mcpTag}${contextTag}`;
  if (ctxParts.length) {
    meta.title = `Given at start:\n${[...ctxMemory, ...ctxCode].join("\n")}`;
  }
  head.appendChild(meta);
  head.addEventListener('click', () => toggleRun(run.id));
  card.appendChild(head);

  if (run.id === state.openRun && state.runDetail) card.appendChild(buildRunDetail(state.runDetail));
  else if (run.id === state.openRun) void loadRunDetail();

  if (run.status === "awaiting_approval" && run.pendingAction) card.appendChild(buildApprovalRow(run));
  return card;
}

async function toggleRun(id) {
  state.openRun = state.openRun === id ? null : id;
  state.runDetail = null;
  if (state.openRun) await loadRunDetail();
  else renderRuns();
}

async function loadRunDetail() {
  const id = state.openRun;
  if (!id) return;
  const result = await api(`/api/runs/${encodeURIComponent(id)}`);
  if (!result.ok) {
    toast(result.data.error || "Could not load the run.");
    state.openRun = null;
    renderRuns();
    return;
  }
  state.runDetail = result.data;
  renderRuns();
}

function buildApprovalRow(run) {
  const row = document.createElement("div");
  row.className = "run-approval";
  const label = document.createElement("span");
  label.className = "run-approval-label";
  const pending = run.pendingAction || {};  label.textContent = `Waiting for approval — ${pending.summary || pending.tag || "next action"}`;
  row.appendChild(label);
  const accept = document.createElement("button");
  accept.type = "button";
  accept.className = "btn btn-accent btn-small";
  accept.textContent = "Approve & continue";
  accept.addEventListener('click', () => answerApproval(run.id, true));
  const reject = document.createElement("button");
  reject.type = "button";
  reject.className = "btn btn-quiet btn-small";
  reject.textContent = "Reject & stop";
  reject.addEventListener('click', () => answerApproval(run.id, false));
  row.appendChild(accept);
  row.appendChild(reject);
  return row;
}

function buildRunDetail(detail) {
  const box = document.createElement("div");
  box.className = "run-detail";

  const controls = document.createElement("div");
  controls.className = "run-controls";
  if (detail.status === "queued") {
    const waiting = document.createElement("span");
    waiting.className = "run-queue-note";
    waiting.textContent = detail.queuePosition
      ? `Waiting in the queue (position ${detail.queuePosition}) — it starts when a slot frees and every dependency has succeeded.`
      : "Waiting for a free run slot — it starts on its own, no need to do anything.";
    controls.appendChild(waiting);
  }
  if (detail.status === "running") {
    const pause = document.createElement("button");
    pause.type = "button";
    pause.className = "btn btn-quiet btn-small";
    pause.textContent = "Pause";
    pause.title = "Park the run at the next step boundary; the step already running finishes first";
    pause.addEventListener('click', () => pauseAgentRun(detail.id));
    controls.appendChild(pause);
  }
  if (detail.status === "paused") {
    const resume = document.createElement("button");
    resume.type = "button";
    resume.className = "btn btn-accent btn-small";
    resume.textContent = "Resume";
    resume.title = "Continue from the recorded step log";
    resume.addEventListener('click', () => resumeAgentRun(detail.id));
    controls.appendChild(resume);
  }
  if (detail.status === "running" || detail.status === "awaiting_approval" || detail.status === "paused" || detail.status === "queued") {
    const stop = document.createElement("button");
    stop.type = "button";
    stop.className = "btn btn-quiet btn-small";
    stop.textContent = "Cancel run";
    stop.addEventListener('click', () => cancelAgentRun(detail.id));
    controls.appendChild(stop);
  }
  if (["failed", "interrupted", "cancelled", "succeeded"].includes(detail.status)) {
    // Two different things, named as such: resume continues this step log,
    // retry starts a clean run of the same goal and settings.
    const again = document.createElement("button");
    again.type = "button";
    again.className = "btn btn-quiet btn-small";
    again.textContent = "Resume where it stopped";
    again.title = "Continue from this run's recorded step log";
    again.addEventListener('click', () => resumeAgentRun(detail.id));
    controls.appendChild(again);
    const retry = document.createElement("button");
    retry.type = "button";
    retry.className = "btn btn-accent btn-small";
    retry.textContent = "Retry from scratch";
    retry.title = detail.retryOf
      ? `Start another clean run — this one was itself a retry of ${detail.retryOf}`
      : "Start a new run with the same goal and settings; its step log starts empty";
    retry.addEventListener('click', () => retryAgentRun(detail.id));
    controls.appendChild(retry);
    if ((detail.stepList || []).some((step) => step.hasCheckpoint)) {
      const undo = document.createElement("button");
      undo.type = "button";
      undo.className = "btn btn-quiet btn-small run-restore";
      undo.textContent = "Restore all files";
      undo.title = "Put every file this run changed back to the content it had before the run first wrote it";
      undo.addEventListener('click', () => restoreRun(detail.id));
      controls.appendChild(undo);
    }
    // The whole-tree snapshot is the only thing that can undo a command step,
    // so it is offered even when the run logged no per-file checkpoints.
    const snapshot = document.createElement("button");
    snapshot.type = "button";
    snapshot.className = "btn btn-quiet btn-small run-restore";
    snapshot.textContent = "Restore snapshot";
    snapshot.title = "Put every tracked file back to the whole-tree snapshot taken before this run started; untracked files are left alone";
    snapshot.addEventListener('click', () => restoreSnapshot(detail.id));
    controls.appendChild(snapshot);
    if (detail.worktree) {
      const apply = document.createElement("button");
      apply.type = "button";
      apply.className = "btn btn-accent btn-small";
      apply.textContent = "Apply to workspace";
      apply.title = `Commit this run's checkout (${detail.worktree}), fast-forward the workspace onto its branch, and remove the checkout`;
      apply.addEventListener('click', () => applyRun(detail.id));
      controls.appendChild(apply);
    }
  }
  const refresh = document.createElement("button");
  refresh.type = "button";
  refresh.className = "btn btn-quiet btn-small";
  refresh.textContent = "Refresh";
  refresh.addEventListener('click', () => loadRunDetail());
  controls.appendChild(refresh);
  box.appendChild(controls);

  const given = [...(detail.context?.memory || []), ...(detail.context?.code || [])];
  if (given.length) {
    const line = document.createElement("p");
    line.className = "run-context";
    line.textContent = `Given at start: ${given.join(", ")}`;
    box.appendChild(line);
  }

  if (detail.error) {
    const err = document.createElement("p");
    err.className = "run-error";
    err.textContent = detail.error;
    box.appendChild(err);
  }

  const steps = detail.stepList || [];
  if (detail.status === "awaiting_approval" && detail.pendingDiff) {
    const preview = document.createElement("div");
    preview.className = "run-diff-wrap";
    const head = document.createElement("span");
    head.className = "run-diff-label";
    head.textContent = "Proposed change — preview before approving:";
    preview.appendChild(head);
    preview.appendChild(diffBlock(detail.pendingDiff));
    box.appendChild(preview);
  }

  if (!steps.length && !detail.error) {
    const none = document.createElement("p");
    none.className = "run-empty";
    none.textContent = "No steps logged yet.";
    box.appendChild(none);
    return box;
  }
  const list = document.createElement("div");
  list.className = "run-steps";
  steps.forEach((step) => {
    const row = document.createElement("div");    row.className = `run-step${step.action.endsWith("-error") || step.action === "unparsed" ? " run-step-bad" : ""}`;
    const seq = document.createElement("span");
    seq.className = "run-seq";
    seq.textContent = step.seq;
    row.appendChild(seq);
    const body = document.createElement("div");
    body.className = "run-step-body";
    const actionLine = document.createElement("div");
    actionLine.className = "run-step-action";
    actionLine.innerHTML = `<span class="run-step-tag">${escapeHtml(step.action)}</span> `;
    const actionText = document.createElement("span");
    actionText.textContent = step.summary || "";
    actionLine.appendChild(actionText);
    body.appendChild(actionLine);
    if (step.output) {
      const output = document.createElement("pre");
      output.className = "run-step-output";
      output.textContent = step.output;
      body.appendChild(output);
    }
    if (step.hasCheckpoint) {
      const restore = document.createElement("button");
      restore.type = "button";
      restore.className = "btn btn-quiet btn-small run-restore";
      restore.textContent = "Restore file to before this step";
      restore.addEventListener('click', () => restoreRunStep(detail.id, step.seq));
      body.appendChild(restore);
    }
    row.appendChild(body);
    list.appendChild(row);
  });
  box.appendChild(list);
  return box;
}

async function restoreRun(runId) {
  if (!confirm("Restore every file this run changed to its content before the run first wrote it?\n\nThis overwrites the files' current contents; edits made after the run are lost.")) return;
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/restore`, {
    method: "POST",
    body: JSON.stringify({}),
    confirmApproval: true,
  });
  if (!result.ok) {
    toast(result.data.error || "Could not restore the run's files.");
    return;
  }
  let data = result.data;
  const changed = (data.skipped || []).filter((entry) => entry.changed);
  if (changed.length) {
    const names = changed.map((entry) => entry.path).join("\n");
    if (!confirm(`${changed.length} file${changed.length === 1 ? " was" : "s were"} edited after the run:\n\n${names}\n\nRestore ${changed.length === 1 ? "it" : "them"} anyway? The later edits will be lost.`)) {
      toast(`Left ${changed.length} changed file${changed.length === 1 ? "" : "s"} alone.`);
      await loadRunDetail();
      return;
    }
    const forced = await api(`/api/runs/${encodeURIComponent(runId)}/restore`, {
      method: "POST",
      body: JSON.stringify({ force: true }),
      confirmApproval: true,
    });
    if (!forced.ok) {
      toast(forced.data.error || "Could not force the restore.");
      return;
    }
    data = forced.data;
  }
  const skipped = (data.skipped || []).length;
  toast(`Restored ${data.count} file${data.count === 1 ? "" : "s"}${skipped ? `; ${skipped} skipped` : ""}.`);
  await loadRunDetail();
}

async function restoreRunStep(runId, seq) {
  if (!confirm(`Restore the file to its contents before step ${seq}? Only that one file is reverted; any later edits to it are lost.`)) return;
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/steps/${seq}/restore`, { method: "POST" });
  if (!result.ok) {
    toast(result.data.error || "Could not restore the file.");
    return;
  }
  toast(result.data.path ? `Restored ${result.data.path} to its pre-write content.` : "File restored.");
  await loadRunDetail();
}

async function restoreSnapshot(runId) {
  if (!confirm("Put every tracked file back to the snapshot this run took before it started?\n\nAnything changed since then — your own edits included — is discarded for those files. Files that were never committed are left alone.")) return;
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/checkpoint/restore`, {
    method: "POST",
    body: JSON.stringify({}),
    confirmApproval: true,
  });
  if (!result.ok) {
    toast(result.data.error || "Could not restore the checkpoint.");
    return;
  }
  const data = result.data || {};
  const skipped = (data.skipped || []).length;
  toast(`Restored ${data.count} file${data.count === 1 ? "" : "s"} from the run's snapshot${skipped ? `, ${skipped} skipped` : ""}.`);
  await loadRunDetail();
}

async function applyRun(runId) {
  const warning = "Commit this run's own checkout, fast-forward the workspace onto its branch, and remove the checkout?"
    + " Uncommitted changes or newer commits in the workspace block the merge; nothing is forced.";
  if (!confirm(warning)) return;
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/apply`, {
    method: "POST",
    body: JSON.stringify({ approve: true }),
    confirmApproval: true,
  });
  if (!result.ok) {
    toast(result.data.error || "Could not apply the run's checkout.");
    return;
  }
  const data = result.data || {};
  const count = (data.applied || []).length;
  toast(`Applied ${count} file${count === 1 ? "" : "s"} from ${data.branch}${data.removed ? " and removed the checkout" : ""}.`);
  await loadRunDetail();
}

function diffBlock(text) {
  const pre = document.createElement("pre");
  pre.className = "run-diff";
  String(text).split("\n").forEach((line) => {
    const span = document.createElement("span");
    const isMeta = line.startsWith("@@") || line.startsWith("---") || line.startsWith("+++");
    span.className = isMeta ? "diff-meta" : line.startsWith("+") ? "diff-add" : line.startsWith("-") ? "diff-del" : "diff-ctx";
    span.textContent = `${line}\n`;
    pre.appendChild(span);
  });
  return pre;
}

async function submitRun(event) {
  event.preventDefault();
  const goal = $("runGoal").value.trim();
  if (!goal) return;
  const crewRoster = document.getElementById("runCrew");
  const stageBudget = document.getElementById("runStageMaxSteps");
  const stagePolicy = document.getElementById("runStagePolicy");
  const crewChips = document.getElementById("crewTemplates");
  const crewOnly = $("runMode").value !== "crew";
  for (const el of [crewRoster, stageBudget, stagePolicy, crewChips]) { if (el) el.classList.toggle("hidden", crewOnly); }
  const payload = {
    goal,
    profile: $("runProfile").value,
    mode: $("runMode").value,
    allowWrites: $("runAllowWrites").checked,
    allowCommands: $("runAllowCommands").checked,
    allowMcp: $("runAllowMcp").checked,
    autoApprove: $("runAutoApprove").checked,
  };
  const priority = Number($("runPriority").value);
  if (priority) payload.priority = priority;
  // Dependencies are ids the server validates; blank means "start as soon as a
  // slot frees" rather than a dependency on nothing.
  const dependsOn = ($("runDependsOn").value || "").split(",").map((id) => id.trim()).filter(Boolean);
  if (dependsOn.length) payload.dependsOn = dependsOn;
  if ($("runMode").value === "crew") {
    const roles = ($("runCrew").value || "").split(",").map((r) => r.trim()).filter(Boolean);
    if (roles.length) payload.crew = roles;
    const budget = Number($("runStageMaxSteps").value) || 0;
    if (budget) payload.stageMaxSteps = budget;
    if ($("runStagePolicy").value) payload.stagePolicy = $("runStagePolicy").value;
  }
  const result = await api("/api/runs", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not start the run.");
    return;
  }
  $("runGoal").value = "";
  const queued = result.data.run && result.data.run.status === "queued";
  toast(queued
    ? `Run queued${result.data.run.queuePosition ? ` (position ${result.data.run.queuePosition})` : ""} — it starts when a slot frees.`
    : "Agent run started.");
  state.openRun = result.data.run && result.data.run.id;
  state.runDetail = null;
  await loadRuns();
  if (state.openRun) await loadRunDetail();
}

async function loadCrewTemplates() {
  const host = document.getElementById("crewTemplates");
  if (!host) return;
  const result = await api("/api/crew-templates");
  const templates = result.ok && Array.isArray(result.data.templates) ? result.data.templates : [];
  host.textContent = "";
  for (const template of templates) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "btn btn-quiet btn-small run-crew-chip";
    chip.textContent = `${template.name} · ${(template.crew || []).join(" → ")}`;
    chip.title = "Fill the crew roster with this template";
    chip.addEventListener("click", () => {
      $("runCrew").value = (template.crew || []).join(", ");
    });
    host.appendChild(chip);
  }
  const save = document.createElement("button");
  save.type = "button";
  save.className = "btn btn-quiet btn-small run-crew-chip";
  save.textContent = "Save current roles";
  save.title = "Save the roster in the field above as a reusable template";
  save.addEventListener("click", saveCrewTemplate);
  host.appendChild(save);
}

async function saveCrewTemplate() {
  const roles = ($("runCrew").value || "").split(",").map((r) => r.trim()).filter(Boolean);
  if (!roles.length) {
    toast("Type crew roles above first, then save them as a template.");
    return;
  }
  const name = (window.prompt("Name for this crew template:") || "").trim();
  if (!name) return;
  const result = await api("/api/crew-templates", {
    method: "POST",
    body: JSON.stringify({ name, crew: roles }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not save the template.");
    return;
  }
  toast("Crew template saved.");
  await loadCrewTemplates();
}

async function resumeAgentRun(runId) {
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/resume`, {
    method: "POST",
    body: JSON.stringify({}),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not resume the run.");
  } else {
    toast("Run resumed — finished stages stay finished.");
  }
  await loadRuns();
  if (runId === state.openRun) await loadRunDetail();
}

async function retryAgentRun(runId) {
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/retry`, {
    method: "POST",
    body: JSON.stringify({}),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not retry the run.");
    return;
  }
  const created = result.data.run || {};
  toast(created.status === "queued"
    ? `Retry queued${created.queuePosition ? ` (position ${created.queuePosition})` : ""} — the new run has an empty step log.`
    : "Retry started — the new run has an empty step log.");
  state.openRun = created.id || null;
  state.runDetail = null;
  await loadRuns();
  if (state.openRun) await loadRunDetail();
}

async function answerApproval(runId, approve) {
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/approve`, {
    method: "POST",
    body: JSON.stringify({ approve }),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not answer the approval.");
  }
  if (approve && result.ok) toast("Approved — the agent continues.");
  await loadRuns();
  if (runId === state.openRun) await loadRunDetail();
}

async function emergencyStopAll() {
  if (!confirm("Stop every live agent run and close every terminal session right now?")) return;
  const result = await api("/api/emergency-stop", {
    method: "POST",
    body: JSON.stringify({}),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not run the emergency stop.");
    return;
  }
  // The count the server reports is what actually stopped — an empty response
  // means nothing was live, and that is worth saying too.
  const runs = (result.data.runs || []).length;
  const terminals = (result.data.terminals || []).length;
  toast(runs || terminals
    ? `Stopped ${runs} run${runs === 1 ? "" : "s"} and closed ${terminals} terminal${terminals === 1 ? "" : "s"}.`
    : "Nothing was live — every run and terminal is already stopped.");
  state.openRun = null;
  state.runDetail = null;
  // Every session was closed server-side; drop the client's attachment so the
  // panel's Restart opens a fresh shell instead of talking to a dead id.
  if (termState.source) termState.source.close();
  termState.source = null;
  termState.id = null;
  await loadRuns();
}

async function pauseAgentRun(runId) {
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/pause`, {
    method: "POST",
    body: JSON.stringify({}),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not pause the run.");
  } else if (result.data.status === "paused") {
    toast("Run paused — resume continues from the step log.");
  } else if (result.data.pauseRequested) {
    toast("Pause requested — the run stops after the step it is executing.");
  } else {
    // The run finished or was cancelled while the request was in flight; say
    // what the record actually holds instead of promising a pause.
    toast(`The run is ${String(result.data.status || "no longer running").replace("_", " ")}.`);
  }
  await loadRuns();
  if (runId === state.openRun) await loadRunDetail();
}

async function cancelAgentRun(runId) {
  const result = await api(`/api/runs/${encodeURIComponent(runId)}/cancel`, {
    method: "POST",
    body: JSON.stringify({}),
  });
  if (!result.ok) {
    toast(result.data.error || "Could not cancel the run.");
  } else {
    toast("Run cancelled.");
  }
  await loadRuns();
  if (runId === state.openRun) await loadRunDetail();
}

(async function init() {
  loadTheme();
  setupThemeToggle();
  applyAccent(localStorage.getItem("freeai-accent") || "classic");
  setupSettingsNav();
  renderAccentPicker();
  setSettingsSection(localStorage.getItem("freeai-settings-section") || "connection");
  bindEvents();
  setupLocalControls();
  $("runsForm").addEventListener("submit", submitRun);
  $("emergencyStopBtn").addEventListener("click", emergencyStopAll);
  setSidebarMode("threads");
  setView("chat");
  autoGrow($("prompt"));
  renderTabs();
  renderContextChip();
  renderSkillChip();
  renderBackendPresets();
  state.lastContextTokens = null;
  setTimeout(updateContextMeter, 50);
  await Promise.all([loadSettings(), loadProjects(), loadConversations(), loadWorkspace()]);
  await Promise.all([  loadUsage(), loadSkills(), loadCrewTemplates()]);
  probeRouter();
  setupMirroring();
  if (state.conversations.length) {
    await openConversation(state.conversations[0].id);
  }
  if (state.workspace) setSidebarMode("files");
  setupTerminal();
  if (state.panelShow.bottom) showTerminalPanel();
})();

/* ------------------------------------------------------------------ */
/* bottom panel: terminal                                               */
/* ------------------------------------------------------------------ */

/* A plain-text view of the PTY (nothing vendored): the shell echoes
   everything, so we capture keystrokes server-side and draw what comes back,
   with ANSI control sequences stripped. */
const termState = { open: false, id: null, source: null, cwd: "", busy: false };

function showTerminalPanel() {
  if (!state.workspace) return;
  const cwd = state.workspace.absolute_path || state.workspace.path || "";
  if (!cwd) return;
  void openTerminalSession(cwd);
}

// Called whenever the open workspace changes (or loads): attach the panel to
// the new folder when the bottom panel is visible.
function terminalFollowWorkspace() {
  if (state.panelShow?.bottom) showTerminalPanel();
}

async function openTerminalSession(cwd) {
  if (termState.busy || !cwd) return;
  // Same folder, already attached: nothing to do (keeps scrollback alive).
  if (termState.id && termState.cwd === cwd) return;
  termState.busy = true;
  try {
    if (termState.id) {
      await api(`/api/terminal/${termState.id}/close`, { method: "POST", body: JSON.stringify({}) });
      if (termState.source) termState.source.close();
      termState.id = null;
      termState.source = null;
      $("terminalHost").replaceChildren();
    }
    const result = await api("/api/terminal/open", {
      method: "POST",
      confirmApproval: true,
      body: JSON.stringify({ cwd, ...terminalSize() }),
    });
    if (!result.ok || !result.data.ok) {
      toast(result.data.error || "Could not open a terminal.");
      return;
    }
    termState.id = result.data.id;
    termState.cwd = result.data.cwd;
    $("terminalFolder").textContent = result.data.cwd;
    if (result.data.reused) $("terminalHost").replaceChildren();
    void connectTerminalStream();
  } finally {
    termState.busy = false;
  }
}

function connectTerminalStream() {
  const pre = document.createElement("pre");
  pre.className = "terminal";
  pre.tabIndex = 0;
  pre.setAttribute("aria-label", "Terminal output");
  $("terminalHost").replaceChildren(pre);
  // A blank session opens with a visible caret line instead of feeling dead.
  pre.append(document.createTextNode("\n"));
  if (termState.source) termState.source.close();
  const source = new EventSource(`/api/terminal/${termState.id}/stream`);
  termState.source = source;
  source.onmessage = (event) => {
    try {
      const chunk = JSON.parse(event.data);
      if (chunk === "__exit__") {
        pre.append(document.createTextNode("\n[process exited — click Restart for a new shell]\n"));
        return;
      }
      pre.append(document.createTextNode(termSanitize(chunk)));
      pre.scrollTop = pre.scrollHeight;
    } catch { /* ignore malformed frames */ }
  };
  source.addEventListener("done", () => { source.close(); });
  pre.focus();
}

function termSanitize(text) {
  return text
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "") // window title
    .replace(/\x1b\[[?0-9;]*[ -/]*[@-~]/g, "")            // cursor + color
    .replace(/\x1b[^\x1b]/g, "")                           // anything left
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "");
}

function terminalSize() {
  const host = $("terminalHost");
  const box = host.getBoundingClientRect();
  return {
    cols: Math.max(20, Math.floor(box.width / 7.2)),
    rows: Math.max(5, Math.floor(box.height / 17)),
  };
}

function terminalFit() {
  if (!termState.id || termState.busy) return;
  const { cols, rows } = terminalSize();
  void api(`/api/terminal/${termState.id}/resize`, {
    method: "POST",
    body: JSON.stringify({ cols, rows }),
  });
}

const TERM_KEYS = {
  Enter: "\r",
  Backspace: "\x7f",
  Tab: "\t",
  ArrowUp: "\x1b[A",
  ArrowDown: "\x1b[B",
  ArrowRight: "\x1b[C",
  ArrowLeft: "\x1b[D",
  Home: "\x1b[H",
  End: "\x1b[F",
  Delete: "\x1b[3~",
  PageUp: "\x1b[5~",
  PageDown: "\x1b[6~",
};

function terminalSend(data) {
  if (!termState.id) return;
  void api(`/api/terminal/${termState.id}/input`, {
    method: "POST",
    body: JSON.stringify({ data }),
  });
}

function setupTerminal() {
  const host = $("terminalHost");
  host.addEventListener("click", () => host.querySelector("pre")?.focus());
  host.addEventListener("keydown", (event) => {
    if (event.ctrlKey) {
      if (event.key === "c") { event.preventDefault(); terminalSend("\x03"); return; }
      if (event.key === "d") { event.preventDefault(); terminalSend("\x04"); return; }
      if (event.key === "u") { event.preventDefault(); terminalSend("\x15"); return; }
      return; // browser shortcuts pass through
    }
    if (event.metaKey || event.altKey) return;
    const mapped = TERM_KEYS[event.key];
    if (mapped) {
      event.preventDefault();
      terminalSend(mapped);
    } else if (event.key.length === 1) {
      event.preventDefault();
      terminalSend(event.key);
    }
  });
  host.addEventListener("paste", (event) => {
    event.preventDefault();
    terminalSend(event.clipboardData ? event.clipboardData.getData("text") : "");
  });
  $("terminalRestart").addEventListener("click", () => {
    if (state.workspace && !termState.busy) {
      void openTerminalSession(state.workspace.absolute_path || state.workspace.path || "");
    }
  });
  if (window.ResizeObserver) {
    let timer = null;
    new ResizeObserver(() => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => terminalFit(), 250);
    }).observe(host);
  }
}
