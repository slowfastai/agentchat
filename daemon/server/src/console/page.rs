//! The console page.
//!
//! One file, no build step, no framework, no network fetches beyond this
//! daemon. It polls rather than streams: a human decides at the gates, so
//! second-granularity updates are indistinguishable from live, and polling
//! survives the daemon restarting under it.

/// The whole console, served at `/`.
pub const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AgentChat runs</title>
<style>
  :root {
    --bg: #ffffff; --fg: #16181d; --muted: #6b7280; --line: #e3e6ea;
    --panel: #f6f7f9; --accent: #2563eb; --warn: #b45309; --danger: #b91c1c;
    --ok: #15803d;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --bg: #14161a; --fg: #e6e8eb; --muted: #9aa1ab; --line: #2a2e35;
      --panel: #1b1e24; --accent: #60a5fa; --warn: #fbbf24; --danger: #f87171;
      --ok: #4ade80;
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: var(--bg); color: var(--fg);
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    display: grid; grid-template-columns: 260px 1fr; height: 100vh;
  }
  aside { border-right: 1px solid var(--line); overflow-y: auto; padding: 12px; }
  main { overflow-y: auto; padding: 16px 20px; }
  h1 { font-size: 15px; margin: 0 0 10px; letter-spacing: .02em; }
  h2 { font-size: 14px; margin: 20px 0 8px; }
  h3 { font-size: 13px; margin: 14px 0 6px; }
  .muted { color: var(--muted); }
  .run { padding: 7px 9px; border-radius: 6px; cursor: pointer; margin-bottom: 3px; }
  .run:hover { background: var(--panel); }
  .run.active { background: var(--panel); outline: 1px solid var(--line); }
  .run .id { font-family: ui-monospace, Menlo, monospace; font-size: 12px; }
  .badge {
    display: inline-block; font-size: 11px; padding: 1px 6px; border-radius: 10px;
    border: 1px solid var(--line); color: var(--muted);
  }
  .badge.wait { color: var(--warn); border-color: var(--warn); }
  .badge.done { color: var(--ok); border-color: var(--ok); }
  .badge.err { color: var(--danger); border-color: var(--danger); }
  button {
    font: inherit; padding: 6px 12px; border-radius: 6px; cursor: pointer;
    border: 1px solid var(--line); background: var(--panel); color: var(--fg);
  }
  button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  button:disabled { opacity: .5; cursor: default; }
  input[type=text], textarea, select {
    font: inherit; width: 100%; padding: 7px 9px; border-radius: 6px;
    border: 1px solid var(--line); background: var(--bg); color: var(--fg);
  }
  textarea { resize: vertical; font-family: ui-monospace, Menlo, monospace; font-size: 13px; }
  select[multiple] { height: 96px; }
  label { display: block; font-size: 12px; color: var(--muted); margin: 10px 0 4px; }
  .row { display: flex; gap: 12px; flex-wrap: wrap; }
  .row > * { flex: 1 1 200px; }
  #log {
    font-family: ui-monospace, Menlo, monospace; font-size: 12px;
    white-space: pre-wrap; background: var(--panel); border: 1px solid var(--line);
    border-radius: 8px; padding: 10px; height: 300px; overflow-y: auto;
  }
  .card { border: 1px solid var(--line); border-radius: 8px; padding: 12px; margin-bottom: 10px; }
  .card.dispute { border-color: var(--warn); }
  .card .where { font-family: ui-monospace, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .card .said { margin: 6px 0; }
  .card .author { border-left: 3px solid var(--warn); padding-left: 9px; margin-top: 8px; }
  details { border: 1px solid var(--line); border-radius: 8px; padding: 8px 12px; margin-bottom: 8px; }
  summary { cursor: pointer; font-size: 13px; }
  table { border-collapse: collapse; font-size: 13px; }
  td, th { text-align: left; padding: 3px 14px 3px 0; }
  .err { color: var(--danger); }
  .hidden { display: none; }
</style>
</head>
<body>
<aside>
  <h1>Runs</h1>
  <div id="runs"></div>
  <button id="newBtn" style="margin-top:12px;width:100%">+ New run</button>
  <p class="muted" style="font-size:11px;margin-top:14px" id="cwd"></p>
</aside>

<main>
  <section id="newRun">
    <h2>Start a run</h2>
    <label for="brief">Brief — what do you want done, in your words</label>
    <textarea id="brief" rows="10" placeholder="# Goal&#10;&#10;...&#10;&#10;# Out of scope&#10;&#10;..."></textarea>
    <p class="muted" style="font-size:12px">
      Out-of-scope matters: <code>contradicts_brief</code> and
      <code>missing_requirement</code> are judged against this text, so a vague
      brief gives reviewers nothing objective to point at.
    </p>
    <div class="row">
      <div><label for="planner">Planner</label><select id="planner"></select></div>
      <div><label for="planReviewers">Plan reviewers</label><select id="planReviewers" multiple></select></div>
    </div>
    <div class="row">
      <div><label for="implementer">Implementer</label><select id="implementer"></select></div>
      <div><label for="codeReviewers">Code reviewers</label><select id="codeReviewers" multiple></select></div>
    </div>
    <label style="margin-top:12px">
      <input type="checkbox" id="planOnly" checked style="width:auto"> Plan only — stop after the plan is approved, never touch the working tree
    </label>
    <p id="startErr" class="err"></p>
    <button class="primary" id="startBtn" style="margin-top:8px">Start</button>
  </section>

  <section id="detail" class="hidden">
    <h2 id="detailTitle"></h2>
    <p class="muted" id="detailMeta"></p>
    <p class="err" id="detailErr"></p>
    <div id="log"></div>
    <div id="approval"></div>
  </section>
</main>

<script>
const $ = (id) => document.getElementById(id);
let agents = [];
let current = null;
let cursor = 0;
let poller = null;

const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) =>
  ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

