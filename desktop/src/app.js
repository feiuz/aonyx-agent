// Aonyx Desktop — talks to `aonyx serve api` through Rust-side Tauri
// commands (no CORS, sandboxed webview). V4.6 streaming chat + V4.7 sessions
// sidebar and memory-palace search.

const tauri = window.__TAURI__;
const invoke = tauri && tauri.core && tauri.core.invoke;

const $ = (id) => document.getElementById(id);
const log = $("log");
const input = $("input");
const sendBtn = $("send");
const dot = $("dot");
const statusText = $("statusText");
const sessionsEl = $("sessions");
const hitsEl = $("hits");

const store = {
  get url() {
    return localStorage.getItem("aonyx.apiUrl") || "http://127.0.0.1:8788";
  },
  set url(v) {
    localStorage.setItem("aonyx.apiUrl", v);
  },
  get token() {
    return localStorage.getItem("aonyx.token") || "";
  },
  set token(v) {
    localStorage.setItem("aonyx.token", v);
  },
  // Embedded local agent on by default; the app launches `aonyx serve api`.
  get local() {
    return localStorage.getItem("aonyx.local") !== "0";
  },
  set local(v) {
    localStorage.setItem("aonyx.local", v ? "1" : "0");
  },
};

let sessionId = null;
let busy = false;
// The active endpoint (set by connect(): the local sidecar, or the remote URL).
let endpoint = { url: "http://127.0.0.1:8788", token: "" };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function setStatus(state, text) {
  dot.className = "dot" + (state ? " " + state : "");
  statusText.textContent = text;
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// Minimal markdown. The source is HTML-escaped first, so the worst case is
// imperfect formatting — never injection.
function mdInline(s) {
  return s
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^*])\*([^*]+)\*/g, "$1<em>$2</em>")
    .replace(
      /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
      '<a href="$2" target="_blank" rel="noreferrer">$1</a>',
    );
}

