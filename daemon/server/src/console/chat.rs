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
  .empty-list { color: var(--muted); font-size: 12px; }
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
    min-height: 0;
    flex-direction: column;
    overflow: hidden;
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
  .message-avatar {
    display: inline-grid;
    width: 20px;
    height: 20px;
    place-items: center;
    border: 1px solid var(--line);
    color: var(--cyan);
    font: 9px var(--mono);
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
  .message-body.markdown {
    white-space: normal;
    line-height: 1.62;
  }
  .message-body.markdown > :first-child { margin-top: 0; }
  .message-body.markdown > :last-child { margin-bottom: 0; }
  .message-body.markdown p { margin: 0 0 12px; }
  .message-body.markdown h1,
  .message-body.markdown h2,
  .message-body.markdown h3,
  .message-body.markdown h4,
  .message-body.markdown h5,
  .message-body.markdown h6 {
    margin: 18px 0 8px;
    color: var(--text);
    line-height: 1.25;
  }
  .message-body.markdown h1 { font-size: 21px; }
  .message-body.markdown h2 { font-size: 18px; }
  .message-body.markdown h3 { font-size: 16px; }
  .message-body.markdown h4,
  .message-body.markdown h5,
  .message-body.markdown h6 { font-size: 14px; }
  .message-body.markdown ul,
  .message-body.markdown ol { margin: 6px 0 14px; padding-left: 1.45em; }
  .message-body.markdown li + li { margin-top: 4px; }
  .message-body.markdown blockquote {
    margin: 12px 0;
    padding: 1px 0 1px 14px;
    border-left: 2px solid var(--line);
    color: var(--muted);
  }
  .message-body.markdown blockquote > :last-child { margin-bottom: 0; }
  .message-body.markdown code {
    padding: 2px 5px;
    background: var(--surface-3);
    color: var(--accent);
    font: .9em/1.4 var(--mono);
  }
  .message-body.markdown pre {
    max-width: 100%;
    margin: 12px 0;
    overflow: auto;
    padding: 12px 14px;
    border: 1px solid var(--line);
    background: #111416;
  }
  .message-body.markdown pre code {
    display: block;
    padding: 0;
    background: transparent;
    color: #d9e3d7;
    white-space: pre;
    font-size: 12px;
  }
  .message-body.markdown a { color: var(--cyan); text-underline-offset: 2px; }
  .message-body.markdown img {
    display: block;
    max-width: 100%;
    height: auto;
    margin: 10px 0;
    border: 1px solid var(--line);
  }
  .message-body.markdown table {
    width: 100%;
    margin: 12px 0;
    border-collapse: collapse;
    font-size: 13px;
  }
  .message-body.markdown th,
  .message-body.markdown td {
    padding: 7px 9px;
    border-bottom: 1px solid var(--line);
    text-align: left;
    vertical-align: top;
  }
  .message-body.markdown th { color: var(--text); font-weight: 700; }
  .message-body.markdown hr { margin: 18px 0; border: 0; border-top: 1px solid var(--line); }
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
    position: relative;
    max-width: 850px;
    margin: 0 auto;
    border: 1px solid var(--line);
    background: var(--surface);
  }
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
  .mention-suggestions {
    position: absolute;
    right: 0;
    bottom: 100%;
    left: 0;
    z-index: 4;
    max-height: 230px;
    margin-bottom: 8px;
    overflow-y: auto;
    border: 1px solid var(--line);
    background: var(--surface);
    box-shadow: 0 10px 28px rgba(0, 0, 0, .3);
  }
  .mention-suggestion {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 9px;
    padding: 8px 10px;
    border: 0;
    border-bottom: 1px solid var(--line-soft);
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .mention-suggestion:last-child { border-bottom: 0; }
  .mention-suggestion:hover, .mention-suggestion.selected { background: var(--surface-3); }
  .mention-suggestion .participant-avatar { width: 25px; height: 25px; }
  .mention-suggestion-copy { min-width: 0; }
  .mention-suggestion-name { overflow: hidden; font-size: 12px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
  .mention-suggestion-handle { margin-top: 1px; color: var(--cyan); font: 10px var(--mono); }
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
    overflow: hidden;
    border: 1px solid var(--line);
    color: var(--cyan);
    font: 10px var(--mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .participant-human .participant-avatar { color: var(--warm); }
  .participant-name { overflow: hidden; font-size: 12px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
  .participant-sub { margin-top: 2px; color: var(--faint); font: 10px var(--mono); }
  .participant-state { display: inline-flex; align-items: center; gap: 5px; color: var(--muted); font-size: 10px; }
  .state-dot { width: 6px; height: 6px; background: var(--faint); border-radius: 50%; }
  .state-dot.idle { background: var(--accent); }
  .state-dot.prompting { background: var(--warm); }
  .state-dot.offline, .state-dot.error { background: var(--danger); }
  .participant-actions { display: flex; gap: 9px; margin-top: 8px; }
  .configure-participant, .remove-participant { padding: 0; border: 0; background: transparent; cursor: pointer; font-size: 10px; }
  .configure-participant { color: var(--cyan); }
  .configure-participant:hover { color: var(--text); }
  .remove-participant { padding: 0 3px; border: 0; background: transparent; color: var(--faint); cursor: pointer; }
  .remove-participant:hover { color: var(--danger); }
  .setting { margin-top: 11px; }
  .setting label, .add-agent label, .dialog-field label { display: block; margin-bottom: 4px; color: var(--faint); font: 10px var(--mono); text-transform: uppercase; }
  .setting select, .setting input, .add-agent select, .dialog-field input, .dialog-field select {
    width: 100%;
    min-height: 30px;
    padding: 5px 7px;
    border: 1px solid var(--line);
    border-radius: 0;
    background: var(--surface-2);
    color: var(--text);
    font-size: 12px;
  }
  .setting select:hover, .setting input:hover, .add-agent select:hover, .dialog-field input:hover, .dialog-field select:hover { border-color: var(--muted); }
  .setting-saving { margin-top: 4px; color: var(--accent); font-size: 10px; }
  .add-agent {
    padding: 16px;
    border-top: 1px solid var(--line);
  }
  .add-agent .primary-button { width: 100%; }
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
  .avatar-config-row { display: flex; align-items: center; gap: 9px; }
  .avatar-preview {
    display: grid;
    width: 36px;
    height: 36px;
    flex: 0 0 auto;
    place-items: center;
    border: 1px solid var(--cyan);
    color: var(--cyan);
    font: 11px var(--mono);
  }
  .avatar-config-row input { min-width: 0; flex: 1; }
  .avatar-picker { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; }
  .avatar-choice {
    min-width: 32px;
    height: 28px;
    padding: 0 6px;
    border: 1px solid var(--line);
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
    font: 10px var(--mono);
  }
  .avatar-choice:hover, .avatar-choice.selected { border-color: var(--cyan); color: var(--cyan); }
  .config-setting-note { margin-top: 5px; color: var(--faint); font-size: 10px; }
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
    .workspace { min-height: calc(100vh - 230px); overflow: visible; }
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
        <textarea id="messageInput" rows="3" placeholder="Message the thread agents..." spellcheck="true" aria-autocomplete="list" aria-controls="mentionSuggestions"></textarea>
        <div class="mention-suggestions hidden" id="mentionSuggestions" role="listbox" aria-label="Mention agents"></div>
        <div class="composer-foot">
          <span class="composer-status" id="composerStatus">Attach an agent to start.</span>
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
      <button class="primary-button" id="addAgentButton" type="button">Add agent</button>
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
<dialog id="participantConfigDialog">
  <form class="dialog-inner" id="participantConfigForm" method="dialog">
    <h2 id="participantConfigTitle">Add agent</h2>
    <p id="participantConfigDescription">Create a named participant with its own session settings.</p>
    <div class="dialog-field">
      <label for="participantAgent">Agent</label>
      <select id="participantAgent"></select>
    </div>
    <div class="dialog-field">
      <label for="participantName">Name</label>
      <input id="participantName" type="text" maxlength="80" placeholder="Frontend Codex" autocomplete="off">
    </div>
    <div class="dialog-field">
      <label for="participantAvatar">Avatar</label>
      <div class="avatar-config-row">
        <span class="avatar-preview" id="participantAvatarPreview">AI</span>
        <input id="participantAvatar" type="text" maxlength="8" placeholder="AI" autocomplete="off">
      </div>
      <div class="avatar-picker" id="participantAvatarPicker" aria-label="Avatar presets"></div>
    </div>
    <div class="dialog-field" id="participantModelField"></div>
    <div class="dialog-field" id="participantReasoningField"></div>
    <div class="config-setting-note">Settings apply to this participant's next turn.</div>
    <div class="dialog-actions">
      <button class="quiet-button" id="cancelParticipantConfigButton" value="cancel" type="button">Cancel</button>
      <button class="primary-button" id="saveParticipantConfigButton" value="default" type="submit">Add agent</button>
    </div>
  </form>
</dialog>
<div class="toast" id="toast" role="status"></div>

<script>
const $ = (id) => document.getElementById(id);
const escapeHtml = (value) => String(value ?? "").replace(/[&<>\"]/g, (char) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;"
}[char]));

function markdownUrl(value) {
  const url = String(value ?? "").trim();
  if (/^(https?:\/\/|mailto:|tel:)/i.test(url)) return url;
  if (/^#[A-Za-z0-9_-]+$/.test(url)) return url;
  return null;
}

function tableCells(line) {
  let value = line.trim();
  if (value.startsWith("|")) value = value.slice(1);
  if (value.endsWith("|")) value = value.slice(0, -1);
  return value.split("|").map((cell) => cell.trim());
}

function isTableSeparator(line) {
  return /^\s*\|?\s*:?-+:?\s*(?:\|\s*:?-+:?\s*)+\|?\s*$/.test(line || "");
}

// Keep the local page dependency-free while ensuring agent-authored markup is safe.
function renderInlineMarkdown(value) {
  const source = String(value ?? "");
  let html = "";
  let index = 0;
  const appendText = (text) => { html += escapeHtml(text); };

  while (index < source.length) {
    if (source[index] === "\\" && /[\\`*_{}[\]()#+.!\->]/.test(source[index + 1] || "")) {
      appendText(source[index + 1]);
      index += 2;
      continue;
    }
    if (source[index] === "\n") {
      html += "<br>";
      index += 1;
      continue;
    }

    if (source[index] === "`") {
      let runLength = 1;
      while (source[index + runLength] === "`") runLength += 1;
      const marker = "`".repeat(runLength);
      const end = source.indexOf(marker, index + runLength);
      if (end >= 0) {
        let code = source.slice(index + runLength, end);
        if (code.startsWith(" ") && code.endsWith(" ") && code.trim()) code = code.slice(1, -1);
        html += `<code>${escapeHtml(code.replace(/\n/g, " "))}</code>`;
        index = end + runLength;
        continue;
      }
    }

    const imageMatch = source.slice(index).match(/^!\[([^\]\n]*)\]\((\S+?)(?:\s+["']([^"']*)["'])?\)/);
    if (imageMatch) {
      const imageUrl = markdownUrl(imageMatch[2]);
      if (imageUrl && /^https?:\/\//i.test(imageUrl)) {
        const title = imageMatch[3] ? ` title="${escapeHtml(imageMatch[3])}"` : "";
        html += `<img src="${escapeHtml(imageUrl)}" alt="${escapeHtml(imageMatch[1])}" loading="lazy" referrerpolicy="no-referrer"${title}>`;
      } else {
        appendText(imageMatch[0]);
      }
      index += imageMatch[0].length;
      continue;
    }

    const linkMatch = source.slice(index).match(/^\[([^\]\n]+)\]\((\S+?)(?:\s+["']([^"']*)["'])?\)/);
    if (linkMatch) {
      const linkUrl = markdownUrl(linkMatch[2]);
      if (linkUrl) {
        const title = linkMatch[3] ? ` title="${escapeHtml(linkMatch[3])}"` : "";
        html += `<a href="${escapeHtml(linkUrl)}" target="_blank" rel="noreferrer noopener"${title}>${renderInlineMarkdown(linkMatch[1])}</a>`;
      } else {
        html += renderInlineMarkdown(linkMatch[1]);
      }
      index += linkMatch[0].length;
      continue;
    }

    const autolinkMatch = source.slice(index).match(/^<(https?:\/\/[^\s>]+|mailto:[^\s>]+)>/i);
    if (autolinkMatch) {
      const linkUrl = markdownUrl(autolinkMatch[1]);
      html += `<a href="${escapeHtml(linkUrl)}" target="_blank" rel="noreferrer noopener">${escapeHtml(autolinkMatch[1])}</a>`;
      index += autolinkMatch[0].length;
      continue;
    }

    let marker = null;
    if (source.startsWith("**", index)) marker = "**";
    else if (source.startsWith("__", index)) marker = "__";
    else if (source.startsWith("~~", index)) marker = "~~";
    if (marker) {
      const end = source.indexOf(marker, index + marker.length);
      if (end > index + marker.length) {
        const tag = marker === "~~" ? "del" : "strong";
        html += `<${tag}>${renderInlineMarkdown(source.slice(index + marker.length, end))}</${tag}>`;
        index = end + marker.length;
        continue;
      }
    }

    if (source[index] === "*" || source[index] === "_") {
      const marker = source[index];
      const end = source.indexOf(marker, index + 1);
      if (end > index + 1 && source[index + 1] !== " ") {
        html += `<em>${renderInlineMarkdown(source.slice(index + 1, end))}</em>`;
        index = end + 1;
        continue;
      }
    }

    appendText(source[index]);
    index += 1;
  }
  return html;
}

function renderMarkdown(value) {
  const lines = String(value ?? "").replace(/\r\n?/g, "\n").split("\n");
  let html = "";
  let paragraph = [];
  let listItems = [];
  let listTag = null;
  let quoteLines = [];

  const flushParagraph = () => {
    if (!paragraph.length) return;
    html += `<p>${renderInlineMarkdown(paragraph.join("\n"))}</p>`;
    paragraph = [];
  };
  const flushList = () => {
    if (!listItems.length) return;
    html += `<${listTag}>${listItems.map((item) => `<li>${renderInlineMarkdown(item)}</li>`).join("")}</${listTag}>`;
    listItems = [];
    listTag = null;
  };
  const flushQuote = () => {
    if (!quoteLines.length) return;
    html += `<blockquote>${renderMarkdown(quoteLines.join("\n"))}</blockquote>`;
    quoteLines = [];
  };
  const flushFlow = () => {
    flushParagraph();
    flushList();
    flushQuote();
  };

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const fence = line.match(/^ {0,3}(`{3,}|~{3,})\s*(.*)$/);
    if (fence) {
      flushFlow();
      const marker = fence[1];
      const closing = new RegExp(`^ {0,3}${marker[0]}{${marker.length},}\\s*$`);
      const codeLines = [];
      let closed = false;
      for (index += 1; index < lines.length; index += 1) {
        if (closing.test(lines[index])) {
          closed = true;
          break;
        }
        codeLines.push(lines[index]);
      }
      const language = (fence[2].trim().split(/\s+/)[0] || "").replace(/[^A-Za-z0-9_-]/g, "");
      const className = language ? ` class="language-${escapeHtml(language)}"` : "";
      html += `<pre><code${className}>${escapeHtml(codeLines.join("\n"))}</code></pre>`;
      if (!closed) break;
      continue;
    }

    if (!line.trim()) {
      flushFlow();
      continue;
    }

    const quote = line.match(/^ {0,3}>\s?(.*)$/);
    if (quote) {
      flushParagraph();
      flushList();
      quoteLines.push(quote[1]);
      continue;
    }
    flushQuote();

    if (line.includes("|") && isTableSeparator(lines[index + 1])) {
      flushParagraph();
      flushList();
      const headers = tableCells(line);
      const rows = [];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
        rows.push(tableCells(lines[index]));
        index += 1;
      }
      index -= 1;
      html += `<table><thead><tr>${headers.map((cell) => `<th>${renderInlineMarkdown(cell)}</th>`).join("")}</tr></thead>`;
      if (rows.length) html += `<tbody>${rows.map((row) => `<tr>${headers.map((_, cellIndex) => `<td>${renderInlineMarkdown(row[cellIndex] || "")}</td>`).join("")}</tr>`).join("")}</tbody>`;
      html += "</table>";
      continue;
    }

    const heading = line.match(/^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      flushParagraph();
      flushList();
      const level = heading[1].length;
      html += `<h${level}>${renderInlineMarkdown(heading[2])}</h${level}>`;
      continue;
    }

    if (/^ {0,3}(?:\*\s*){3,}$/.test(line) || /^ {0,3}(?:-\s*){3,}$/.test(line) || /^ {0,3}(?:_\s*){3,}$/.test(line)) {
      flushParagraph();
      flushList();
      html += "<hr>";
      continue;
    }

    const unordered = line.match(/^ {0,3}[-+*]\s+(.+)$/);
    const ordered = line.match(/^ {0,3}\d+[.)]\s+(.+)$/);
    if (unordered || ordered) {
      flushParagraph();
      const nextTag = ordered ? "ol" : "ul";
      if (listTag && listTag !== nextTag) flushList();
      listTag = nextTag;
      listItems.push((unordered || ordered)[1]);
      continue;
    }
    if (listTag && /^\s{2,}\S/.test(line)) {
      listItems[listItems.length - 1] += `\n${line.trim()}`;
      continue;
    }
    flushList();
    paragraph.push(line);
  }

  flushFlow();
  return html;
}

const state = {
  socket: null,
  reconnectTimer: null,
  timelineFollowing: true,
  timelineScrollFrame: null,
  agents: [],
  threads: new Map(),
  snapshots: new Map(),
  messages: new Map(),
  currentThreadId: null,
  attached: new Set(),
  configuringParticipantId: null,
  mentionCandidates: [],
  mentionIndex: 0,
  mentionRange: null,
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

function timelineIsNearBottom() {
  const timeline = $("timeline");
  return timeline.scrollHeight - timeline.scrollTop - timeline.clientHeight < 96;
}

function scheduleTimelineScroll() {
  if (state.timelineScrollFrame !== null) return;
  state.timelineScrollFrame = requestAnimationFrame(() => {
    state.timelineScrollFrame = null;
    if (state.timelineFollowing) $("timeline").scrollTop = $("timeline").scrollHeight;
  });
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

function mentionHandleForParticipant(participant) {
  return participant && (participant.mention_handle || participant.agent_id || "");
}

function mentionedParticipants(content, participants) {
  const handles = Array.from(
    String(content || "").matchAll(/(?:^|[\s([{"'，。,:;])@([\p{L}\p{N}._-]+)/gu),
    (match) => match[1].toLowerCase(),
  );
  if (!handles.length) return [];
  return (participants || []).filter((participant) => {
    if (participant.kind !== "agent") return false;
    const aliases = [mentionHandleForParticipant(participant), participant.agent_id]
      .filter(Boolean)
      .map((handle) => handle.toLowerCase());
    return handles.some((handle) => aliases.includes(handle));
  });
}

function currentMentionToken() {
  const input = $("messageInput");
  const cursor = input.selectionStart;
  if (cursor !== input.selectionEnd) return null;
  const before = input.value.slice(0, cursor);
  const match = before.match(/(?:^|[\s([{"'，。,:;])@([\p{L}\p{N}._-]*)$/u);
  if (!match) return null;
  const start = before.lastIndexOf("@");
  return { start, end: cursor, query: match[1].toLowerCase() };
}

function hideMentionSuggestions() {
  state.mentionCandidates = [];
  state.mentionIndex = 0;
  state.mentionRange = null;
  $("mentionSuggestions").classList.add("hidden");
}

function updateMentionSuggestionSelection() {
  document.querySelectorAll("[data-mention-index]").forEach((element) => {
    element.classList.toggle("selected", Number(element.dataset.mentionIndex) === state.mentionIndex);
    element.setAttribute("aria-selected", Number(element.dataset.mentionIndex) === state.mentionIndex ? "true" : "false");
  });
}

function renderMentionSuggestions() {
  const snapshot = currentSnapshot();
  const token = currentMentionToken();
  if (!snapshot || !token) {
    hideMentionSuggestions();
    return;
  }

  const participants = snapshot.participants;
  const candidates = participants
    .filter((participant) => participant.kind === "agent")
    .filter((participant) => {
      const name = participantDisplayName(participant, participants).toLowerCase();
      const handle = mentionHandleForParticipant(participant).toLowerCase();
      return !token.query || name.includes(token.query) || handle.includes(token.query);
    });
  if (!candidates.length) {
    hideMentionSuggestions();
    return;
  }

  state.mentionCandidates = candidates;
  state.mentionIndex = Math.min(state.mentionIndex, candidates.length - 1);
  state.mentionRange = token;
  $("mentionSuggestions").innerHTML = candidates.map((participant, index) => {
    const name = participantDisplayName(participant, participants);
    const avatar = participant.avatar || defaultAvatarForName(name);
    const handle = mentionHandleForParticipant(participant);
    return "<button class=\"mention-suggestion\" type=\"button\" role=\"option\" data-mention-index=\"" + index + "\" aria-selected=\"" + (index === state.mentionIndex ? "true" : "false") + "\">" +
      "<span class=\"participant-avatar\">" + escapeHtml(avatar) + "</span>" +
      "<span class=\"mention-suggestion-copy\"><span class=\"mention-suggestion-name\">" + escapeHtml(name) + "</span><span class=\"mention-suggestion-handle\">@" + escapeHtml(handle) + "</span></span>" +
      "</button>";
  }).join("");
  $("mentionSuggestions").classList.remove("hidden");
  document.querySelectorAll("[data-mention-index]").forEach((element) => {
    element.addEventListener("mousedown", (event) => {
      event.preventDefault();
      chooseMention(Number(element.dataset.mentionIndex));
    });
  });
  updateMentionSuggestionSelection();
}

function chooseMention(index) {
  const participant = state.mentionCandidates[index];
  const range = state.mentionRange;
  if (!participant || !range) return;
  const input = $("messageInput");
  const handle = mentionHandleForParticipant(participant);
  const replacement = "@" + handle + " ";
  input.value = input.value.slice(0, range.start) + replacement + input.value.slice(range.end);
  const cursor = range.start + replacement.length;
  input.focus();
  input.setSelectionRange(cursor, cursor);
  hideMentionSuggestions();
  renderComposerStatus();
}

function handleMentionKeydown(event) {
  if ($("mentionSuggestions").classList.contains("hidden") || !state.mentionCandidates.length) return false;
  if (event.key === "ArrowDown") {
    event.preventDefault();
    state.mentionIndex = (state.mentionIndex + 1) % state.mentionCandidates.length;
    updateMentionSuggestionSelection();
    return true;
  }
  if (event.key === "ArrowUp") {
    event.preventDefault();
    state.mentionIndex = (state.mentionIndex - 1 + state.mentionCandidates.length) % state.mentionCandidates.length;
    updateMentionSuggestionSelection();
    return true;
  }
  if (event.key === "Enter" || event.key === "Tab") {
    event.preventDefault();
    chooseMention(state.mentionIndex);
    return true;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    hideMentionSuggestions();
    return true;
  }
  return false;
}

function settingDescriptor(participant, settingId) {
  const summary = agentSummary(participant.agent_id);
  return summary && (summary.settings || []).find((setting) => setting.id === settingId);
}

function settingMarkup(participant, settingId, label, key) {
  const descriptor = settingDescriptor(participant, settingId);
  if (!descriptor) return "";
  const current = participant.settings && participant.settings[key] ? participant.settings[key] : (descriptor.current_value || "");
  return `<div class="setting"><label>${escapeHtml(label)}</label><select data-setting="${escapeHtml(settingId)}" data-participant="${escapeHtml(participant.participant_id)}">${settingOptionsMarkup(descriptor, current)}</select></div>`;
}

function settingOptionsMarkup(descriptor, current) {
  const values = Array.isArray(descriptor.values) ? descriptor.values.slice() : [];
  if (current && !values.some((value) => value.id === current)) values.unshift({ id: current, label: current });
  return `<option value="">Default</option>${values.map((value) => `<option value="${escapeHtml(value.id)}" ${value.id === current ? "selected" : ""}>${escapeHtml(value.label)}</option>`).join("")}`;
}

function participantDisplayName(participant, participants) {
  if (participant.kind !== "agent" || !participant.agent_id) return participant.display_name;
  const sameAgents = (participants || []).filter((item) => item.kind === "agent" && item.agent_id === participant.agent_id);
  if (sameAgents.length < 2) return participant.display_name;
  if (sameAgents.some((item) => item.display_name !== participant.display_name)) return participant.display_name;
  const instance = sameAgents.findIndex((item) => item.participant_id === participant.participant_id) + 1;
  return `${participant.display_name} #${instance}`;
}

function defaultAvatarForName(value) {
  const words = String(value ?? "").trim().split(/\s+/).filter(Boolean);
  const initials = words.slice(0, 2).map((word) => word[0]).join("").toUpperCase();
  return initials || "AI";
}

function renderAddAgentButton() {
  const availableAgents = state.agents.filter((agent) => agent.status === "online");
  $("addAgentButton").disabled = !state.currentThreadId || !availableAgents.length;
}

function configSettingMarkup(agentId, settingId, label, key, settings) {
  const summary = agentSummary(agentId);
  const descriptor = summary && (summary.settings || []).find((setting) => setting.id === settingId);
  if (!descriptor) return "";
  const current = settings && settings[key] ? settings[key] : (descriptor.current_value || "");
  return `<label for="participantConfig${key === "model" ? "Model" : "Reasoning"}">${escapeHtml(label)}</label><select id="participantConfig${key === "model" ? "Model" : "Reasoning"}">${settingOptionsMarkup(descriptor, current)}</select>`;
}

function renderConfigSettings(agentId, settings) {
  $("participantModelField").innerHTML = configSettingMarkup(agentId, "model", "Model", "model", settings);
  $("participantReasoningField").innerHTML = configSettingMarkup(agentId, "reasoning_effort", "Reasoning effort", "reasoning_effort", settings);
}

function renderConfigAvatarPresets() {
  const presets = ["AI", "DEV", "QA", "OPS", "DOC", "UX"];
  $("participantAvatarPicker").innerHTML = presets.map((preset) => `<button class="avatar-choice" type="button" data-avatar-preset="${preset}">${preset}</button>`).join("");
  document.querySelectorAll("[data-avatar-preset]").forEach((element) => {
    element.addEventListener("click", () => {
      $("participantAvatar").value = element.dataset.avatarPreset;
      updateConfigAvatarPreview();
    });
  });
}

function updateConfigAvatarPreview() {
  const name = $("participantName").value.trim();
  const avatar = $("participantAvatar").value.trim() || defaultAvatarForName(name);
  $("participantAvatarPreview").textContent = avatar.slice(0, 8);
  document.querySelectorAll("[data-avatar-preset]").forEach((element) => {
    element.classList.toggle("selected", element.dataset.avatarPreset === avatar);
  });
}

function renderConfigAgentOptions(selectedAgentId, locked) {
  const agents = state.agents.filter((agent) => agent.status === "online" || agent.agent_id === selectedAgentId);
  $("participantAgent").innerHTML = agents.map((agent) => `<option value="${escapeHtml(agent.agent_id)}" ${agent.agent_id === selectedAgentId ? "selected" : ""}>${escapeHtml(agent.name)}</option>`).join("");
  $("participantAgent").disabled = locked;
}

function openParticipantConfig(participantId = null) {
  if (!state.currentThreadId) return;
  const snapshot = currentSnapshot();
  const participant = participantId && snapshot ? snapshot.participants.find((item) => item.participant_id === participantId) : null;
  const fallbackAgent = state.agents.find((agent) => agent.status === "online");
  const agentId = participant ? participant.agent_id : (fallbackAgent && fallbackAgent.agent_id);
  if (!agentId) {
    showToast("No online agents are available.");
    return;
  }
  const summary = agentSummary(agentId);
  const defaultName = participant ? participant.display_name : (summary ? summary.name : agentId);
  const defaultAvatar = participant ? (participant.avatar || defaultAvatarForName(defaultName)) : defaultAvatarForName(defaultName);
  const settings = participant ? (participant.settings || {}) : {
    model: summary && (summary.settings || []).find((setting) => setting.id === "model")?.current_value || null,
    reasoning_effort: summary && (summary.settings || []).find((setting) => setting.id === "reasoning_effort")?.current_value || null,
  };
  state.configuringParticipantId = participantId;
  $("participantConfigTitle").textContent = participant ? "Configure agent" : "Add agent";
  $("participantConfigDescription").textContent = participant ? "Update this participant's name, avatar, or session settings." : "Create a named participant with its own session settings.";
  $("saveParticipantConfigButton").textContent = participant ? "Save configuration" : "Add agent";
  renderConfigAgentOptions(agentId, Boolean(participant));
  $("participantName").value = defaultName;
  $("participantAvatar").value = defaultAvatar;
  renderConfigSettings(agentId, settings);
  renderConfigAvatarPresets();
  updateConfigAvatarPreview();
  const dialog = $("participantConfigDialog");
  if (typeof dialog.showModal === "function") dialog.showModal();
  else dialog.setAttribute("open", "open");
  setTimeout(() => $(participant ? "participantName" : "participantAgent").focus(), 0);
}

function renderParticipants() {
  const snapshot = currentSnapshot();
  const participants = snapshot ? snapshot.participants : [];
  $("participantCount").textContent = `${participants.filter((p) => p.kind === "agent").length} connected`;
  if (!snapshot) {
    $("participants").innerHTML = '<div class="inspector-note">Select a thread to manage its agents.</div>';
    renderAddAgentButton();
    return;
  }
  $("participants").innerHTML = participants.map((participant) => {
    const isHuman = participant.kind === "human";
    const status = participantStatus(participant);
    const displayName = participantDisplayName(participant, participants);
    const avatar = participant.avatar || (isHuman ? "YOU" : "AI");
    const label = participant.agent_id ? `@${participant.mention_handle || participant.agent_id}` : "thread owner";
    const controls = isHuman ? "" : `
      ${settingMarkup(participant, "model", "Model", "model")}
      ${settingMarkup(participant, "reasoning_effort", "Reasoning effort", "reasoning_effort")}`;
    return `<div class="participant ${isHuman ? "participant-human" : ""}">
      <div class="participant-head">
        <div class="participant-identity">
          <span class="participant-avatar">${escapeHtml(avatar)}</span>
          <div><div class="participant-name">${escapeHtml(displayName)}</div><div class="participant-sub">${escapeHtml(label)}</div></div>
        </div>
        <div class="participant-state"><span class="state-dot ${escapeHtml(status)}"></span>${escapeHtml(status)}</div>
      </div>
      ${isHuman ? "" : `<div class="participant-actions"><button class="configure-participant" data-configure-participant="${escapeHtml(participant.participant_id)}" type="button" title="Configure participant">Configure</button><button class="remove-participant" data-remove-participant="${escapeHtml(participant.participant_id)}" type="button" title="Remove participant" aria-label="Remove ${escapeHtml(displayName)}">x remove</button></div>${controls}`}
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
  document.querySelectorAll("[data-configure-participant]").forEach((element) => {
    element.addEventListener("click", () => openParticipantConfig(element.dataset.configureParticipant));
  });
  renderAddAgentButton();
}

function renderComposerStatus() {
  const snapshot = currentSnapshot();
  if (!snapshot) {
    $("composerStatus").textContent = "Select a thread";
    $("sendButton").disabled = true;
    return;
  }
  const agents = snapshot.participants.filter((participant) => participant.kind === "agent");
  const mentioned = mentionedParticipants($("messageInput").value, agents);
  if (!agents.length) {
    $("composerStatus").textContent = "Attach an agent to start.";
    $("sendButton").disabled = true;
    return;
  }
  if (mentioned.length) {
    $("composerStatus").textContent = "Mention route: " + mentioned.map((participant) => "@" + mentionHandleForParticipant(participant)).join(", ");
  } else {
    $("composerStatus").textContent = "All thread agents";
  }
  $("sendButton").disabled = !state.socket || state.socket.readyState !== WebSocket.OPEN;
}

function renderTimeline() {
  const threadId = state.currentThreadId;
  const timeline = $("timeline");
  if (!threadId) return;
  const shouldFollow = state.timelineFollowing || timelineIsNearBottom();
  state.timelineFollowing = shouldFollow;
  const messages = ensureMessages(threadId);
  if (!messages.length) {
    timeline.innerHTML = '<div class="timeline-empty">No messages yet. Attach an agent and send the first message.</div>';
  } else {
    timeline.innerHTML = messages.map(renderMessage).join("");
  }
  if (shouldFollow || messages.length < 2) scheduleTimelineScroll();
}

function renderMessage(message) {
  if (message.kind === "user") {
    return `<article class="message user"><div class="message-head"><span class="message-name">${escapeHtml(message.sender && message.sender.display_name || "You")}</span><span class="message-tag">user</span><span>${escapeHtml(formatTime(message.created_at || Date.now()))}</span></div><div class="message-body">${escapeHtml(message.content)}</div></article>`;
  }
  const status = message.state || "streaming";
  const thinking = message.thinking ? `<details class="assistant-thinking"><summary>Reasoning</summary><div class="detail">${escapeHtml(message.thinking)}</div></details>` : "";
  const tools = message.tools && message.tools.length ? `<div class="assistant-tools">${message.tools.map((tool) => `<div class="tool-row"><span class="tool-status ${tool.status === "failed" ? "failed" : ""}">${escapeHtml(tool.status || "working")}</span><span>${escapeHtml(tool.title)}</span></div>`).join("")}</div>` : "";
  const plan = message.plan ? `<details class="assistant-plan"><summary>Plan update</summary><div class="detail">${escapeHtml(JSON.stringify(message.plan, null, 2))}</div></details>` : "";
  const body = message.response ? `<div class="message-body markdown">${renderMarkdown(message.response)}</div>` : (status === "streaming" ? '<div class="message-body">Working...</div>' : "");
  const footer = status === "streaming" ? "streaming" : (message.stop_reason || status);
  const avatar = message.avatar ? `<span class="message-avatar">${escapeHtml(message.avatar)}</span>` : "";
  return `<article class="message assistant ${escapeHtml(status)}"><div class="message-head">${avatar}<span class="message-name">${escapeHtml(message.display_name || message.agent_id || "Agent")}</span><span class="message-tag">agent</span><span>${escapeHtml(formatTime(message.created_at || Date.now()))}</span></div>${thinking}${tools}${plan}${body}<div class="assistant-footer">${escapeHtml(footer)}</div></article>`;
}

function assistantMessage(event) {
  const messages = ensureMessages(event.thread_id);
  const key = `${event.participant_id}:${event.turn_id}`;
  let message = messages.find((item) => item.kind === "assistant" && item.key === key);
  if (!message) {
    const participants = ensureSnapshot(event.thread_id).participants;
    const participant = participants.find((item) => item.participant_id === event.participant_id);
    message = { kind: "assistant", key, participant_id: event.participant_id, agent_id: event.agent_id, display_name: participant ? participantDisplayName(participant, participants) : event.agent_id, avatar: participant ? (participant.avatar || "AI") : "AI", thinking: "", response: "", tools: [], state: "streaming", created_at: Date.now() };
    messages.push(message);
  }
  return message;
}

function updateSetting(participantId, settingId, value) {
  const snapshot = currentSnapshot();
  const participant = snapshot && snapshot.participants.find((item) => item.participant_id === participantId);
  if (!snapshot || !participant) return;
  const settings = {
    model: settingId === "model" ? (value || null) : ((participant.settings || {}).model || null),
    // A model change can invalidate the old reasoning value, so let ACP
    // refresh the model's available thought levels before choosing one.
    reasoning_effort: settingId === "reasoning_effort" ? (value || null) : null,
  };
  if (send({ type: "set_thread_participant_settings", thread_id: snapshot.thread_id, participant_id: participantId, settings })) {
    $("composerStatus").textContent = "Saving participant settings...";
  }
}

function selectThread(threadId) {
  if (!state.threads.has(threadId)) return;
  state.currentThreadId = threadId;
  state.timelineFollowing = true;
  state.attached.delete(threadId);
  state.messages.set(threadId, []);
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
      updateThreadSummary(event.thread_id, { participant_count: ensureSnapshot(event.thread_id).participants.length, last_thread_seq: event.thread_seq });
      if (state.currentThreadId === event.thread_id) {
        renderAll();
        send({ type: "list_agents" });
      }
      break;
    case "thread_participant_settings_updated":
      applyParticipant(event.thread_id, event.participant);
      updateThreadSummary(event.thread_id, { last_thread_seq: event.thread_seq });
      if (state.currentThreadId === event.thread_id) {
        renderAll();
        $("composerStatus").textContent = "Participant settings saved";
        send({ type: "list_agents" });
      }
      break;
    case "thread_participant_removed":
      {
        const snapshot = ensureSnapshot(event.thread_id);
        snapshot.participants = snapshot.participants.filter((participant) => participant.participant_id !== event.participant_id);
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
  renderComposerStatus();
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

function closeParticipantConfig() {
  const dialog = $("participantConfigDialog");
  if (typeof dialog.close === "function") dialog.close();
  else dialog.removeAttribute("open");
  state.configuringParticipantId = null;
}

$("newThreadButton").addEventListener("click", openNewThreadDialog);
$("emptyNewThreadButton").addEventListener("click", openNewThreadDialog);
$("cancelNewThreadButton").addEventListener("click", () => $("newThreadDialog").close());
$("cancelParticipantConfigButton").addEventListener("click", closeParticipantConfig);
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
$("addAgentButton").addEventListener("click", () => openParticipantConfig());
$("participantAgent").addEventListener("change", () => {
  if (state.configuringParticipantId) return;
  const summary = agentSummary($("participantAgent").value);
  if (!summary) return;
  $("participantName").value = summary.name;
  $("participantAvatar").value = defaultAvatarForName(summary.name);
  const settings = {
    model: (summary.settings || []).find((setting) => setting.id === "model")?.current_value || null,
    reasoning_effort: (summary.settings || []).find((setting) => setting.id === "reasoning_effort")?.current_value || null,
  };
  renderConfigSettings(summary.agent_id, settings);
  updateConfigAvatarPreview();
});
$("participantName").addEventListener("input", updateConfigAvatarPreview);
$("participantAvatar").addEventListener("input", updateConfigAvatarPreview);
$("participantConfigForm").addEventListener("submit", (event) => {
  event.preventDefault();
  const snapshot = currentSnapshot();
  const agentId = $("participantAgent").value;
  const name = $("participantName").value.trim();
  if (!snapshot || !agentId || !name) {
    showToast("Choose an agent and enter a name.");
    return;
  }
  const config = {
    display_name: name,
    avatar: $("participantAvatar").value.trim() || defaultAvatarForName(name),
    settings: {
      model: $("participantConfigModel") ? ($("participantConfigModel").value || null) : null,
      reasoning_effort: $("participantConfigReasoning") ? ($("participantConfigReasoning").value || null) : null,
    },
  };
  const message = state.configuringParticipantId
    ? { type: "set_thread_participant_configuration", thread_id: snapshot.thread_id, participant_id: state.configuringParticipantId, config }
    : { type: "add_thread_participant_with_config", thread_id: snapshot.thread_id, agent_id: agentId, config };
  if (send(message)) closeParticipantConfig();
});
$("sendButton").addEventListener("click", sendMessage);
$("messageInput").addEventListener("input", () => {
  state.mentionIndex = 0;
  renderMentionSuggestions();
  renderComposerStatus();
});
$("messageInput").addEventListener("blur", () => setTimeout(hideMentionSuggestions, 120));
$("messageInput").addEventListener("keydown", (event) => {
  if (handleMentionKeydown(event)) return;
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
  const agents = snapshot.participants.filter((participant) => participant.kind === "agent");
  if (!agents.length) {
    showToast("Attach at least one agent.");
    return;
  }
  state.timelineFollowing = true;
  if (send({ type: "send_thread_message", thread_id: snapshot.thread_id, content, target_participant_ids: null })) {
    $("messageInput").value = "";
    $("messageInput").focus();
    hideMentionSuggestions();
    renderComposerStatus();
  }
}

$("timeline").addEventListener("scroll", () => {
  state.timelineFollowing = timelineIsNearBottom();
});

renderAll();
connect();
</script>
</body>
</html>"##;
