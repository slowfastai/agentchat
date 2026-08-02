//! The browser chat surface.
//!
//! This page deliberately has no build step. It speaks the same JSON WebSocket
//! protocol as the mobile client, which keeps the browser a useful debugging
//! and day-to-day surface for the daemon instead of introducing a second API.

/// The Thread chat workspace, served at `/chat`.
pub const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AgentChat / Web</title>
<style>
  :root {
    color-scheme: dark;
    --bg: #101214;
    --surface: #171a1d;
    --surface-2: #1d2125;
    --surface-3: #252a2f;
    --line: #30363c;
    --line-soft: #24292e;
    --text: #eef0ed;
    --muted: #8e979e;
    --faint: #626b72;
    --accent: #c9ed73;
    --accent-ink: #161b12;
    --warm: #f2b477;
    --cyan: #84d6cf;
    --danger: #f48f83;
    --mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    --sans: Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  * { box-sizing: border-box; }
  html, body { height: 100%; }
  body {
    margin: 0;
    overflow: hidden;
    background: var(--bg);
    color: var(--text);
    font: 14px/1.45 var(--sans);
  }
  button, input, select, textarea { font: inherit; }
  button { color: inherit; }
  button:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .app-shell {
    display: grid;
    grid-template-columns: 258px minmax(0, 1fr) 306px;
    height: 100%;
  }
  .sidebar, .inspector { background: var(--surface); }
  .sidebar {
    display: flex;
    min-width: 0;
    flex-direction: column;
    border-right: 1px solid var(--line);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 20px 18px 18px;
    border-bottom: 1px solid var(--line-soft);
    font-weight: 700;
    letter-spacing: .01em;
  }
  .brand-mark {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 11px;
    font-weight: 900;
    letter-spacing: -.04em;
  }
  .brand small {
    display: block;
    margin-top: 1px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 500;
  }
  .side-heading, .inspector-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .side-heading {
    padding: 18px 14px 8px 18px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: .12em;
    text-transform: uppercase;
  }
  .icon-button, .quiet-button, .primary-button, .danger-button {
    border: 1px solid transparent;
    cursor: pointer;
    transition: background .16s ease, border-color .16s ease, color .16s ease, transform .16s ease;
  }
  .icon-button {
    display: grid;
    width: 28px;
    height: 28px;
    padding: 0;
    place-items: center;
    border-color: var(--line);
    background: transparent;
    color: var(--muted);
    font-size: 18px;
    line-height: 1;
  }
  .icon-button:hover, .quiet-button:hover { border-color: var(--muted); background: var(--surface-3); color: var(--text); }
  .thread-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 4px 8px 12px;
  }
  .thread-item {
    display: block;
    width: 100%;
    margin: 2px 0;
    padding: 10px 10px 9px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .thread-item:hover { background: var(--surface-2); }
  .thread-item.active { border-color: var(--line); background: var(--surface-2); }
  .thread-item-title {
    overflow: hidden;
    color: var(--text);
    font-size: 13px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .thread-item-meta {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 4px;
    color: var(--faint);
    font: 10px var(--mono);
  }
  .empty-list, .empty-copy { color: var(--muted); font-size: 12px; }
  .empty-list { padding: 18px 10px; }
  .sidebar-foot {
    padding: 13px 16px 16px;
    border-top: 1px solid var(--line-soft);
    color: var(--muted);
    font-size: 11px;
  }
  .connection {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 9px;
    color: var(--text);
  }
  .connection-dot { width: 7px; height: 7px; background: var(--warm); border-radius: 50%; }
  .connection-dot.online { background: var(--accent); }
  .connection-dot.error { background: var(--danger); }
  .sidebar-foot .path {
    overflow: hidden;
    margin-bottom: 12px;
    font: 10px/1.4 var(--mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sidebar-foot a { color: var(--cyan); text-decoration: none; }
  .sidebar-foot a:hover { text-decoration: underline; }
  .workspace {
    display: flex;
    min-width: 0;
    flex-direction: column;
    background: var(--bg);
  }
  .topbar {
    display: flex;
    min-height: 78px;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 14px 24px 13px;
    border-bottom: 1px solid var(--line);
  }
  .topbar-copy { min-width: 0; }
  .eyebrow {
    margin: 0 0 3px;
    color: var(--accent);
    font: 10px var(--mono);
    letter-spacing: .1em;
    text-transform: uppercase;
  }
  h1 {
    overflow: hidden;
    margin: 0;
    font-size: 19px;
    font-weight: 680;
    letter-spacing: -.01em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .topbar-meta { margin: 4px 0 0; color: var(--muted); font-size: 11px; }
  .topbar-actions { display: flex; flex: 0 0 auto; gap: 7px; }
  .quiet-button, .primary-button, .danger-button {
    min-height: 30px;
    padding: 5px 10px;
    border-color: var(--line);
    background: transparent;
    font-size: 12px;
  }
  .quiet-button { color: var(--muted); }
  .primary-button { border-color: var(--accent); background: var(--accent); color: var(--accent-ink); font-weight: 700; }
  .primary-button:hover { background: #d7f68d; transform: translateY(-1px); }
  .danger-button { border-color: #65403d; color: var(--danger); }
  .danger-button:hover { background: #392222; }
  .quiet-button:disabled, .primary-button:disabled, .danger-button:disabled { cursor: default; opacity: .45; transform: none; }
  .empty-state {
    display: grid;
    flex: 1;
    place-items: center;
    padding: 40px;
    text-align: center;
  }
  .empty-state-inner { max-width: 430px; }
  .empty-state .mark {
    display: inline-grid;
    width: 42px;
    height: 42px;
    margin-bottom: 18px;
    place-items: center;
    border: 1px solid var(--accent);
    color: var(--accent);
    font: 12px var(--mono);
  }
  .empty-state h2 { margin: 0 0 7px; font-size: 24px; letter-spacing: -.02em; }
  .empty-state p { margin: 0 0 20px; color: var(--muted); }
  .timeline {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 26px max(24px, calc((100% - 850px) / 2)) 28px;
  }
  .timeline-empty { padding: 34px 0; color: var(--muted); text-align: center; }
  .message {
    max-width: 780px;
    margin: 0 auto 25px;
  }
  .message-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 7px;
    color: var(--muted);
    font-size: 11px;
  }
  .message-name { color: var(--text); font-weight: 700; }
  .message-tag {
    padding: 1px 5px;
    border: 1px solid var(--line);
    color: var(--faint);
    font: 9px var(--mono);
    text-transform: uppercase;
  }
  .message-body {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: #e3e6e3;
    font-size: 14px;
  }
  .message.user { padding-left: 15px; border-left: 2px solid var(--warm); }
  .message.user .message-body { color: #f4dfc9; }
  .message.assistant { padding-left: 15px; border-left: 2px solid var(--cyan); }
  .message.assistant.streaming { border-left-color: var(--accent); }
  .message.assistant.failed { border-left-color: var(--danger); }
  .assistant-thinking, .assistant-tools, .assistant-plan {
    margin-bottom: 10px;
    border: 1px solid var(--line-soft);
    background: var(--surface);
    color: var(--muted);
    font-size: 12px;
  }
  .assistant-thinking summary, .assistant-plan summary { padding: 7px 9px; cursor: pointer; color: var(--faint); }
  .assistant-thinking .detail, .assistant-plan .detail { padding: 0 9px 9px; white-space: pre-wrap; }
  .assistant-tools { padding: 7px 9px; }
  .tool-row { display: flex; align-items: baseline; gap: 8px; padding: 2px 0; }
  .tool-status { color: var(--accent); font: 10px var(--mono); }
  .tool-status.failed { color: var(--danger); }
  .assistant-footer { margin-top: 8px; color: var(--faint); font: 10px var(--mono); }
  .composer-wrap {
    padding: 0 24px 20px;
    background: var(--bg);
  }
  .composer {
    max-width: 850px;
    margin: 0 auto;
    border: 1px solid var(--line);
    background: var(--surface);
  }
  .recipient-bar {
    display: flex;
    min-height: 35px;
    align-items: center;
    gap: 7px;
    overflow-x: auto;
    padding: 7px 10px 0;
    color: var(--muted);
    font-size: 11px;
    white-space: nowrap;
  }
  .recipient-label { margin-right: 2px; color: var(--faint); font: 10px var(--mono); text-transform: uppercase; }
  .recipient {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
  }
  .recipient input { width: 12px; height: 12px; accent-color: var(--accent); }
  .recipient.offline { opacity: .5; }
  .composer textarea {
    display: block;
    width: 100%;
    min-height: 68px;
    max-height: 210px;
    resize: vertical;
    padding: 9px 11px;
    border: 0;
    background: transparent;
    color: var(--text);
    line-height: 1.5;
  }
  .composer textarea::placeholder { color: var(--faint); }
  .composer-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 9px 9px;
    border-top: 1px solid var(--line-soft);
  }
  .composer-status { overflow: hidden; color: var(--faint); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .inspector {
    min-width: 0;
    overflow-y: auto;
    border-left: 1px solid var(--line);
  }
  .inspector-heading {
    padding: 20px 16px 15px;
    border-bottom: 1px solid var(--line);
  }
  .inspector-heading h2 { margin: 0; font-size: 13px; font-weight: 700; }
  .inspector-heading span { color: var(--muted); font: 10px var(--mono); }
  .participants { padding: 2px 0; }
  .participant {
    padding: 14px 16px 13px;
    border-bottom: 1px solid var(--line-soft);
  }
  .participant-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 8px; }
  .participant-identity { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .participant-avatar {
    display: grid;
    width: 25px;
    height: 25px;
    flex: 0 0 auto;
    place-items: center;
    border: 1px solid var(--line);
    color: var(--cyan);
    font: 10px var(--mono);
  }
  .participant-human .participant-avatar { color: var(--warm); }
  .participant-name { overflow: hidden; font-size: 12px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
  .participant-sub { margin-top: 2px; color: var(--faint); font: 10px var(--mono); }
  .participant-state { display: inline-flex; align-items: center; gap: 5px; color: var(--muted); font-size: 10px; }
  .state-dot { width: 6px; height: 6px; background: var(--faint); border-radius: 50%; }
  .state-dot.idle { background: var(--accent); }
  .state-dot.prompting { background: var(--warm); }
  .state-dot.offline, .state-dot.error { background: var(--danger); }
  .remove-participant { padding: 0 3px; border: 0; background: transparent; color: var(--faint); cursor: pointer; }
  .remove-participant:hover { color: var(--danger); }
  .setting { margin-top: 11px; }
  .setting label, .add-agent label, .dialog-field label { display: block; margin-bottom: 4px; color: var(--faint); font: 10px var(--mono); text-transform: uppercase; }
  .setting select, .setting input, .add-agent select, .dialog-field input {
    width: 100%;
    min-height: 30px;
    padding: 5px 7px;
    border: 1px solid var(--line);
    border-radius: 0;
    background: var(--surface-2);
    color: var(--text);
    font-size: 12px;
  }
  .setting select:hover, .setting input:hover, .add-agent select:hover, .dialog-field input:hover { border-color: var(--muted); }
  .setting-saving { margin-top: 4px; color: var(--accent); font-size: 10px; }
  .add-agent {
    padding: 16px;
    border-top: 1px solid var(--line);
  }
  .add-agent-row { display: flex; gap: 6px; }
  .add-agent-row select { min-width: 0; flex: 1; }
  .add-agent-row button { flex: 0 0 auto; }
  .inspector-note { padding: 15px 16px; color: var(--faint); font-size: 11px; }
  dialog {
    width: min(430px, calc(100vw - 32px));
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 0;
    background: var(--surface);
    color: var(--text);
  }
  dialog::backdrop { background: rgba(0, 0, 0, .65); }
  .dialog-inner { padding: 20px; }
  .dialog-inner h2 { margin: 0 0 4px; font-size: 16px; }
  .dialog-inner p { margin: 0 0 18px; color: var(--muted); font-size: 12px; }
  .dialog-field { margin-bottom: 13px; }
  .dialog-path-row { display: flex; gap: 8px; align-items: stretch; }
  .dialog-path-row input { min-width: 0; flex: 1; }
  .dialog-path-row button { white-space: nowrap; }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 7px; margin-top: 18px; }
  .toast {
    position: fixed;
    right: 18px;
    bottom: 18px;
    max-width: min(420px, calc(100vw - 36px));
    padding: 10px 12px;
    border: 1px solid var(--danger);
    background: #2b1d1d;
    color: #ffd6d1;
    font-size: 12px;
    opacity: 0;
    pointer-events: none;
    transform: translateY(8px);
    transition: opacity .18s ease, transform .18s ease;
  }
  .toast.visible { opacity: 1; transform: translateY(0); }
  .hidden { display: none !important; }
  @media (max-width: 1100px) {
    .app-shell { grid-template-columns: 220px minmax(0, 1fr); }
    .inspector { grid-column: 1 / -1; display: none; }
    .workspace { min-height: 0; }
  }
  @media (max-width: 720px) {
    body { overflow: auto; }
    .app-shell { display: block; height: auto; min-height: 100%; }
    .sidebar { min-height: 0; border-right: 0; border-bottom: 1px solid var(--line); }
    .brand { padding: 13px 14px; }
    .side-heading { padding-top: 11px; }
    .thread-list { max-height: 145px; }
    .sidebar-foot { display: none; }
    .workspace { min-height: calc(100vh - 230px); }
    .topbar { padding: 13px 15px; }
    .topbar-actions .quiet-button:first-child { display: none; }
    .timeline { padding: 20px 15px; }
    .composer-wrap { padding: 0 15px 15px; }
  }
</style>
</head>
<body>
<div class="app-shell">
  <aside class="sidebar">
    <div class="brand">
      <span class="brand-mark">AC</span>
      <div>AgentChat<small>Web workspace</small></div>
    </div>
    <div class="side-heading">
      <span>Threads</span>
      <button class="icon-button" id="newThreadButton" type="button" title="New thread" aria-label="New thread">+</button>
    </div>
    <div class="thread-list" id="threadList"></div>
    <div class="sidebar-foot">
      <div class="connection"><span class="connection-dot" id="connectionDot"></span><span id="connectionLabel">Connecting</span></div>
      <div class="path" id="socketLabel">ws://127.0.0.1:9390</div>
      <a href="/">Open run console</a>
    </div>
  </aside>

  <main class="workspace">
    <header class="topbar">
      <div class="topbar-copy">
        <p class="eyebrow" id="threadEyebrow">No thread selected</p>
        <h1 id="threadTitle">Start a conversation</h1>
        <p class="topbar-meta" id="threadMeta">Create a thread, then attach one or more agents.</p>
      </div>
      <div class="topbar-actions">
        <button class="quiet-button" id="refreshButton" type="button" title="Refresh threads">Refresh</button>
        <button class="danger-button hidden" id="stopButton" type="button">Stop</button>
        <button class="quiet-button hidden" id="closeThreadButton" type="button">Close</button>
      </div>
    </header>

    <section class="empty-state" id="emptyState">
      <div class="empty-state-inner">
        <div class="mark">CHAT</div>
        <h2>Talk to your agents.</h2>
        <p>Keep a focused thread, route a message to one or several agents, and change each participant's model before the next turn.</p>
        <button class="primary-button" id="emptyNewThreadButton" type="button">New thread</button>
      </div>
    </section>

    <section class="timeline hidden" id="timeline"></section>

    <div class="composer-wrap hidden" id="composerWrap">
      <div class="composer">
        <div class="recipient-bar" id="recipientBar"></div>
        <textarea id="messageInput" rows="3" placeholder="Message the selected agents..." spellcheck="true"></textarea>
        <div class="composer-foot">
          <span class="composer-status" id="composerStatus">Select a recipient</span>
          <button class="primary-button" id="sendButton" type="button">Send</button>
        </div>
      </div>
    </div>
  </main>

  <aside class="inspector">
    <div class="inspector-heading">
      <div><h2>Participants</h2><span id="participantCount">0 connected</span></div>
    </div>
    <div class="participants" id="participants"></div>
    <div class="add-agent">
      <label for="agentPicker">Add agent</label>
      <div class="add-agent-row">
        <select id="agentPicker"><option value="">Choose an agent</option></select>
        <button class="quiet-button" id="addAgentButton" type="button">Attach</button>
      </div>
    </div>
    <div class="inspector-note">Settings apply to the next turn. A running participant must finish before its configuration can change.</div>
  </aside>
</div>

<dialog id="newThreadDialog">
  <form class="dialog-inner" id="newThreadForm" method="dialog">
    <h2>New thread</h2>
    <p>Create a private workspace for a conversation with one or more agents.</p>
    <div class="dialog-field">
      <label for="newThreadTitle">Title</label>
      <input id="newThreadTitle" type="text" placeholder="Architecture review" maxlength="120">
    </div>
    <div class="dialog-field">
      <label for="newThreadWorkingDir">Working directory</label>
      <div class="dialog-path-row">
        <input id="newThreadWorkingDir" type="text" value="." spellcheck="false">
        <button class="quiet-button" id="chooseWorkingDirButton" type="button">Choose folder</button>
      </div>
    </div>
    <div class="dialog-actions">
      <button class="quiet-button" id="cancelNewThreadButton" value="cancel" type="button">Cancel</button>
      <button class="primary-button" value="default" type="submit">Create thread</button>
    </div>
  </form>
</dialog>
<div class="toast" id="toast" role="status"></div>

<script>
const $ = (id) => document.getElementById(id);
const escapeHtml = (value) => String(value ?? "").replace(/[&<>\"]/g, (char) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;"
}[char]));
const state = {
  socket: null,
  reconnectTimer: null,
  agents: [],
  threads: new Map(),
  snapshots: new Map(),
  messages: new Map(),
  targets: new Map(),
  currentThreadId: null,
  attached: new Set(),
};

let toastTimer = null;
const lastWorkingDirectoryKey = "agentchat_last_working_directory";

function socketUrl() {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const host = location.hostname || "127.0.0.1";
  return `${scheme}://${host}:9390`;
}

function showToast(message) {
  $("toast").textContent = message;
  $("toast").classList.add("visible");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => $("toast").classList.remove("visible"), 4800);
}

async function chooseWorkingDirectory() {
  const button = $("chooseWorkingDirButton");
  button.disabled = true;
  button.textContent = "Opening...";
  try {
    const response = await fetch("/api/select-working-directory", { method: "POST" });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || "Could not open the folder picker.");
    if (result.path) {
      $("newThreadWorkingDir").value = result.path;
      localStorage.setItem(lastWorkingDirectoryKey, result.path);
    }
  } catch (error) {
    showToast(error.message || "Could not open the folder picker.");
  } finally {
    button.disabled = false;
    button.textContent = "Choose folder";
  }
}

function send(message) {
  if (!state.socket || state.socket.readyState !== WebSocket.OPEN) {
    showToast("The daemon is not connected.");
    return false;
  }
  state.socket.send(JSON.stringify(message));
  return true;
}

function setConnection(status, kind) {
  $("connectionLabel").textContent = status;
  $("connectionDot").className = `connection-dot ${kind || ""}`;
}

function connect() {
  clearTimeout(state.reconnectTimer);
  setConnection("Connecting", "");
  const socket = new WebSocket(socketUrl());
  state.socket = socket;
  $("socketLabel").textContent = socketUrl();
  socket.addEventListener("open", () => {
    setConnection("Connected", "online");
    send({ type: "list_agents" });
    send({ type: "list_threads" });
    renderAll();
  });
  socket.addEventListener("message", (event) => {
    try {
      handleEvent(JSON.parse(event.data));
    } catch (error) {
      showToast(`Invalid daemon event: ${error.message}`);
    }
  });
  socket.addEventListener("close", () => {
    if (state.socket !== socket) return;
    setConnection("Reconnecting", "error");
    renderAll();
    state.reconnectTimer = setTimeout(connect, 1800);
  });
  socket.addEventListener("error", () => setConnection("Connection error", "error"));
}

function ensureSnapshot(threadId) {
  if (!state.snapshots.has(threadId)) {
    state.snapshots.set(threadId, {
      thread_id: threadId,
      title: null,
      working_dir: ".",
      created_at_ms: Date.now(),
      last_thread_seq: 0,
      participants: [],
    });
  }
  return state.snapshots.get(threadId);
}

function ensureMessages(threadId) {
  if (!state.messages.has(threadId)) state.messages.set(threadId, []);
  return state.messages.get(threadId);
}

function ensureTargets(threadId) {
  if (!state.targets.has(threadId)) {
    const snapshot = ensureSnapshot(threadId);
    state.targets.set(threadId, new Set(snapshot.participants.filter((p) => p.kind === "agent").map((p) => p.participant_id)));
  }
  return state.targets.get(threadId);
}

function currentSnapshot() {
  return state.currentThreadId ? state.snapshots.get(state.currentThreadId) : null;
}

function titleFor(thread) {
  return thread && thread.title ? thread.title : "Untitled thread";
}

function shortId(id) {
  if (!id) return "";
  return id.length > 18 ? `${id.slice(0, 8)}...${id.slice(-5)}` : id;
}

function formatTime(timestamp) {
  if (!timestamp) return "";
  return new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function renderThreadList() {
  const items = Array.from(state.threads.values()).sort((left, right) => left.created_at_ms - right.created_at_ms);
  if (!items.length) {
    $("threadList").innerHTML = '<div class="empty-list">No threads yet.</div>';
    return;
  }
  $("threadList").innerHTML = items.map((thread) => `
    <button class="thread-item ${thread.thread_id === state.currentThreadId ? "active" : ""}" data-thread-id="${escapeHtml(thread.thread_id)}" type="button">
      <div class="thread-item-title">${escapeHtml(titleFor(thread))}</div>
      <div class="thread-item-meta"><span>${thread.participant_count || 0} participants</span><span>${escapeHtml(thread.state || "idle")}</span></div>
    </button>`).join("");
  document.querySelectorAll("[data-thread-id]").forEach((element) => {
    element.addEventListener("click", () => selectThread(element.dataset.threadId));
  });
}

function renderHeader() {
  const snapshot = currentSnapshot();
  const summary = state.currentThreadId ? state.threads.get(state.currentThreadId) : null;
  const hasThread = Boolean(state.currentThreadId && snapshot);
  $("emptyState").classList.toggle("hidden", hasThread);
  $("timeline").classList.toggle("hidden", !hasThread);
  $("composerWrap").classList.toggle("hidden", !hasThread);
  $("closeThreadButton").classList.toggle("hidden", !hasThread);
  if (!hasThread) {
    $("threadEyebrow").textContent = "No thread selected";
    $("threadTitle").textContent = "Start a conversation";
    $("threadMeta").textContent = "Create a thread, then attach one or more agents.";
    return;
  }
  $("threadEyebrow").textContent = snapshot.last_thread_seq ? `Thread / ${shortId(snapshot.thread_id)}` : "New thread";
  $("threadTitle").textContent = titleFor(snapshot);
  $("threadMeta").textContent = `${snapshot.working_dir || "."}  /  ${snapshot.participants.filter((p) => p.kind === "agent").length} agents`;
}

function participantStatus(participant) {
  const messages = ensureMessages(state.currentThreadId || "");
  const streaming = messages.some((message) => message.kind === "assistant" && message.participant_id === participant.participant_id && message.state === "streaming");
  if (streaming) return "prompting";
  return participant.state || "idle";
}

function agentSummary(agentId) {
  return state.agents.find((agent) => agent.agent_id === agentId);
}

function settingDescriptor(participant, settingId) {
  const summary = agentSummary(participant.agent_id);
  return summary && (summary.settings || []).find((setting) => setting.id === settingId);
}

function settingMarkup(participant, settingId, label, key) {
  const descriptor = settingDescriptor(participant, settingId);
  if (!descriptor) return "";
  const current = participant.settings && participant.settings[key] ? participant.settings[key] : (descriptor.current_value || "");
  const values = Array.isArray(descriptor.values) ? descriptor.values.slice() : [];
  if (current && !values.some((value) => value.id === current)) values.unshift({ id: current, label: current });
  const options = `<option value="">Default</option>${values.map((value) => `<option value="${escapeHtml(value.id)}" ${value.id === current ? "selected" : ""}>${escapeHtml(value.label)}</option>`).join("")}`;
  return `<div class="setting"><label>${escapeHtml(label)}</label><select data-setting="${escapeHtml(settingId)}" data-participant="${escapeHtml(participant.participant_id)}">${options}</select></div>`;
}

function renderParticipants() {
  const snapshot = currentSnapshot();
  const participants = snapshot ? snapshot.participants : [];
  $("participantCount").textContent = `${participants.filter((p) => p.kind === "agent").length} connected`;
  if (!snapshot) {
    $("participants").innerHTML = '<div class="inspector-note">Select a thread to manage its agents.</div>';
    $("agentPicker").innerHTML = '<option value="">Choose an agent</option>';
    return;
  }
  $("participants").innerHTML = participants.map((participant) => {
    const isHuman = participant.kind === "human";
    const status = participantStatus(participant);
    const label = participant.agent_id ? `@${participant.mention_handle || participant.agent_id}` : "thread owner";
    const controls = isHuman ? "" : `
      ${settingMarkup(participant, "model", "Model", "model")}
      ${settingMarkup(participant, "reasoning_effort", "Reasoning effort", "reasoning_effort")}`;
    return `<div class="participant ${isHuman ? "participant-human" : ""}">
      <div class="participant-head">
        <div class="participant-identity">
          <span class="participant-avatar">${isHuman ? "YOU" : "AI"}</span>
          <div><div class="participant-name">${escapeHtml(participant.display_name)}</div><div class="participant-sub">${escapeHtml(label)}</div></div>
        </div>
        <div class="participant-state"><span class="state-dot ${escapeHtml(status)}"></span>${escapeHtml(status)}</div>
      </div>
      ${isHuman ? "" : `<button class="remove-participant" data-remove-participant="${escapeHtml(participant.participant_id)}" type="button" title="Remove participant" aria-label="Remove ${escapeHtml(participant.display_name)}">x remove</button>${controls}`}
    </div>`;
  }).join("") || '<div class="inspector-note">No participants yet.</div>';

  document.querySelectorAll("[data-setting]").forEach((element) => {
    element.addEventListener("change", () => updateSetting(element.dataset.participant, element.dataset.setting, element.value));
  });
  document.querySelectorAll("[data-remove-participant]").forEach((element) => {
    element.addEventListener("click", () => {
      if (confirm("Remove this participant and close its session?")) {
        send({ type: "remove_thread_participant", thread_id: state.currentThreadId, participant_id: element.dataset.removeParticipant });
      }
    });
  });
  const selectedAgentIds = new Set(participants.filter((p) => p.kind === "agent").map((p) => p.agent_id));
  $("agentPicker").innerHTML = '<option value="">Choose an agent</option>' + state.agents
    .filter((agent) => agent.status === "online" && !selectedAgentIds.has(agent.agent_id))
    .map((agent) => `<option value="${escapeHtml(agent.agent_id)}">${escapeHtml(agent.name)}</option>`).join("");
}

function renderRecipients() {
  const snapshot = currentSnapshot();
  if (!snapshot) {
    $("recipientBar").innerHTML = "";
    $("composerStatus").textContent = "Select a thread";
    return;
  }
  const agents = snapshot.participants.filter((participant) => participant.kind === "agent");
  const targets = ensureTargets(snapshot.thread_id);
  $("recipientBar").innerHTML = `<span class="recipient-label">Route to</span>` + (agents.length ? agents.map((participant) => `
    <label class="recipient ${participant.state === "offline" ? "offline" : ""}"><input type="checkbox" data-recipient="${escapeHtml(participant.participant_id)}" ${targets.has(participant.participant_id) ? "checked" : ""}><span>${escapeHtml(participant.display_name)}</span></label>`).join("") : '<span class="empty-copy">Attach an agent to start.</span>');
  document.querySelectorAll("[data-recipient]").forEach((element) => {
    element.addEventListener("change", () => {
      if (element.checked) targets.add(element.dataset.recipient);
      else targets.delete(element.dataset.recipient);
      renderComposerStatus();
    });
  });
  renderComposerStatus();
}

function renderComposerStatus() {
  const snapshot = currentSnapshot();
  if (!snapshot) return;
  const agents = snapshot.participants.filter((participant) => participant.kind === "agent");
  const selected = agents.filter((participant) => ensureTargets(snapshot.thread_id).has(participant.participant_id));
  $("composerStatus").textContent = selected.length ? `${selected.length} recipient${selected.length === 1 ? "" : "s"} selected` : "Select a recipient";
  $("sendButton").disabled = !selected.length || !state.socket || state.socket.readyState !== WebSocket.OPEN;
}

function renderTimeline() {
  const threadId = state.currentThreadId;
  const timeline = $("timeline");
  if (!threadId) return;
  const shouldStick = timeline.scrollHeight - timeline.scrollTop - timeline.clientHeight < 100;
  const messages = ensureMessages(threadId);
  if (!messages.length) {
    timeline.innerHTML = '<div class="timeline-empty">No messages yet. Select agents above and send the first message.</div>';
  } else {
    timeline.innerHTML = messages.map(renderMessage).join("");
  }
  if (shouldStick || messages.length < 2) timeline.scrollTop = timeline.scrollHeight;
}

function renderMessage(message) {
  if (message.kind === "user") {
    return `<article class="message user"><div class="message-head"><span class="message-name">${escapeHtml(message.sender && message.sender.display_name || "You")}</span><span class="message-tag">user</span><span>${escapeHtml(formatTime(message.created_at || Date.now()))}</span></div><div class="message-body">${escapeHtml(message.content)}</div></article>`;
  }
  const status = message.state || "streaming";
  const thinking = message.thinking ? `<details class="assistant-thinking"><summary>Reasoning</summary><div class="detail">${escapeHtml(message.thinking)}</div></details>` : "";
  const tools = message.tools && message.tools.length ? `<div class="assistant-tools">${message.tools.map((tool) => `<div class="tool-row"><span class="tool-status ${tool.status === "failed" ? "failed" : ""}">${escapeHtml(tool.status || "working")}</span><span>${escapeHtml(tool.title)}</span></div>`).join("")}</div>` : "";
  const plan = message.plan ? `<details class="assistant-plan"><summary>Plan update</summary><div class="detail">${escapeHtml(JSON.stringify(message.plan, null, 2))}</div></details>` : "";
  const body = message.response ? `<div class="message-body">${escapeHtml(message.response)}</div>` : (status === "streaming" ? '<div class="message-body">Working...</div>' : "");
  const footer = status === "streaming" ? "streaming" : (message.stop_reason || status);
  return `<article class="message assistant ${escapeHtml(status)}"><div class="message-head"><span class="message-name">${escapeHtml(message.display_name || message.agent_id || "Agent")}</span><span class="message-tag">agent</span><span>${escapeHtml(formatTime(message.created_at || Date.now()))}</span></div>${thinking}${tools}${plan}${body}<div class="assistant-footer">${escapeHtml(footer)}</div></article>`;
}

function assistantMessage(event) {
  const messages = ensureMessages(event.thread_id);
  const key = `${event.participant_id}:${event.turn_id}`;
  let message = messages.find((item) => item.kind === "assistant" && item.key === key);
  if (!message) {
    const participant = ensureSnapshot(event.thread_id).participants.find((item) => item.participant_id === event.participant_id);
    message = { kind: "assistant", key, participant_id: event.participant_id, agent_id: event.agent_id, display_name: participant ? participant.display_name : event.agent_id, thinking: "", response: "", tools: [], state: "streaming", created_at: Date.now() };
    messages.push(message);
  }
  return message;
}

function updateSetting(participantId, settingId, value) {
  const snapshot = currentSnapshot();
  const participant = snapshot && snapshot.participants.find((item) => item.participant_id === participantId);
  if (!snapshot || !participant) return;
  const settings = { ...(participant.settings || {}) };
  settings[settingId === "reasoning_effort" ? "reasoning_effort" : "model"] = value || null;
  if (send({ type: "set_thread_participant_settings", thread_id: snapshot.thread_id, participant_id: participantId, settings })) {
    $("composerStatus").textContent = "Saving participant settings...";
  }
}

function selectThread(threadId) {
  if (!state.threads.has(threadId)) return;
  state.currentThreadId = threadId;
  state.attached.delete(threadId);
  state.messages.set(threadId, []);
  ensureTargets(threadId).clear();
  renderAll();
  send({ type: "attach_thread", thread_id: threadId, after_seq: 0 });
}

function handleEvent(event) {
  switch (event.type) {
    case "agent_list":
      state.agents = event.agents || [];
      renderParticipants();
      break;
    case "thread_list":
      state.threads = new Map((event.threads || []).map((thread) => [thread.thread_id, thread]));
      renderThreadList();
      if (!state.currentThreadId && state.threads.size) selectThread(state.threads.keys().next().value);
      break;
    case "thread_created":
      state.threads.set(event.thread_id, { thread_id: event.thread_id, title: null, working_dir: ".", created_at_ms: event.created_at_ms, state: "idle", participant_count: 1, last_thread_seq: 0 });
      ensureSnapshot(event.thread_id);
      if (state.currentThreadId !== event.thread_id) selectThread(event.thread_id);
      renderThreadList();
      break;
    case "thread_attached":
      break;
    case "thread_snapshot":
      state.snapshots.set(event.snapshot.thread_id, event.snapshot);
      ensureTargets(event.snapshot.thread_id);
      updateThreadSummary(event.snapshot.thread_id, {
        title: event.snapshot.title,
        working_dir: event.snapshot.working_dir,
        participant_count: event.snapshot.participants.length,
        last_thread_seq: event.snapshot.last_thread_seq,
      });
      if (state.currentThreadId === event.snapshot.thread_id) renderAll();
      break;
    case "thread_replay_complete":
      state.attached.add(event.thread_id);
      if (state.threads.has(event.thread_id)) state.threads.get(event.thread_id).last_thread_seq = event.last_thread_seq;
      if (state.currentThreadId === event.thread_id) renderAll();
      break;
    case "thread_participant_added":
      applyParticipant(event.thread_id, event.participant);
      ensureTargets(event.thread_id).add(event.participant.participant_id);
      updateThreadSummary(event.thread_id, { participant_count: ensureSnapshot(event.thread_id).participants.length, last_thread_seq: event.thread_seq });
      if (state.currentThreadId === event.thread_id) renderAll();
      break;
    case "thread_participant_settings_updated":
      applyParticipant(event.thread_id, event.participant);
      updateThreadSummary(event.thread_id, { last_thread_seq: event.thread_seq });
      if (state.currentThreadId === event.thread_id) {
        renderAll();
        $("composerStatus").textContent = "Participant settings saved";
      }
      break;
    case "thread_participant_removed":
      {
        const snapshot = ensureSnapshot(event.thread_id);
        snapshot.participants = snapshot.participants.filter((participant) => participant.participant_id !== event.participant_id);
        ensureTargets(event.thread_id).delete(event.participant_id);
        updateThreadSummary(event.thread_id, { participant_count: snapshot.participants.length, last_thread_seq: event.thread_seq });
        if (state.currentThreadId === event.thread_id) renderAll();
      }
      break;
    case "thread_message":
      addUserMessage(event);
      updateThreadSummary(event.thread_id, { last_thread_seq: event.thread_seq });
      break;
    case "thread_assistant_message":
      {
        const message = assistantMessage(event);
        message.message_id = event.message_id;
        message.thinking = event.thinking || "";
        message.response = event.response || "";
        message.state = event.state || "streaming";
        message.stop_reason = event.stop_reason || "";
        message.created_at = message.created_at || Date.now();
        updateThreadSummary(event.thread_id, { last_thread_seq: event.thread_seq, state: message.state === "streaming" ? "prompting" : "idle" });
        if (state.currentThreadId === event.thread_id) renderAll();
      }
      break;
    case "thread_agent_delta":
      {
        const message = assistantMessage(event);
        if (event.delta_type === "thinking") message.thinking += event.content || "";
        else if (event.delta_type === "text") message.response += event.content || "";
        message.state = "streaming";
        updateThreadSummary(event.thread_id, { last_thread_seq: event.thread_seq, state: "prompting" });
        if (state.currentThreadId === event.thread_id) renderAll();
      }
      break;
    case "thread_agent_tool_update":
      {
        const message = assistantMessage(event);
        const existing = message.tools.find((tool) => tool.tool_call_id === event.tool_call_id);
        if (existing) Object.assign(existing, event);
        else message.tools.push({ tool_call_id: event.tool_call_id, title: event.title, status: event.status, content: event.content });
        if (state.currentThreadId === event.thread_id) renderAll();
      }
      break;
    case "thread_agent_plan_update":
      assistantMessage(event).plan = event.plan_json;
      if (state.currentThreadId === event.thread_id) renderAll();
      break;
    case "thread_agent_turn_end":
      {
        const message = assistantMessage(event);
        message.state = "completed";
        message.stop_reason = event.stop_reason;
        updateThreadSummary(event.thread_id, { last_thread_seq: event.thread_seq, state: "idle" });
        if (state.currentThreadId === event.thread_id) renderAll();
      }
      break;
    case "thread_closed":
      state.threads.delete(event.thread_id);
      state.snapshots.delete(event.thread_id);
      state.messages.delete(event.thread_id);
      state.targets.delete(event.thread_id);
      if (state.currentThreadId === event.thread_id) {
        state.currentThreadId = null;
        renderAll();
      }
      renderThreadList();
      break;
    case "error":
      showToast(event.message || "The daemon returned an error.");
      break;
    case "daemon_status":
      showToast(event.message || "The daemon is stopping.");
      break;
  }
}

function applyParticipant(threadId, participant) {
  const snapshot = ensureSnapshot(threadId);
  const index = snapshot.participants.findIndex((item) => item.participant_id === participant.participant_id);
  if (index >= 0) snapshot.participants[index] = participant;
  else snapshot.participants.push(participant);
}

function addUserMessage(event) {
  const messages = ensureMessages(event.thread_id);
  if (!messages.some((message) => message.kind === "user" && message.message_id === event.message_id)) {
    messages.push({ kind: "user", message_id: event.message_id, sender: event.sender, content: event.content, created_at: Date.now() });
  }
  if (state.currentThreadId === event.thread_id) renderAll();
}

function updateThreadSummary(threadId, updates) {
  const summary = state.threads.get(threadId);
  if (summary) Object.assign(summary, updates);
  if (state.currentThreadId === threadId) renderHeader();
  renderThreadList();
}

function renderAll() {
  renderHeader();
  renderThreadList();
  renderParticipants();
  renderRecipients();
  renderTimeline();
  const snapshot = currentSnapshot();
  const messages = snapshot ? ensureMessages(snapshot.thread_id) : [];
  const busy = messages.some((message) => message.kind === "assistant" && message.state === "streaming");
  $("stopButton").classList.toggle("hidden", !busy);
  $("refreshButton").disabled = !state.socket || state.socket.readyState !== WebSocket.OPEN;
}

function openNewThreadDialog() {
  $("newThreadTitle").value = "";
  $("newThreadWorkingDir").value = localStorage.getItem(lastWorkingDirectoryKey) || ".";
  const dialog = $("newThreadDialog");
  if (typeof dialog.showModal === "function") dialog.showModal();
  else dialog.setAttribute("open", "open");
  setTimeout(() => $("newThreadTitle").focus(), 0);
}

$("newThreadButton").addEventListener("click", openNewThreadDialog);
$("emptyNewThreadButton").addEventListener("click", openNewThreadDialog);
$("cancelNewThreadButton").addEventListener("click", () => $("newThreadDialog").close());
$("chooseWorkingDirButton").addEventListener("click", chooseWorkingDirectory);
$("newThreadForm").addEventListener("submit", (event) => {
  event.preventDefault();
  const workingDir = $("newThreadWorkingDir").value.trim() || ".";
  const title = $("newThreadTitle").value.trim() || null;
  localStorage.setItem(lastWorkingDirectoryKey, workingDir);
  if (send({ type: "create_thread", title, working_dir: workingDir })) $("newThreadDialog").close();
});
$("refreshButton").addEventListener("click", () => {
  send({ type: "list_agents" });
  send({ type: "list_threads" });
});
$("addAgentButton").addEventListener("click", () => {
  const agentId = $("agentPicker").value;
  if (!state.currentThreadId || !agentId) return;
  send({ type: "add_thread_participant", thread_id: state.currentThreadId, agent_id: agentId });
  $("agentPicker").value = "";
});
$("sendButton").addEventListener("click", sendMessage);
$("messageInput").addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    sendMessage();
  }
});
$("stopButton").addEventListener("click", () => {
  const snapshot = currentSnapshot();
  if (!snapshot) return;
  snapshot.participants.filter((participant) => participant.kind === "agent" && participant.session_id && participantStatus(participant) === "prompting").forEach((participant) => {
    send({ type: "cancel", session_id: participant.session_id });
  });
});
$("closeThreadButton").addEventListener("click", () => {
  if (state.currentThreadId && confirm("Close this thread and its agent sessions?")) send({ type: "close_thread", thread_id: state.currentThreadId });
});

function sendMessage() {
  const content = $("messageInput").value.trim();
  const snapshot = currentSnapshot();
  if (!content || !snapshot) return;
  const targets = Array.from(ensureTargets(snapshot.thread_id));
  if (!targets.length) {
    showToast("Select at least one agent.");
    return;
  }
  if (send({ type: "send_thread_message", thread_id: snapshot.thread_id, content, target_participant_ids: targets })) {
    $("messageInput").value = "";
    $("messageInput").focus();
  }
}

renderAll();
connect();
</script>
</body>
</html>"##;