function renderMarkdown(src) {
  const lines = escapeHtml(src).split("\n");
  let html = "";
  let list = null;
  const closeList = () => {
    if (list) {
      html += "</" + list + ">";
      list = null;
    }
  };
  for (let i = 0; i < lines.length; ) {
    const line = lines[i];
    if (line.startsWith("```")) {
      closeList();
      i++;
      const buf = [];
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++; // skip closing fence
      html += "<pre><code>" + buf.join("\n") + "</code></pre>";
      continue;
    }
    const h = line.match(/^(#{1,3})\s+(.*)$/);
    if (h) {
      closeList();
      const lvl = h[1].length + 2; // # -> h3
      html += "<h" + lvl + ">" + mdInline(h[2]) + "</h" + lvl + ">";
      i++;
      continue;
    }
    const ul = line.match(/^\s*[-*]\s+(.*)$/);
    if (ul) {
      if (list !== "ul") {
        closeList();
        html += "<ul>";
        list = "ul";
      }
      html += "<li>" + mdInline(ul[1]) + "</li>";
      i++;
      continue;
    }
    const ol = line.match(/^\s*\d+\.\s+(.*)$/);
    if (ol) {
      if (list !== "ol") {
        closeList();
        html += "<ol>";
        list = "ol";
      }
      html += "<li>" + mdInline(ol[1]) + "</li>";
      i++;
      continue;
    }
    if (line.trim() === "") {
      closeList();
      i++;
      continue;
    }
    closeList();
    html += "<p>" + mdInline(line) + "</p>";
    i++;
  }
  closeList();
  return html;
}

function clearEmpty() {
  const e = $("empty");
  if (e) e.remove();
}

function resetLog() {
  log.innerHTML = "";
}

function args(extra) {
  return Object.assign({ base: endpoint.url, token: endpoint.token }, extra);
}

// Render a finished message (used for loaded transcripts).
function addMsg(role, text, opts = {}) {
  clearEmpty();
  const wrap = document.createElement("div");
  wrap.className = "msg " + role;
  const r = document.createElement("div");
  r.className = "role";
  r.textContent = role === "user" ? "you" : "aonyx";
  wrap.appendChild(r);
  const b = document.createElement("div");
  b.className = "bubble" + (opts.error ? " error" : "") + (opts.md ? " md" : "");
  b.innerHTML = opts.md ? renderMarkdown(text) : escapeHtml(text);
  wrap.appendChild(b);
  if (opts.tools && opts.tools.length) {
    const t = document.createElement("div");
    t.className = "tools";
    t.textContent = "called: " + opts.tools.join(", ");
    wrap.appendChild(t);
  }
  log.appendChild(wrap);
  log.scrollTop = log.scrollHeight;
  return b;
}

function toolNamesOf(msg) {
  return (msg.tool_calls || []).map((tc) => tc && tc.name).filter(Boolean);
}

// ---- connection + sessions ----

// Probe /v1/info, retrying while a freshly-spawned local agent binds.
async function probeInfo(tries) {
  for (let i = 0; i < tries; i++) {
    try {
      return await invoke("api_info", args());
    } catch (e) {
      if (i === tries - 1) throw e;
      await sleep(350);
    }
  }
}

async function connect() {
  if (!invoke) {
    setStatus("err", "not running in Tauri");
    return false;
  }
  sessionId = null;
  setStatus("", store.local ? "starting local agent…" : "connecting…");
  try {
    endpoint = store.local
      ? { url: await invoke("start_local"), token: "" }
      : { url: store.url, token: store.token };
    const info = await probeInfo(store.local ? 14 : 2);
    setStatus("ok", `${info.provider} · ${info.model}`);
    await loadSessions();
    return true;
  } catch (e) {
    setStatus("err", String(e));
    return false;
  }
}

async function loadSessions() {
  if (!invoke) return;
  try {
    const list = await invoke("api_list_sessions", args({ project: null }));
    renderSessions(Array.isArray(list) ? list : []);
  } catch {
    sessionsEl.innerHTML = '<li class="muted">— unavailable —</li>';
  }
}

function renderSessions(list) {
  sessionsEl.innerHTML = "";
  if (!list.length) {
    sessionsEl.innerHTML = '<li class="muted">no sessions yet</li>';
    return;
  }
  for (const s of list) {
    const li = document.createElement("li");
    if (s.id === sessionId) li.className = "active";
    const title = document.createElement("span");
    title.textContent = s.title || "(untitled)";
    const meta = document.createElement("span");
    meta.className = "meta";
    meta.textContent = `${s.turns} turn${s.turns === 1 ? "" : "s"}`;
    li.append(title, meta);
    li.addEventListener("click", () => switchSession(s.id));
    sessionsEl.appendChild(li);
  }
}

async function switchSession(id) {
  if (!invoke || busy) return;
  try {
    const rec = await invoke("api_get_session", args({ session: id }));
    sessionId = id;
    resetLog();
    for (const m of rec.messages || []) {
      if (m.role === "user" && m.content) addMsg("user", m.content);
      else if (m.role === "assistant" && (m.content || toolNamesOf(m).length))
        addMsg("assistant", m.content || "", { tools: toolNamesOf(m), md: true });
    }
    if (!log.children.length) {
      log.innerHTML = '<div class="empty"><p class="muted">empty session — say hello</p></div>';
    }
    [...sessionsEl.children].forEach((li) => li.classList.remove("active"));
    refreshActive();
  } catch (e) {
    setStatus("err", String(e));
  }
}

function refreshActive() {
  // re-mark the active row by reloading the list (cheap)
  loadSessions();
}

async function ensureSession() {
  if (sessionId) return sessionId;
  const rec = await invoke("api_create_session", args({ project: null }));
  sessionId = rec.id;
  loadSessions();
  return sessionId;
}

async function newSession() {
  if (!invoke) return;
  try {
    const rec = await invoke("api_create_session", args({ project: null }));
    sessionId = rec.id;
    resetLog();
    log.innerHTML = '<div class="empty"><p class="muted">new session — say hello</p></div>';
    loadSessions();
  } catch (e) {
    setStatus("err", String(e));
  }
}

// ---- memory search ----

async function searchMemory(q) {
  if (!invoke || !q.trim()) return;
  hitsEl.innerHTML = '<li class="muted">searching…</li>';
  try {
    const hits = await invoke("api_memory_search", args({ q, k: 8 }));
    renderHits(Array.isArray(hits) ? hits : []);
  } catch (e) {
    hitsEl.innerHTML = `<li class="muted">${escapeHtml(String(e))}</li>`;
  }
}

function renderHits(hits) {
  hitsEl.innerHTML = "";
  if (!hits.length) {
    hitsEl.innerHTML = '<li class="muted">no matches</li>';
    return;
  }
  for (const h of hits) {
    const li = document.createElement("li");
    const score = document.createElement("span");
    score.className = "score";
    score.textContent = (h.score || 0).toFixed(2);
    const txt = document.createElement("span");
    txt.textContent = (h.content || "").slice(0, 240);
    li.append(score, txt);
    hitsEl.appendChild(li);
  }
}

// ---- send (streaming) ----

async function send() {
  const text = input.value.trim();
  if (!text || busy) return;
  if (!invoke) {
    addMsg("assistant", "This build is not running inside Tauri.", { error: true });
    return;
  }
  try {
    if (!(await ensureSessionOrConnect())) return;
  } catch (e) {
    addMsg("assistant", String(e), { error: true });
    return;
  }

  addMsg("user", text);
  input.value = "";
  autoGrow();
  busy = true;
  sendBtn.disabled = true;

  clearEmpty();
  const wrap = document.createElement("div");
  wrap.className = "msg assistant";
  const role = document.createElement("div");
  role.className = "role";
  role.textContent = "aonyx";
  const bubble = document.createElement("div");
  bubble.className = "bubble thinking";
  bubble.textContent = "…";
  const toolsEl = document.createElement("div");
  toolsEl.className = "tools";
  toolsEl.style.display = "none";
  wrap.append(role, bubble, toolsEl);
  log.appendChild(wrap);
  log.scrollTop = log.scrollHeight;

  let acc = "";
  const tools = [];
  const renderTools = () => {
    if (tools.length) {
      toolsEl.style.display = "";
      toolsEl.textContent = "called: " + tools.join(", ");
    }
  };
  const startText = () => {
    if (bubble.classList.contains("thinking")) {
      bubble.classList.remove("thinking");
      bubble.textContent = "";
    }
  };

  const channel = new tauri.core.Channel();
  channel.onmessage = (frame) => {
    switch (frame && frame.type) {
      case "delta":
        startText();
        acc += frame.text || "";
        bubble.textContent = acc;
        log.scrollTop = log.scrollHeight;
        break;
      case "tool_start":
        if (frame.name) tools.push(frame.name);
        renderTools();
        log.scrollTop = log.scrollHeight;
        break;
      case "done":
        bubble.classList.remove("thinking");
        bubble.classList.add("md");
        bubble.innerHTML = renderMarkdown(acc || frame.reply || "(no reply)");
        renderTools();
        log.scrollTop = log.scrollHeight;
        break;
      case "error":
        bubble.classList.remove("thinking");
        bubble.classList.add("error");
        bubble.textContent = frame.message || "stream error";
        break;
      default:
        break;
    }
  };

  try {
    await invoke(
      "api_stream",
      args({ session: sessionId, content: text, onEvent: channel }),
    );
    if (bubble.classList.contains("thinking")) {
      bubble.classList.remove("thinking");
      bubble.classList.add("md");
      bubble.innerHTML = renderMarkdown(acc || "(no reply)");
    }
    loadSessions(); // refresh titles / turn counts
  } catch (e) {
    bubble.classList.remove("thinking");
    bubble.classList.add("error");
    bubble.textContent = String(e);
    setStatus("err", "request failed");
  } finally {
    busy = false;
    sendBtn.disabled = false;
    input.focus();
  }
}

async function ensureSessionOrConnect() {
  if (sessionId) return true;
  if (!dot.classList.contains("ok")) {
    if (!(await connect())) return false;
  }
  await ensureSession();
  return !!sessionId;
}

function autoGrow() {
  input.style.height = "auto";
  input.style.height = Math.min(input.scrollHeight, 144) + "px";
}

// ---- provider wizard ----
const PROVIDER_DEFAULTS = {
  anthropic: { base: "" },
  openai: { base: "https://api.openai.com" },
  openrouter: { base: "" },
  ollama: { base: "http://localhost:11434" },
  "lm-studio": { base: "http://localhost:1234" },
  "claude-code": { base: "" },
};
const MODEL_CUSTOM = "__custom__";
// Providers whose model *list* requires an API key to query.
const LIST_NEEDS_KEY = { anthropic: true, openai: true };

// Live-fetch the models a provider actually exposes (Rust `list_models`) and
// fill the <select>. No hardcoded suggestions; signals when a key is required.
async function loadModels(provider, current) {
  const sel = $("wzModel");
  const note = $("wzModelNote");
  if (!invoke) return;

  if (provider === "claude-code") {
    // The `claude` CLI takes an alias (sonnet/opus/haiku) or a full id — there
    // is no HTTP models endpoint, so these are its accepted values.
    setModelOptions(["sonnet", "opus", "haiku"], current);
    note.textContent = "Claude Code — alias CLI ; « Custom » pour un id daté précis.";
    return;
  }
  if (LIST_NEEDS_KEY[provider] && !$("wzKey").value.trim()) {
    setModelOptions([], current);
    note.textContent = "🔑 Saisis ta clé API, puis ↻ pour lister les modèles.";
    return;
  }

  const base =
    $("wzBase").value.trim() || (PROVIDER_DEFAULTS[provider] || {}).base || "";
  const key = $("wzKey").value;
  note.textContent = "chargement des modèles…";
  sel.disabled = true;
  try {
    const models = await invoke("list_models", { provider, base, key });
    setModelOptions(models || [], current);
    note.textContent =
      models && models.length
        ? `${models.length} modèles disponibles`
        : "aucun modèle retourné — utilise Custom.";
  } catch (e) {
    const msg = String(e);
    setModelOptions([], current);
    note.textContent = msg.includes("API_KEY_REQUIRED")
      ? "🔑 Clé API requise — saisis-la puis ↻."
      : "fetch impossible (" + msg + ") — démarre le serveur ou utilise Custom.";
  } finally {
    sel.disabled = false;
  }
}

// (Re)build the model <select> from a list + a Custom… option; preselect current.
function setModelOptions(models, current) {
  const sel = $("wzModel");
  // Keep the currently-configured model as a real, selectable entry — an
  // existing config must never be forced into the "Custom…" typing path.
  const list =
    current && !models.includes(current) ? [current, ...models] : models;
  sel.innerHTML = "";
  for (const m of list) {
    const o = document.createElement("option");
    o.value = m;
    o.textContent = m;
    sel.appendChild(o);
  }
  const custom = document.createElement("option");
  custom.value = MODEL_CUSTOM;
  custom.textContent = "Custom… (saisir l'id)";
  sel.appendChild(custom);

  if (current && list.includes(current)) {
    sel.value = current;
    showModelCustom(false);
  } else {
    sel.value = list[0] || MODEL_CUSTOM;
    showModelCustom(sel.value === MODEL_CUSTOM);
  }
}

function showModelCustom(on) {
  $("wzModelCustomRow").style.display = on ? "" : "none";
}

// Effective model id: the custom input when "Custom…" is picked, else the select.
function selectedModel() {
  const v = $("wzModel").value;
  return v === MODEL_CUSTOM ? $("wzModelCustom").value.trim() : v;
}

function wzErr(msg) {
  const n = $("wzNote");
  n.classList.add("err");
  n.textContent = msg;
}

function wzReflect() {
  const p = $("wzProvider").value;
  document.querySelectorAll("#wizard .wz-field").forEach((el) => {
    const on = (el.dataset.for || "").split(" ").includes(p);
    el.classList.toggle("hidden", !on);
  });
  const d = PROVIDER_DEFAULTS[p] || {};
  $("wzBase").placeholder = d.base || "https://…";
}

async function openWizard() {
  if (!invoke) return;
  try {
    const c = await invoke("read_provider_config");
    $("wzProvider").value = c.provider || "anthropic";
    loadModels(c.provider || "anthropic", c.model);
    $("wzKey").value =
      c.anthropic_api_key || c.openai_api_key || c.openrouter_api_key || "";
    $("wzBase").value =
      c.openai_base_url || c.ollama_base_url || c.lm_studio_base_url || "";
    $("wzBin").value = c.claude_code_binary || "";
  } catch {}
  $("wzNote").textContent = "";
  $("wzNote").classList.remove("err");
  wzReflect();
  $("settings").classList.add("hidden");
  $("workspace").style.display = "none";
  $("wizard").classList.remove("hidden");
}

function closeWizard() {
  $("wizard").classList.add("hidden");
  $("workspace").style.display = "";
}

async function wzSave() {
  if (!invoke) return;
  const p = $("wzProvider").value;
  const model = selectedModel();
  if (!model) return wzErr("Pick a model (or enter a custom id).");
  const key = $("wzKey").value;
  const base = $("wzBase").value.trim();
  if (["anthropic", "openai", "openrouter"].includes(p) && !key) {
    return wzErr("This provider needs an API key.");
  }
  const cfg = { provider: p, model };
  if (p === "anthropic") cfg.anthropic_api_key = key;
  else if (p === "openai") {
    cfg.openai_api_key = key;
    cfg.openai_base_url = base;
  } else if (p === "openrouter") cfg.openrouter_api_key = key;
  else if (p === "ollama") cfg.ollama_base_url = base;
  else if (p === "lm-studio") cfg.lm_studio_base_url = base;
  else if (p === "claude-code") cfg.claude_code_binary = $("wzBin").value.trim();

  $("wzNote").classList.remove("err");
  $("wzNote").textContent = "saving + restarting the local agent…";
  try {
    await invoke("save_provider_config", { cfg });
  } catch (e) {
    return wzErr("save failed: " + e);
  }
  store.local = true;
  const ok = await connect();
  if (ok) closeWizard();
  else
    wzErr(
      "Saved, but the agent isn't reachable yet — check the key/model, or that `aonyx` (built with --features api) is on your PATH.",
    );
}

$("wzProvider").addEventListener("change", () => {
  wzReflect();
  loadModels($("wzProvider").value, null);
});
$("wzModel").addEventListener("change", () => {
  showModelCustom($("wzModel").value === MODEL_CUSTOM);
});
$("wzModelRefresh").addEventListener("click", () =>
  loadModels($("wzProvider").value, selectedModel()),
);
// Once the key is entered, auto-load the list for key-gated providers.
$("wzKey").addEventListener("change", () => {
  if (LIST_NEEDS_KEY[$("wzProvider").value]) {
    loadModels($("wzProvider").value, selectedModel());
  }
});
$("wzSave").addEventListener("click", wzSave);
$("wzCancel").addEventListener("click", closeWizard);
$("cfgProviderBtn").addEventListener("click", () => {
  $("settings").classList.add("hidden");
  openWizard();
});

// ---- wiring ----
function reflectLocal() {
  const on = $("localMode").checked;
  $("apiUrl").disabled = on;
  $("token").disabled = on;
}

$("settingsBtn").addEventListener("click", () => {
  $("settings").classList.toggle("hidden");
  $("localMode").checked = store.local;
  $("apiUrl").value = store.url;
  $("token").value = store.token;
  reflectLocal();
});
$("localMode").addEventListener("change", reflectLocal);
$("saveBtn").addEventListener("click", async () => {
  store.local = $("localMode").checked;
  store.url = $("apiUrl").value.trim() || "http://127.0.0.1:8788";
  store.token = $("token").value;
  $("settings").classList.add("hidden");
  await connect();
});
// ---- updater ----
let pendingUpdate = null;
async function checkUpdate() {
  if (!invoke) return;
  const note = $("updateNote");
  const installBtn = $("installUpdateBtn");
  installBtn.classList.add("hidden");
  pendingUpdate = null;
  note.textContent = "checking…";
  try {
    const u = await invoke("check_for_update");
    if (u && u.version) {
      pendingUpdate = u;
      note.textContent = `v${u.version} available`;
      installBtn.classList.remove("hidden");
    } else {
      note.textContent = "up to date";
    }
  } catch (e) {
    note.textContent = "check failed: " + e;
  }
}
async function installUpdate() {
  if (!invoke || !pendingUpdate) return;
  const note = $("updateNote");
  const installBtn = $("installUpdateBtn");
  installBtn.disabled = true;
  note.textContent = "downloading & installing… the app will restart";
  try {
    await invoke("install_update"); // app re-execs into the new build on success
  } catch (e) {
    note.textContent = "update failed: " + e;
    installBtn.disabled = false;
  }
}
$("updateBtn").addEventListener("click", checkUpdate);
$("installUpdateBtn").addEventListener("click", installUpdate);

window.addEventListener("beforeunload", () => {
  try {
    if (store.local && invoke) invoke("stop_local");
  } catch {}
});
$("newBtn").addEventListener("click", newSession);
$("memq").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    searchMemory(e.target.value);
  }
});
sendBtn.addEventListener("click", send);
input.addEventListener("input", autoGrow);
input.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
  }
});

// First run: if the local agent can't be reached (no provider configured
// yet), open the provider wizard instead of leaving a bare error.
connect().then((ok) => {
  if (!ok && store.local) openWizard();
});
