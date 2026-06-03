// Aonyx Desktop — talks to `aonyx serve api` through Rust-side Tauri
// commands (no CORS, sandboxed). Blocking turns for V4.6; streaming in V4.7.

const tauri = window.__TAURI__;
const invoke = tauri && tauri.core && tauri.core.invoke;

const $ = (id) => document.getElementById(id);
const log = $("log");
const input = $("input");
const sendBtn = $("send");
const dot = $("dot");
const statusText = $("statusText");

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
};

let sessionId = null;
let busy = false;

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

function clearEmpty() {
  const e = $("empty");
  if (e) e.remove();
}

function addMsg(role, text, opts = {}) {
  clearEmpty();
  const wrap = document.createElement("div");
  wrap.className = "msg " + role;

  const r = document.createElement("div");
  r.className = "role";
  r.textContent = role === "user" ? "you" : "aonyx";
  wrap.appendChild(r);

  const b = document.createElement("div");
  b.className = "bubble" + (opts.error ? " error" : "") + (opts.thinking ? " thinking" : "");
  b.innerHTML = escapeHtml(text);
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

async function connect() {
  if (!invoke) {
    setStatus("err", "not running in Tauri");
    return false;
  }
  setStatus("", "connecting…");
  try {
    const info = await invoke("api_info", { base: store.url, token: store.token });
    setStatus("ok", `${info.provider} · ${info.model}`);
    // fresh session for the (possibly new) endpoint
    const rec = await invoke("api_create_session", {
      base: store.url,
      token: store.token,
      project: null,
    });
    sessionId = rec.id;
    return true;
  } catch (e) {
    setStatus("err", String(e));
    return false;
  }
}

async function send() {
  const text = input.value.trim();
  if (!text || busy) return;
  if (!invoke) {
    addMsg("assistant", "This build is not running inside Tauri.", { error: true });
    return;
  }
  if (!sessionId && !(await connect())) {
    addMsg("assistant", "Not connected — check the API URL/token in Settings.", {
      error: true,
    });
    return;
  }

  addMsg("user", text);
  input.value = "";
  autoGrow();
  busy = true;
  sendBtn.disabled = true;

  // Assistant bubble we stream tokens into.
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
        if (!acc && frame.reply) bubble.textContent = frame.reply;
        renderTools();
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
    await invoke("api_stream", {
      base: store.url,
      token: store.token,
      session: sessionId,
      content: text,
      onEvent: channel,
    });
    if (bubble.classList.contains("thinking")) {
      // Stream ended with no delta/done (e.g. empty reply).
      bubble.classList.remove("thinking");
      bubble.textContent = acc || "(no reply)";
    }
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

function autoGrow() {
  input.style.height = "auto";
  input.style.height = Math.min(input.scrollHeight, 144) + "px";
}

// ---- wiring ----
$("settingsBtn").addEventListener("click", () => {
  $("settings").classList.toggle("hidden");
  $("apiUrl").value = store.url;
  $("token").value = store.token;
});
$("saveBtn").addEventListener("click", async () => {
  store.url = $("apiUrl").value.trim() || "http://127.0.0.1:8788";
  store.token = $("token").value;
  $("settings").classList.add("hidden");
  sessionId = null;
  await connect();
});
sendBtn.addEventListener("click", send);
input.addEventListener("input", autoGrow);
input.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
  }
});

// initial connection attempt against the saved/default endpoint
connect();