async function api(path, options) {
  const res = await fetch(path, options);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || res.statusText);
  return body;
}

function fillAgents() {
  const single = (el, skipFirst) => {
    el.innerHTML = agents.map((a, i) =>
      `<option value="${esc(a)}"${i === 0 && !skipFirst ? " selected" : ""}>${esc(a)}</option>`).join("");
  };
  single($("planner"));
  single($("implementer"));
  const multi = (el, exclude) => {
    el.innerHTML = agents.map((a) =>
      `<option value="${esc(a)}"${a === exclude ? "" : " selected"}>${esc(a)}</option>`).join("");
  };
  multi($("planReviewers"), agents[0]);
  multi($("codeReviewers"), agents[0]);
}

function picked(el) {
  return Array.from(el.selectedOptions).map((o) => o.value);
}

function statusBadge(run) {
  if (run.error) return `<span class="badge err">failed</span>`;
  if (run.pending) return `<span class="badge wait">needs you</span>`;
  if (run.finished) return `<span class="badge done">${esc(run.status)}</span>`;
  return `<span class="badge">${esc(run.stage || run.status)}</span>`;
}

async function refreshRuns() {
  const { runs } = await api("/api/runs");
  $("runs").innerHTML = runs.map((r) => `
    <div class="run ${r.run_id === current ? "active" : ""}" data-id="${esc(r.run_id)}">
      <div class="id">${esc(r.run_id)}</div>
      <div>${statusBadge(r)}</div>
    </div>`).join("") || `<p class="muted">No runs yet.</p>`;
  for (const el of document.querySelectorAll(".run")) {
    el.onclick = () => selectRun(el.dataset.id);
  }
}

function selectRun(id) {
  current = id;
  cursor = 0;
  $("log").textContent = "";
  $("approval").innerHTML = "";
  $("detailErr").textContent = "";
  $("newRun").classList.add("hidden");
  $("detail").classList.remove("hidden");
  $("detailTitle").textContent = id;
  refreshRuns();
  poll();
}

function showNewRun() {
  current = null;
  $("detail").classList.add("hidden");
  $("newRun").classList.remove("hidden");
  refreshRuns();
}

function renderFinding(item, className) {
  const f = item.finding, d = item.disposition;
  return `<div class="card ${className}">
    <div class="where">${esc(f.location || f.file)} · ${esc(f.category)}</div>
    <div class="said"><strong>${esc(f.reviewer)}:</strong> ${esc(f.problem)}</div>
    ${f.evidence ? `<div class="muted">evidence: ${esc(f.evidence)}</div>` : ""}
    ${f.recommendation ? `<div class="muted">suggested: ${esc(f.recommendation)}</div>` : ""}
    ${d && d.reason ? `<div class="author"><strong>author ${esc(d.action)}:</strong> ${esc(d.reason)}</div>` : ""}
  </div>`;
}

