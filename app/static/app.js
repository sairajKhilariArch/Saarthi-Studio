const $ = (id) => document.getElementById(id);
let currentConversation = null;
let busy = false;
let conversations = [];

function escapeHtml(s) { return String(s ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c])); }
function toast(msg) {
  $("toast").textContent = msg; $("toast").classList.remove("hidden");
  clearTimeout(toast.timer); toast.timer = setTimeout(() => $("toast").classList.add("hidden"), 3800);
}
async function api(path, options={}) {
  const response = await fetch(path, {headers:{"Content-Type":"application/json", ...(options.headers||{})}, ...options});
  const data = await response.json().catch(() => ({}));
  if (!response.ok && !data.error) data.error = `Request failed (HTTP ${response.status})`;
  return {ok:response.ok, status:response.status, data};
}
function setBusy(value) {
  busy = value; $("send").disabled = value; $("askAboutCode").disabled = value;
  $("thinking").classList.toggle("hidden", !value);
}
function showTab(tab) {
  const chat = tab === "chat";
  $("chatTab").classList.toggle("hidden", !chat);
  $("settingsTab").classList.toggle("hidden", tab !== "settings");
  $("usageTab").classList.toggle("hidden", tab !== "usage");
  $("pageTitle").textContent = chat ? "AI Coding Chat" : tab === "settings" ? "Router settings" : "Usage & errors";
  document.querySelectorAll(".nav-button").forEach(b => b.classList.toggle("active", b.dataset.tab === tab));
  if (tab === "settings") loadSettings();
  if (tab === "usage") loadUsage();
}
function renderConversations() {
  const box = $("conversationList"); box.replaceChildren();
  if (!conversations.length) { box.innerHTML = '<div class="muted pad">No conversations yet.</div>'; return; }
  conversations.forEach(c => {
    const item = document.createElement("div");
    item.className = "conversation-item" + (c.id === currentConversation ? " active" : "");
    item.innerHTML = `<span>▤</span><span class="conv-title" title="${escapeHtml(c.title)}">${escapeHtml(c.title)}</span><button class="delete-conv" title="Delete conversation" aria-label="Delete conversation">×</button>`;
    item.addEventListener("click", e => { if (e.target.closest(".delete-conv")) return; openConversation(c.id); });
    item.querySelector(".delete-conv").addEventListener("click", async e => {
      e.stopPropagation();
      if (!confirm(`Delete "${c.title}" and its local history?`)) return;
      await api(`/api/conversations/${encodeURIComponent(c.id)}/delete`, {method:"POST", body:"{}"});
      if (currentConversation === c.id) { currentConversation = null; $("messages").replaceChildren(); $("welcome").classList.remove("hidden"); }
      await loadConversations();
    });
    box.appendChild(item);
  });
}
async function loadConversations() {
  const r = await api("/api/conversations");
  if (!r.ok) return toast(r.data.error || "Could not load history.");
  conversations = r.data; renderConversations();
}
function addMessage(role, content, meta=null, isError=false) {
  $("welcome").classList.add("hidden");
  const row = document.createElement("div"); row.className = `message ${role}` + (isError ? " error" : "");
  const avatar = document.createElement("div"); avatar.className = "avatar"; avatar.textContent = role === "user" ? "Y" : isError ? "!" : "✦";
  const body = document.createElement("div"); body.className = "bubble"; body.textContent = content;
  const wrap = document.createElement("div"); wrap.style.minWidth = "0"; wrap.style.maxWidth = "100%"; wrap.appendChild(body);
  if (meta) {
    const m = document.createElement("div"); m.className = "message-meta";
    const parts = [];
    if (meta.provider) parts.push(meta.provider);
    if (meta.latency_ms != null) parts.push(`${meta.latency_ms} ms`);
    if (meta.total_tokens != null) parts.push(`${meta.total_tokens} tokens`);
    if (meta.fallback_attempts) parts.push(`fallback attempts: ${meta.fallback_attempts}`);
    m.textContent = parts.join(" · "); wrap.appendChild(m);
  }
  row.append(avatar, wrap); $("messages").appendChild(row); $("messages").scrollTop = $("messages").scrollHeight;
}
async function openConversation(id) {
  currentConversation = id;
  const r = await api(`/api/conversations/${encodeURIComponent(id)}/messages`);
  if (!r.ok) return toast(r.data.error || "Could not load conversation.");
  $("messages").replaceChildren();
  $("welcome").classList.toggle("hidden", r.data.length > 0);
  r.data.forEach(m => addMessage(m.role, m.content));
  renderConversations(); showTab("chat");
}
async function newConversation() {
  const r = await api("/api/conversations", {method:"POST", body:JSON.stringify({title:"New conversation"})});
  if (!r.ok) return toast(r.data.error || "Could not create conversation.");
  currentConversation = r.data.id; $("messages").replaceChildren(); $("welcome").classList.remove("hidden");
  await loadConversations(); showTab("chat"); $("prompt").focus();
}
async function sendMessage(prefill=null) {
  if (busy) return;
  const message = (prefill ?? $("prompt").value).trim();
  if (!message) { $("prompt").focus(); return; }
  if (!currentConversation) {
    const r = await api("/api/conversations", {method:"POST", body:JSON.stringify({title:message.slice(0,64)})});
    if (!r.ok) return toast(r.data.error || "Could not create conversation.");
    currentConversation = r.data.id;
  }
  const code = $("codeEditor").value;
  const language = $("language").value;
  $("prompt").value = "";
  addMessage("user", message + (code.trim() ? `\n\n[${language} code attached]` : ""));
  setBusy(true);
  const r = await api("/api/chat", {method:"POST", body:JSON.stringify({conversation_id:currentConversation,message,code,language})});
  setBusy(false);
  if (r.ok && r.data.ok) addMessage("assistant", r.data.answer, r.data.meta);
  else addMessage("assistant", r.data.error || "The request failed.", r.data.meta, true);
  await loadConversations(); await loadUsageSummary();
}
async function loadSettings() {
  const r = await api("/api/config"); if (!r.ok) return;
  $("baseUrl").value = r.data.base_url || "";
  $("model").value = r.data.model || "auto";
  $("apiKey").value = "";
  $("apiKey").placeholder = r.data.has_api_key ? "Saved key (leave blank to keep it)" : "Paste your router API key";
  $("keyStatus").textContent = r.data.has_api_key ? "A key is saved locally. Leave the field blank to keep it, or enter a new key to replace it." : "Key not configured. The key is saved in a permission-restricted local config file.";
}
function settingsMessage(msg, ok) {
  const box = $("settingsMessage"); box.textContent = msg; box.className = "message-box " + (ok ? "success" : "error");
}
async function saveSettings(e) {
  if (e) e.preventDefault();
  const payload = {base_url:$("baseUrl").value.trim(), model:$("model").value.trim() || "auto"};
  if ($("apiKey").value.trim()) payload.api_key = $("apiKey").value.trim();
  const r = await api("/api/config", {method:"POST", body:JSON.stringify(payload)});
  if (!r.ok) return settingsMessage(r.data.error || "Could not save settings.", false);
  $("apiKey").value = ""; settingsMessage("Settings saved locally.", true);
  $("connectionLabel").textContent = "Settings saved"; $("apiKey").placeholder = "Saved key (leave blank to keep it)";
}
async function testConnection() {
  await saveSettings();
  const r = await api("/api/test", {method:"POST", body:"{}"});
  if (r.ok && r.data.ok) { settingsMessage(r.data.message, true); $("connectionLabel").textContent = "Router connected"; document.querySelector(".status-pill").classList.add("online"); toast("Router connection successful."); }
  else { settingsMessage(r.data.error || "Connection test failed.", false); $("connectionLabel").textContent = "Connection failed"; document.querySelector(".status-pill").classList.remove("online"); }
}
async function loadUsageSummary() {
  const r = await api("/api/usage"); if (!r.ok) return;
  const s = r.data.summary || {};
  $("statRequests").textContent = s.requests || 0; $("statSuccess").textContent = s.successful || 0;
  $("statErrors").textContent = s.failed || 0; $("statTokens").textContent = Number(s.total_tokens || 0).toLocaleString();
  $("statLatency").textContent = s.avg_latency_ms ? `${Math.round(s.avg_latency_ms)} ms` : "—";
}
async function loadUsage() {
  const r = await api("/api/usage"); if (!r.ok) return;
  const s = r.data.summary || {};
  $("statRequests").textContent = s.requests || 0; $("statSuccess").textContent = s.successful || 0;
  $("statErrors").textContent = s.failed || 0; $("statTokens").textContent = Number(s.total_tokens || 0).toLocaleString();
  $("statLatency").textContent = s.avg_latency_ms ? `${Math.round(s.avg_latency_ms)} ms` : "—";
  const body = $("usageRows"); body.replaceChildren();
  if (!r.data.requests.length) { body.innerHTML = '<tr><td colspan="6" class="muted">No requests yet. Send a chat message to start recording usage.</td></tr>'; return; }
  r.data.requests.forEach(x => {
    const tr = document.createElement("tr");
    const when = new Date(x.created_at*1000).toLocaleString();
    const tokens = x.total_tokens == null ? "not reported" : Number(x.total_tokens).toLocaleString();
    const detail = x.status === "ok" ? (x.fallback_attempts ? `Fallback attempts: ${x.fallback_attempts}` : "Completed") : (x.error || "Request failed");
    [when, x.status, `${x.provider || "unknown"}${x.model ? " · "+x.model : ""}`, x.latency_ms == null ? "—" : `${x.latency_ms} ms`, tokens, detail].forEach((v,i) => {
      const td = document.createElement("td"); td.textContent = v || "—";
      if (i===1) td.className = x.status === "ok" ? "status-ok" : "status-error";
      if (i===5) td.className = "detail-cell";
      tr.appendChild(td);
    });
    body.appendChild(tr);
  });
}
function openProviderDashboard() {
  window.open("http://127.0.0.1:3001", "_blank", "noopener,noreferrer");
}
$("openProviderDashboard").addEventListener("click", openProviderDashboard);
$("openProviderDashboardSettings").addEventListener("click", openProviderDashboard);
$("newChat").addEventListener("click", newConversation);
$("openSettings").addEventListener("click", () => showTab("settings"));
document.querySelectorAll("[data-tab]").forEach(b => b.addEventListener("click", () => showTab(b.dataset.tab)));
document.querySelectorAll("[data-prompt]").forEach(b => b.addEventListener("click", () => { $("prompt").value = b.dataset.prompt; $("prompt").focus(); }));
$("send").addEventListener("click", () => sendMessage());
$("prompt").addEventListener("keydown", e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(); }});
$("askAboutCode").addEventListener("click", () => sendMessage("Please review the code in the editor. Explain what it does, identify bugs or risks, and suggest an improved version."));
$("clearCode").addEventListener("click", () => { $("codeEditor").value = ""; updateCodeCount(); });
$("copyCode").addEventListener("click", async () => {
  try { await navigator.clipboard.writeText($("codeEditor").value); toast("Code copied."); }
  catch { $("codeEditor").select(); document.execCommand("copy"); toast("Code copied."); }
});
$("codeEditor").addEventListener("input", updateCodeCount);
function updateCodeCount() { $("codeCount").textContent = `${$("codeEditor").value.length.toLocaleString()} characters`; }
$("settingsForm").addEventListener("submit", saveSettings);
$("testConnection").addEventListener("click", testConnection);
$("refreshUsage").addEventListener("click", loadUsage);
(async function init() {
  await loadSettings(); await loadConversations(); await loadUsageSummary();
  if (conversations.length) await openConversation(conversations[0].id);
})();