function renderApproval(run) {
  const p = run.pending;
  if (!p) { $("approval").innerHTML = ""; return; }
  const s = p.summary;

  const disputes = p.disputes.length
    ? `<h3>Decide — the agents could not settle these</h3>` +
      p.disputes.map((i) => renderFinding(i, "dispute")).join("")
    : `<h3>Decide</h3><p class="muted">Nothing is disputed. The reviewers and the author agreed on everything blocking.</p>`;

  const followups = p.followups.length
    ? `<details><summary>Follow-ups nobody adopted (${p.followups.length})</summary>
       ${p.followups.map((i) => renderFinding(i, "")).join("")}</details>`
    : "";

  const archive = (p.archive || []).map((sec) =>
    `<details ${sec.expanded ? "open" : ""}><summary>${esc(sec.title)}</summary>
      ${sec.groups.map((g) => `<h3>${esc(g.file)} · ${esc(g.category)} — ${g.consensus} reviewer(s)</h3>
        ${g.items.map((i) => renderFinding(i, "")).join("")}`).join("")}
    </details>`).join("");

  $("approval").innerHTML = `
    <h2>${esc(p.phase)} v${p.version} ready for review
      <span class="badge wait">${esc(p.exit_reason || "unknown")}</span></h2>
    ${disputes}
    <h3>Summary</h3>
    <table>
      <tr><th></th><th>raised</th><th>accepted</th><th>disputed</th></tr>
      <tr><td>blocking</td><td>${s.blocking_raised}</td><td>${s.blocking_accepted}</td><td>${s.blocking_disputed}</td></tr>
      <tr><td>advisory</td><td>${s.non_blocking_raised}</td><td>${s.non_blocking_adopted}</td><td>${s.non_blocking_declined} left</td></tr>
    </table>
    <p class="muted">${s.cycles_used} cycle(s)${s.human_iterations ? `, ${s.human_iterations} human round-trip(s)` : ""}</p>
    ${followups}
    ${archive}
    <label for="comments">Comments — sent back with "Request changes", and the budget resets</label>
    <textarea id="comments" rows="3"></textarea>
    <p class="err" id="decideErr"></p>
    <div class="row" style="margin-top:8px">
      <button class="primary" onclick="decide('approve')">Approve</button>
      <button onclick="decide('request_changes')">Request changes</button>
      <button onclick="decide('cancel')">Cancel run</button>
    </div>`;
}

async function decide(kind) {
  const comments = ($("comments") || {}).value || "";
  try {
    await api(`/api/runs/${encodeURIComponent(current)}/decision`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ decision: kind, comments }),
    });
    $("approval").innerHTML = `<p class="muted">Sent. Waiting for the run to pick it up…</p>`;
  } catch (e) {
    const box = $("decideErr");
    if (box) box.textContent = e.message;
  }
}

async function poll() {
  if (!current) return;
  try {
    const { entries, run } = await api(
      `/api/runs/${encodeURIComponent(current)}/log?after=${cursor}`);
    if (entries.length) {
      cursor = entries[entries.length - 1].seq;
      const log = $("log");
      const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 40;
      log.textContent += entries.map((e) => e.line).join("\n") + "\n";
      if (atBottom) log.scrollTop = log.scrollHeight;
    }
    if (run) {
      $("detailMeta").textContent =
        `${run.status}${run.phase ? " · " + run.phase : ""}` +
        `${run.stage ? " · " + run.stage : ""} · v${run.version} · round ${run.round}` +
        ` · ${run.cycles_used} cycle(s)`;
      $("detailErr").textContent = run.error || "";
      renderApproval(run);
    }
    await refreshRuns();
  } catch (e) {
    $("detailErr").textContent = e.message;
  }
}

async function start() {
  $("startErr").textContent = "";
  $("startBtn").disabled = true;
  try {
    const { run_id } = await api("/api/runs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        brief: $("brief").value,
        planner: $("planner").value,
        plan_reviewers: picked($("planReviewers")),
        implementer: $("implementer").value,
        code_reviewers: picked($("codeReviewers")),
        plan_only: $("planOnly").checked,
      }),
    });
    selectRun(run_id);
  } catch (e) {
    $("startErr").textContent = e.message;
  } finally {
    $("startBtn").disabled = false;
  }
}

(async function init() {
  const config = await api("/api/config");
  agents = config.agents || [];
  $("cwd").textContent = config.working_dir;
  fillAgents();
  $("startBtn").onclick = start;
  $("newBtn").onclick = showNewRun;
  await refreshRuns();
  poller = setInterval(poll, 1000);
})();
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_self_contained() {
        // A strict local console must not depend on anything it cannot reach.
        for remote in ["http://", "https://", "//cdn", "integrity="] {
            assert!(
                !PAGE.contains(remote),
                "page reaches outside the daemon via {remote}"
            );
        }
    }

    #[test]
    fn the_page_talks_to_every_endpoint_the_router_serves() {
        for endpoint in ["/api/config", "/api/runs", "/log?after=", "/decision"] {
            assert!(PAGE.contains(endpoint), "page never calls {endpoint}");
        }
    }

    #[test]
    fn plan_only_defaults_to_checked() {
        // The safe option has to be the default, not a thing you remember.
        assert!(PAGE.contains(r#"id="planOnly" checked"#));
    }

    #[test]
    fn user_supplied_text_is_escaped_before_it_reaches_the_dom() {
        assert!(PAGE.contains("const esc ="));

        // Every field a model wrote goes through it before reaching innerHTML.
        // Findings are agent output; a reviewer that emits markup must not be
        // able to rewrite the page the human is deciding on.
        for field in [
            "esc(f.problem)",
            "esc(f.evidence)",
            "esc(f.recommendation)",
            "esc(f.reviewer)",
            "esc(d.reason)",
        ] {
            assert!(PAGE.contains(field), "{field} reaches the DOM unescaped");
        }
    }
}
