"use strict";

const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

// ---------- forced auto-update ----------
// Backend checks on launch and installs+relaunches automatically. We just show
// a blocking overlay so the app can't be used mid-update.
function wireUpdater() {
  const overlay = $("update-overlay");
  const bar = $("update-bar");
  const pct = $("update-pct");
  const msg = $("update-msg");
  listen("update://available", (e) => {
    overlay.hidden = false;
    msg.textContent = `Installing required update${e.payload ? " " + e.payload : ""}. The app will restart automatically.`;
  });
  listen("update://progress", (e) => {
    overlay.hidden = false;
    const [done, total] = e.payload || [];
    if (total) {
      const p = Math.min(100, Math.round((done / total) * 100));
      bar.style.width = p + "%";
      pct.textContent = p + "%";
    } else {
      pct.textContent = "Downloading…";
    }
  });
  // update://error is non-fatal (offline, no release yet, bad signature, failed
  // file swap). Always dismiss the blocking overlay so the app stays usable.
  listen("update://error", () => {
    overlay.hidden = true;
  });
}

const TYPE_NAMES = {
  ukpga: "UK Public General Act",
  uksi: "UK Statutory Instrument",
  asp: "Act of the Scottish Parliament",
  asc: "Act of Senedd Cymru",
  anaw: "Act of the National Assembly for Wales",
  wsi: "Wales Statutory Instrument",
  ssi: "Scottish Statutory Instrument",
  nia: "Northern Ireland Act",
  nisr: "Northern Ireland Statutory Rule",
};

const state = { query: "", offset: 0, limit: 10, total: 0, settings: null, lastQuery: null };

const $ = (id) => document.getElementById(id);

function legUrl(id) {
  return "https://www.legislation.gov.uk/" + String(id || "").replace(/^https?:\/\/[^/]+\/(id\/)?/, "");
}
function esc(s) {
  return String(s == null ? "" : s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

// ---------- panel ----------
function openPanel(title, html) {
  $("panel-title").textContent = title;
  $("panel-body").innerHTML = html;
  $("panel").hidden = false;
  $("scrim").hidden = false;
}
function closePanel() {
  $("panel").hidden = true;
  $("scrim").hidden = true;
}

// ---------- search ----------
async function runSearch(reset) {
  const q = $("q").value.trim();
  if (!q) return;
  if (reset) {
    state.offset = 0;
    $("results").innerHTML = "";
  }
  state.query = q;
  state.limit = parseInt($("f-limit").value, 10) || 10;

  const params = {
    query: q,
    offset: state.offset,
    limit: state.limit,
    include_text: false,
  };
  const type = $("f-type").value;
  if (type) params.legislation_type = [type];
  const from = parseInt($("f-from").value, 10);
  const to = parseInt($("f-to").value, 10);
  if (!isNaN(from)) params.year_from = from;
  if (!isNaN(to)) params.year_to = to;

  const meta = $("search-meta");
  meta.hidden = false;
  meta.innerHTML = '<span class="spinner">Searching Lex…</span>';
  $("results-foot").hidden = true;

  try {
    const data = await invoke("lex_search", { params });
    state.total = data.total || 0;
    const results = data.results || [];
    meta.textContent = `${state.total.toLocaleString()} matching item(s) · showing ${Math.min(state.offset + results.length, state.total)}`;
    renderResults(results, state.offset === 0);
    state.offset += results.length;
    $("results-foot").hidden = !(state.offset < state.total && results.length > 0);
  } catch (e) {
    meta.hidden = true;
    if (state.offset === 0) {
      $("results").innerHTML = `<div class="error">${esc(e)}</div>`;
    } else {
      $("results").insertAdjacentHTML("beforeend", `<div class="error">${esc(e)}</div>`);
    }
  }
}

function renderResults(results, isFirst) {
  const wrap = $("results");
  if (isFirst && results.length === 0) {
    wrap.innerHTML = '<div class="empty">No legislation matched. Try broader terms or remove filters.</div>';
    return;
  }
  for (const r of results) {
    const id = r.id || r.uri || "";
    const typeName = TYPE_NAMES[r.type] || (r.type ? String(r.type).toUpperCase() : "Legislation");
    const yearNum = [r.year, r.number].filter((x) => x != null && x !== "").join(" No. ");
    const topScore = Array.isArray(r.sections) && r.sections[0] ? r.sections[0].score : null;
    const card = document.createElement("div");
    card.className = "card";
    card.innerHTML = `
      <h3>${esc(r.title) || "(untitled)"}</h3>
      <div class="sub">
        <span class="badge">${esc(typeName)}</span>
        ${yearNum ? `<span class="badge">${esc(yearNum)}</span>` : ""}
        ${r.status && r.status !== "stub" ? `<span class="badge">${esc(r.status)}</span>` : ""}
        ${topScore != null ? `<span class="badge">match ${(topScore * 100).toFixed(0)}%</span>` : ""}
      </div>
      ${r.description ? `<p class="desc">${esc(String(r.description).slice(0, 320))}${String(r.description).length > 320 ? "…" : ""}</p>` : ""}
      <div class="actions">
        <button class="btn btn-link" data-open="${esc(legUrl(id))}">Open on legislation.gov.uk ↗</button>
        <button class="btn btn-ghost" data-fulltext="${esc(id)}">Full text</button>
        <button class="btn btn-ghost" data-amend="${esc(id)}" data-title="${esc(r.title)}">Amendments</button>
        <button class="btn btn-ghost" data-notes="${esc(id)}" data-title="${esc(r.title)}">Explanatory notes</button>
        <button class="btn btn-ghost" data-explain="${esc(id)}" data-title="${esc(r.title)}">Explain with AI</button>
      </div>`;
    wrap.appendChild(card);
  }
}

// ---------- full text & explain ----------
async function showFullText(id) {
  openPanel("Full text", '<div class="spinner">Fetching full text from Lex…</div>');
  try {
    const data = await invoke("lex_full_text", { legislationId: id, includeSchedules: false });
    const title = (data.legislation && data.legislation.title) || id;
    const text = data.full_text || "(no text returned)";
    openPanel(title, `
      <p><button class="btn btn-link" data-open="${esc(legUrl(id))}">Open on legislation.gov.uk ↗</button></p>
      <pre>${esc(text)}</pre>`);
  } catch (e) {
    openPanel("Full text", `<div class="error">${esc(e)}</div>`);
  }
}

async function showAmendments(id, title) {
  openPanel(`Amendments · ${title}`, '<div class="spinner">Fetching amendments from Lex…</div>');
  try {
    const list = await invoke("lex_amendments", { legislationId: id, searchAmended: true });
    const items = Array.isArray(list) ? list : [];
    if (!items.length) {
      openPanel(`Amendments · ${title}`, '<div class="empty">No recorded amendments to this legislation in Lex.</div>');
      return;
    }
    const rows = items.map((a) => {
      const affecting = [a.affecting_legislation, a.affecting_year, a.affecting_number].filter(Boolean).join(" ");
      const url = a.affecting_provision_url || a.affecting_url;
      return `<div class="card">
        <div class="sub">${a.type_of_effect ? `<span class="badge">${esc(a.type_of_effect)}</span>` : ""}
          ${a.changed_provision ? `<span class="badge">affects ${esc(a.changed_provision)}</span>` : ""}</div>
        ${affecting ? `<p class="desc">By: ${esc(affecting)}${a.affecting_provision ? " — " + esc(a.affecting_provision) : ""}</p>` : ""}
        ${a.ai_explanation ? `<p class="desc">${esc(a.ai_explanation)}</p>` : ""}
        ${url ? `<button class="btn btn-link" data-open="${esc(url)}">View amending provision ↗</button>` : ""}
      </div>`;
    }).join("");
    openPanel(`Amendments · ${title} (${items.length})`, rows);
  } catch (e) {
    openPanel("Amendments", `<div class="error">${esc(e)}</div>`);
  }
}

async function showNotes(id, title) {
  openPanel(`Explanatory notes · ${title}`, '<div class="spinner">Fetching explanatory notes from Lex…</div>');
  try {
    const list = await invoke("lex_explanatory_notes", { legislationId: id });
    const items = Array.isArray(list) ? list : [];
    if (!items.length) {
      openPanel(`Explanatory notes · ${title}`, '<div class="empty">No explanatory notes for this legislation in Lex.</div>');
      return;
    }
    items.sort((a, b) => (a.order || 0) - (b.order || 0));
    const rows = items.map((n) => {
      const heading = Array.isArray(n.route) && n.route.length ? n.route.join(" › ") : (n.note_type || "Note");
      return `<div class="card">
        <div class="sub"><span class="badge">${esc(heading)}</span>${n.section_number != null ? `<span class="badge">s.${esc(n.section_number)}</span>` : ""}</div>
        <pre>${esc(n.text || "")}</pre>
      </div>`;
    }).join("");
    openPanel(`Explanatory notes · ${title} (${items.length})`, rows);
  } catch (e) {
    openPanel("Explanatory notes", `<div class="error">${esc(e)}</div>`);
  }
}

async function explain(id, title) {
  if (!state.settings || !state.settings.llm_enabled) {
    openPanel("Explain with AI", `<div class="empty">AI explanations are off. Open <b>Settings</b>, enable them and add your own API key (or run a local model with Ollama). Curia needs no subscription.</div>`);
    return;
  }
  openPanel("Explain with AI", '<div class="spinner">Reading the legislation and asking your model…</div>');
  try {
    // Ground the model in the actual text rather than its own memory.
    let excerpt = "";
    try {
      const ft = await invoke("lex_full_text", { legislationId: id, includeSchedules: false });
      excerpt = (ft.full_text || "").slice(0, 8000);
    } catch (_) { /* fall back to title only */ }
    const context = `Act/SI: ${title} (${id})\nSource: ${legUrl(id)}\n\n${excerpt || "(full text unavailable; answer only if you can from the title, otherwise say so)"}`;
    const question = state.query
      ? `In relation to: "${state.query}" — what does this legislation say, and which sections are relevant?`
      : "Summarise what this legislation covers and its key provisions.";
    const answer = await invoke("llm_explain", { question, context });
    openPanel(`Explain · ${title}`, `
      <p><button class="btn btn-link" data-open="${esc(legUrl(id))}">Open source ↗</button></p>
      <div class="answer">${esc(answer)}</div>
      <p class="hint">AI-generated from the retrieved text. Research aid, not legal advice — verify against the source.</p>`);
  } catch (e) {
    openPanel("Explain with AI", `<div class="error">${esc(e)}</div>`);
  }
}

// ---------- settings ----------
function fillSettings(s) {
  $("s-lex-url").value = s.lex_base_url || "";
  $("s-contact").value = s.contact || "";
  $("s-llm-enabled").checked = !!s.llm_enabled;
  $("s-provider").value = s.provider || "openai";
  $("s-base-url").value = s.base_url || "";
  $("s-model").value = s.model || "";
  // The key itself never comes back from the backend (it lives in Windows
  // Credential Manager); we only learn whether one is saved.
  $("s-api-key").value = "";
  $("s-api-key").placeholder = s.api_key_set ? "Saved — leave blank to keep" : "sk-…";
  $("clear-api-key").hidden = !s.api_key_set;
  syncProviderRows();
}
function syncProviderRows() {
  const p = $("s-provider").value;
  $("row-baseurl").hidden = !(p === "openai_compatible" || p === "ollama");
  $("row-apikey").hidden = p === "ollama";
}
async function saveSettings() {
  const settings = {
    lex_base_url: $("s-lex-url").value.trim() || "https://lex.lab.i.ai.gov.uk",
    contact: $("s-contact").value.trim(),
    llm_enabled: $("s-llm-enabled").checked,
    provider: $("s-provider").value,
    base_url: $("s-base-url").value.trim(),
    model: $("s-model").value.trim(),
    // Blank means "keep the saved key".
    api_key: $("s-api-key").value.trim(),
  };
  const saved = $("settings-saved");
  try {
    await invoke("save_settings", { settings });
    state.settings = await invoke("get_settings");
    fillSettings(state.settings);
    saved.textContent = "Saved ✓";
    saved.classList.remove("saved-error");
  } catch (e) {
    saved.textContent = String(e);
    saved.classList.add("saved-error");
  }
  saved.hidden = false;
  setTimeout(() => (saved.hidden = true), 4000);
  checkHealth();
}
async function clearApiKey() {
  try {
    await invoke("clear_api_key");
    state.settings = await invoke("get_settings");
    fillSettings(state.settings);
  } catch (e) {
    const saved = $("settings-saved");
    saved.textContent = String(e);
    saved.classList.add("saved-error");
    saved.hidden = false;
  }
}

const VIEWS = ["assistant", "search", "guidance", "caselaw", "updates", "settings"];
let currentTab = "assistant";
function showView(which) {
  for (const v of VIEWS) $("view-" + v).hidden = v !== which;
  document.querySelectorAll(".tab").forEach((t) => t.classList.toggle("active", t.dataset.tab === which));
  if (which !== "settings") currentTab = which;
  if (which === "updates") loadUpdates(false);
  if (which === "assistant") syncAiNotice();
}

// ---------- health ----------
async function checkHealth() {
  const pill = $("status-pill");
  pill.className = "pill pill-muted";
  pill.textContent = "checking…";
  try {
    const h = await invoke("lex_health");
    const ok = h && h.status === "healthy";
    pill.className = "pill " + (ok ? "pill-ok" : "pill-bad");
    pill.textContent = ok ? "Lex online" : "Lex degraded";
  } catch (_) {
    pill.className = "pill pill-bad";
    pill.textContent = "Lex offline";
  }
}

// ---------- wiring ----------
document.addEventListener("click", (ev) => {
  const t = ev.target.closest("[data-open],[data-ext],[data-fulltext],[data-amend],[data-notes],[data-explain],[data-cite],[data-gread],[data-cread],[data-watch],[data-unwatch],[data-history],[data-diff],[data-summarise],[data-tab],[data-example]");
  if (!t) return;
  if (t.dataset.tab) { showView(t.dataset.tab); return; }
  if (t.dataset.cite) { showCitation(Number(t.dataset.cite)); return; }
  if (t.dataset.gread) { readGuidance(t.dataset.gread, t.dataset.q || "", t.dataset.title || ""); return; }
  if (t.dataset.cread) { readJudgment(t.dataset.cread, t.dataset.q || "", t.dataset.title || ""); return; }
  if (t.dataset.watch) { watchDoc(t.dataset.watch, t); return; }
  if (t.dataset.unwatch) { unwatchDoc(t.dataset.unwatch); return; }
  if (t.dataset.history) { showHistory(t.dataset.history, t); return; }
  if (t.dataset.diff) { toggleDiff(t.dataset.diff); return; }
  if (t.dataset.summarise) { summariseChange(t.dataset.summarise, t); return; }
  if (t.dataset.open) { ev.preventDefault(); invoke("open_url", { url: t.dataset.open }); }
  else if (t.dataset.ext) { ev.preventDefault(); invoke("open_url", { url: t.dataset.ext }); }
  else if (t.dataset.fulltext) { showFullText(t.dataset.fulltext); }
  else if (t.dataset.amend) { showAmendments(t.dataset.amend, t.dataset.title || ""); }
  else if (t.dataset.notes) { showNotes(t.dataset.notes, t.dataset.title || ""); }
  else if (t.dataset.explain) { explain(t.dataset.explain, t.dataset.title || ""); }
});

window.addEventListener("DOMContentLoaded", async () => {
  $("search-form").addEventListener("submit", (e) => { e.preventDefault(); runSearch(true); });
  $("load-more").addEventListener("click", () => runSearch(false));
  $("open-settings").addEventListener("click", () => showView("settings"));
  $("close-settings").addEventListener("click", () => showView(currentTab));
  $("save-settings").addEventListener("click", saveSettings);
  $("clear-api-key").addEventListener("click", clearApiKey);
  $("s-provider").addEventListener("change", syncProviderRows);
  $("panel-close").addEventListener("click", closePanel);
  $("scrim").addEventListener("click", closePanel);

  wireUpdater();
  wireNewTabs();

  try {
    state.settings = await invoke("get_settings");
    fillSettings(state.settings);
  } catch (e) {
    state.settings = { llm_enabled: false };
  }
  checkHealth();
  showView("assistant");
  $("ask-q").focus();
});

// =====================================================================
// v0.2: Assistant, Guidance & Rules, Case law, Updates
// =====================================================================

function fmtDate(s) {
  if (!s) return "";
  const d = new Date(s);
  return isNaN(d) ? String(s).slice(0, 10) : d.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}
function todayLong() {
  return new Date().toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long", year: "numeric" });
}

// ---------- tiny, safe markdown (escape first, then format) ----------
function citeChips(html) {
  // [3], [3, 5], [3–6]
  return html.replace(/\[(\d+(?:\s*[,–-]\s*\d+)*)\]/g, (m, inner) => {
    const nums = [];
    for (const part of inner.split(",")) {
      const r = part.split(/[–-]/).map((x) => parseInt(x.trim(), 10));
      if (r.length === 2 && !isNaN(r[0]) && !isNaN(r[1]) && r[1] >= r[0] && r[1] - r[0] < 30) {
        for (let i = r[0]; i <= r[1]; i++) nums.push(i);
      } else if (!isNaN(r[0])) nums.push(r[0]);
    }
    return nums.map((n) => `<button class="cite" type="button" data-cite="${n}" title="Show source ${n}">${n}</button>`).join("");
  });
}
function inline(s) {
  return citeChips(esc(s))
    .replace(/\*\*(.+?)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^*])\*(?!\s)(.+?)\*(?!\*)/g, "$1<i>$2</i>")
    .replace(/`([^`]+)`/g, "<code>$1</code>");
}
function renderMarkdown(md) {
  const lines = String(md || "").replace(/\r/g, "").split("\n");
  let out = "", list = null, para = [];
  const flushPara = () => { if (para.length) { out += `<p>${inline(para.join(" "))}</p>`; para = []; } };
  const closeList = () => { if (list) { out += `</${list}>`; list = null; } };
  for (const raw of lines) {
    const line = raw.trimEnd();
    let m;
    if (!line.trim()) { flushPara(); closeList(); continue; }
    if ((m = line.match(/^#{1,6}\s+(.*)$/))) { flushPara(); closeList(); out += `<h4>${inline(m[1])}</h4>`; continue; }
    if ((m = line.match(/^\s*[-*•]\s+(.*)$/))) { flushPara(); if (list !== "ul") { closeList(); out += "<ul>"; list = "ul"; } out += `<li>${inline(m[1])}</li>`; continue; }
    if ((m = line.match(/^\s*\d+[.)]\s+(.*)$/))) { flushPara(); if (list !== "ol") { closeList(); out += "<ol>"; list = "ol"; } out += `<li>${inline(m[1])}</li>`; continue; }
    closeList();
    para.push(line.trim());
  }
  flushPara(); closeList();
  return out;
}

// ---------- assistant ----------
const chat = { turns: [], sources: new Map(), nextSource: 1, busy: false };

function syncAiNotice() {
  const on = !!(state.settings && state.settings.llm_enabled);
  $("ai-off").hidden = on;
}

function renderSourceList(list) {
  if (!list || !list.length) return "";
  const rows = list.map((s) => `
    <div class="src" id="src-${s.n}">
      <span class="src-n">[${s.n}]</span>
      <div class="src-body">
        <span class="badge">${esc(s.kind)}</span>
        <span class="src-title">${esc(s.title)}</span>
        ${s.detail ? `<span class="src-detail"> — ${esc(s.detail)}</span>` : ""}
        <div><button class="btn btn-link" type="button" data-open="${esc(s.url)}">Open at source ↗</button>
          <button class="btn btn-ghost" type="button" data-cite="${s.n}">Show extract</button></div>
      </div>
    </div>`).join("");
  return `<div class="sources"><h5>Sources</h5>${rows}</div>`;
}

function appendMsg(role, html, cls = "") {
  $("chat-empty").hidden = true;
  const div = document.createElement("div");
  div.className = `msg msg-${role} ${cls}`;
  div.innerHTML = html;
  $("chat").appendChild(div);
  div.scrollIntoView({ behavior: "smooth", block: "start" });
  return div;
}

async function ask(question) {
  question = question.trim();
  if (!question || chat.busy) return;
  if (!state.settings || !state.settings.llm_enabled) { syncAiNotice(); showView("settings"); return; }
  chat.busy = true;
  $("ask-btn").disabled = true;
  $("ask-q").value = "";
  const userDiv = appendMsg("user", esc(question));
  const pending = appendMsg("assistant", '<div class="thinking spinner">Researching — searching the Rules, guidance, legislation and case law… this can take 20–60 seconds.</div>');
  try {
    const res = await invoke("assistant_ask", {
      req: {
        history: chat.turns.map((t) => ({ role: t.role, content: t.content })),
        question,
        first_source: chat.nextSource,
        today: todayLong(),
      },
    });
    for (const s of res.sources) chat.sources.set(s.n, s);
    chat.nextSource = res.next_source;
    chat.turns.push({ role: "user", content: question });
    chat.turns.push({ role: "assistant", content: res.answer });
    const steps = res.steps.length
      ? `<details class="steps"><summary>Research steps (${res.steps.length}) · ${res.retrieved} passages read</summary><ol>${res.steps
          .map((st) => `<li class="${st.ok ? "" : "fail"}">${esc(st.summary)}${st.ok ? "" : " — failed"}</li>`).join("")}</ol></details>`
      : "";
    pending.innerHTML = renderMarkdown(res.answer) + renderSourceList(res.sources) + steps;
  } catch (e) {
    pending.className = "msg msg-assistant msg-error";
    pending.innerHTML = esc(String(e));
    userDiv.remove();
    $("ask-q").value = question;
  } finally {
    chat.busy = false;
    $("ask-btn").disabled = false;
  }
}

function showCitation(n) {
  const s = chat.sources.get(n);
  if (!s) return;
  openPanel(`[${n}] ${s.title}`, `
    <div class="reader-head"><span class="badge">${esc(s.kind)}</span>${s.detail ? esc(s.detail) : ""}</div>
    <div class="passage"><p>${esc(s.excerpt)}</p></div>
    <p><button class="btn btn-link" type="button" data-open="${esc(s.url)}">Open at this point in the source ↗</button></p>
    <p class="hint">Extract as retrieved by Curia. Always read the full source and check it is current.</p>`);
}

function newChat() {
  chat.turns = []; chat.sources.clear(); chat.nextSource = 1;
  $("chat").innerHTML = "";
  $("chat-empty").hidden = false;
  $("ask-q").focus();
}

// ---------- reader panel ----------
function renderPassages(passages, emptyMsg) {
  if (!passages.length) return `<div class="empty">${esc(emptyMsg)}</div>`;
  return passages.map((p) => `
    <div class="passage">
      ${p.heading ? `<div class="ph">${esc(p.heading)}</div>` : ""}
      <p>${esc(p.text)}</p>
      <button class="btn btn-link" type="button" data-open="${esc(p.url)}">Open here ↗</button>
    </div>`).join("");
}

async function readGuidance(path, q, title) {
  openPanel(title || "Guidance", '<div class="spinner">Fetching from GOV.UK…</div>');
  try {
    const r = await invoke("guidance_read", { path, query: q });
    const d = r.doc;
    const latest = d.change_history && d.change_history[0];
    const head = `
      <div class="reader-head"><span class="badge">${esc(d.kind)}</span> Last updated ${esc(fmtDate(d.updated))}
        · <button class="btn btn-link" type="button" data-open="${esc(d.url)}">Open on GOV.UK ↗</button>
        <button class="btn btn-ghost" type="button" data-watch="${esc(d.path)}">Watch for changes</button>
        ${q ? `<button class="btn btn-ghost" type="button" data-gread="${esc(d.path)}" data-q="" data-title="${esc(d.title)}">Show full text</button>` : ""}
      </div>
      ${latest && latest.note ? `<div class="change-note"><b>Latest change (${esc(fmtDate(latest.timestamp))}):</b> ${esc(latest.note)}</div>` : ""}
      ${d.pdfs.length ? `<p class="hint">Also published as PDF: ${d.pdfs.map((u) => `<button class="btn btn-link" type="button" data-open="${esc(u)}">PDF ↗</button>`).join(" ")}</p>` : ""}
      ${q ? `<p class="hint">Passages most relevant to “${esc(q)}”:</p>` : ""}`;
    openPanel(d.title, head + renderPassages(r.passages, d.pdfs.length ? "This document is only published as PDF — open it above." : "No text found."));
  } catch (e) {
    openPanel(title || "Guidance", `<div class="error">${esc(e)}</div>`);
  }
}

async function readJudgment(uri, q, title) {
  openPanel(title || "Judgment", '<div class="spinner">Fetching judgment from Find Case Law…</div>');
  try {
    const r = await invoke("caselaw_read", { uri, query: q });
    const j = r.doc;
    const head = `
      <div class="reader-head">${esc(j.citation)} · ${j.paragraphs} paragraphs
        · <button class="btn btn-link" type="button" data-open="${esc(j.url)}">Open on Find Case Law ↗</button>
        ${q ? `<button class="btn btn-ghost" type="button" data-cread="${esc(j.uri)}" data-q="" data-title="${esc(j.title)}">Show full judgment</button>` : ""}
      </div>
      ${q ? `<p class="hint">Paragraphs most relevant to “${esc(q)}”:</p>` : ""}`;
    openPanel(j.title || title, head + renderPassages(r.passages, "No text found."));
  } catch (e) {
    openPanel(title || "Judgment", `<div class="error">${esc(e)}</div>`);
  }
}

// ---------- guidance & rules search ----------
async function guidanceSearch() {
  const q = $("g-q").value.trim();
  if (!q) return;
  const meta = $("g-meta");
  meta.hidden = false;
  meta.innerHTML = '<span class="spinner">Searching GOV.UK…</span>';
  $("g-results").innerHTML = "";
  try {
    const hits = await invoke("guidance_search", { query: q, scope: $("g-scope").value });
    meta.textContent = `${hits.length} result(s)`;
    if (!hits.length) { $("g-results").innerHTML = '<div class="empty">Nothing matched. Try other words or a wider scope.</div>'; return; }
    $("g-results").innerHTML = hits.map((h) => `
      <div class="card">
        <h3>${esc(h.title)}</h3>
        <div class="sub"><span class="badge">${esc(h.kind)}</span><span class="badge">updated ${esc(fmtDate(h.updated))}</span></div>
        ${h.description ? `<p class="desc">${esc(h.description)}</p>` : ""}
        <div class="actions">
          <button class="btn btn-ghost" type="button" data-gread="${esc(h.path)}" data-q="${esc(q)}" data-title="${esc(h.title)}">Relevant passages</button>
          <button class="btn btn-ghost" type="button" data-gread="${esc(h.path)}" data-q="" data-title="${esc(h.title)}">Read</button>
          <button class="btn btn-link" type="button" data-open="${esc(h.url)}">Open on GOV.UK ↗</button>
          <button class="btn btn-ghost" type="button" data-watch="${esc(h.path)}">Watch</button>
        </div>
      </div>`).join("");
  } catch (e) {
    meta.hidden = true;
    $("g-results").innerHTML = `<div class="error">${esc(e)}</div>`;
  }
}

// ---------- case law search ----------
const cl = { q: "", courts: "", page: 1 };
async function caselawSearch(reset) {
  if (reset) {
    cl.q = $("c-q").value.trim();
    cl.courts = $("c-courts").value;
    cl.page = 1;
    $("c-results").innerHTML = "";
  }
  if (!cl.q) return;
  const meta = $("c-meta");
  meta.hidden = false;
  meta.innerHTML = '<span class="spinner">Searching Find Case Law…</span>';
  $("c-foot").hidden = true;
  try {
    const cases = await invoke("caselaw_search", { query: cl.q, courts: cl.courts, page: cl.page });
    meta.textContent = "Ranked by relevance";
    if (!cases.length && cl.page === 1) {
      $("c-results").innerHTML = '<div class="empty">No judgments contain all of those words. Try fewer or different words.</div>';
      return;
    }
    $("c-results").insertAdjacentHTML("beforeend", cases.map((c) => `
      <div class="card">
        <h3>${esc(c.title)}</h3>
        <div class="sub">${c.citation ? `<span class="badge">${esc(c.citation)}</span>` : ""}<span class="badge">${esc(c.court)}</span><span class="badge">${esc(fmtDate(c.date))}</span></div>
        <div class="actions">
          <button class="btn btn-ghost" type="button" data-cread="${esc(c.uri)}" data-q="${esc(cl.q)}" data-title="${esc(c.title)}">Relevant paragraphs</button>
          <button class="btn btn-ghost" type="button" data-cread="${esc(c.uri)}" data-q="" data-title="${esc(c.title)}">Read</button>
          <button class="btn btn-link" type="button" data-open="${esc(c.url)}">Open on Find Case Law ↗</button>
        </div>
      </div>`).join(""));
    cl.page += 1;
    $("c-foot").hidden = cases.length < 20;
  } catch (e) {
    meta.hidden = true;
    $("c-results").insertAdjacentHTML("beforeend", `<div class="error">${esc(e)}</div>`);
  }
}

// ---------- updates ----------
let tracker = { watched: [], changes: [], last_checked: "" };
let recentLoaded = false;

function renderTracker() {
  const w = tracker.watched || [];
  $("u-watched").innerHTML = w.length
    ? w.map((d) => `
      <div class="card">
        <h3>${esc(d.title)}</h3>
        <div class="sub"><span class="badge">last updated ${esc(fmtDate(d.updated))}</span></div>
        <div class="actions">
          <button class="btn btn-ghost" type="button" data-gread="${esc(d.path)}" data-q="" data-title="${esc(d.title)}">Read</button>
          <button class="btn btn-ghost" type="button" data-history="${esc(d.path)}">Change notes</button>
          <button class="btn btn-ghost" type="button" data-unwatch="${esc(d.path)}">Stop watching</button>
        </div>
        <div class="history"></div>
      </div>`).join("")
    : '<div class="empty">Nothing watched yet. Use <b>Watch</b> on a document below or in Guidance &amp; Rules.</div>';

  const ch = tracker.changes || [];
  $("u-changes").innerHTML = ch.length
    ? ch.map((c, i) => `
      <div class="card" id="chg-${i}">
        <h3>${esc(c.title)}</h3>
        <div class="sub"><span class="badge">${esc(fmtDate(c.from))} → ${esc(fmtDate(c.to))}</span>
          <span class="badge">+${c.added} / −${c.removed} lines</span></div>
        ${c.note ? `<div class="change-note"><b>GOV.UK change note:</b> ${esc(c.note)}</div>` : ""}
        <div class="summary">${c.summary ? "<b>AI summary:</b> " + esc(c.summary) : ""}</div>
        <div class="actions">
          <button class="btn btn-ghost" type="button" data-diff="${i}">Show changes</button>
          <button class="btn btn-ghost" type="button" data-summarise="${esc(c.id)}">${c.summary ? "Re-summarise" : "Summarise with AI"}</button>
          <button class="btn btn-link" type="button" data-open="https://www.gov.uk${esc(c.path)}">Open on GOV.UK ↗</button>
        </div>
        <div class="diff" hidden></div>
      </div>`).join("")
    : '<div class="empty">No changes detected yet. Changes appear here when a watched document is updated.</div>';

  $("u-last").textContent = tracker.last_checked ? `Last checked ${tracker.last_checked}` : "";
  const fresh = ch.filter((c) => !c.summary).length;
  $("updates-badge").hidden = !fresh;
  $("updates-badge").textContent = fresh;
}

async function loadUpdates(force) {
  try { tracker = await invoke("tracker_state"); } catch (_) {}
  renderTracker();
  if (recentLoaded && !force) return;
  $("u-recent").innerHTML = '<div class="spinner">Loading latest guidance updates from GOV.UK…</div>';
  try {
    const hits = await invoke("tracker_recent", { count: 40 });
    recentLoaded = true;
    const watched = new Set((tracker.watched || []).map((d) => d.path));
    $("u-recent").innerHTML = hits.map((h) => `
      <div class="card">
        <h3>${esc(h.title)}</h3>
        <div class="sub"><span class="badge">${esc(h.kind)}</span><span class="badge">updated ${esc(fmtDate(h.updated))}</span></div>
        <div class="actions">
          <button class="btn btn-ghost" type="button" data-history="${esc(h.path)}">What changed?</button>
          <button class="btn btn-ghost" type="button" data-gread="${esc(h.path)}" data-q="" data-title="${esc(h.title)}">Read</button>
          <button class="btn btn-link" type="button" data-open="${esc(h.url)}">Open on GOV.UK ↗</button>
          ${watched.has(h.path) ? '<span class="badge">watching</span>' : `<button class="btn btn-ghost" type="button" data-watch="${esc(h.path)}">Watch</button>`}
        </div>
        <div class="history"></div>
      </div>`).join("");
  } catch (e) {
    $("u-recent").innerHTML = `<div class="error">${esc(e)}</div>`;
  }
}

async function showHistory(path, btn) {
  const box = btn.closest(".card").querySelector(".history");
  if (!box.hidden && box.innerHTML) { box.innerHTML = ""; return; }
  box.innerHTML = '<div class="spinner">Fetching change notes…</div>';
  try {
    const hist = await invoke("tracker_history", { path });
    box.innerHTML = hist.length
      ? hist.slice(0, 5).map((c) => `<div class="change-note"><b>${esc(fmtDate(c.timestamp))}:</b> ${esc(c.note)}</div>`).join("")
      : '<div class="hint">No change notes published.</div>';
  } catch (e) {
    box.innerHTML = `<div class="error">${esc(e)}</div>`;
  }
}

async function watchDoc(path, btn) {
  const label = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Adding…";
  try {
    tracker = await invoke("tracker_watch", { path });
    btn.textContent = "Watching ✓";
    renderTracker();
  } catch (e) {
    btn.disabled = false;
    btn.textContent = label;
    alertInline(btn, e);
  }
}
async function unwatchDoc(path) {
  try { tracker = await invoke("tracker_unwatch", { path }); renderTracker(); } catch (e) { /* ignore */ }
}
function alertInline(el, e) {
  const n = document.createElement("div");
  n.className = "error";
  n.textContent = String(e);
  el.closest(".card, .reader-head, .panel-body")?.appendChild(n);
}

async function checkNow() {
  const b = $("u-check");
  b.disabled = true;
  b.textContent = "Checking…";
  try {
    const r = await invoke("tracker_check", { now: new Date().toLocaleString("en-GB") });
    await loadUpdates(false);
    $("u-last").textContent = `Checked ${r.checked} · ${r.changed} changed${r.errors.length ? " · " + r.errors.length + " error(s)" : ""}`;
  } catch (e) {
    $("u-last").textContent = String(e);
  } finally {
    b.disabled = false;
    b.textContent = "Check watched now";
  }
}

function toggleDiff(i) {
  const card = $("chg-" + i);
  const box = card.querySelector(".diff");
  if (!box.hidden) { box.hidden = true; return; }
  const c = tracker.changes[Number(i)];
  box.innerHTML = c.diff.length
    ? c.diff.map((l) => {
        if (l.tag === "…") return '<div class="gap">⋯</div>';
        const cls = l.tag === "+" ? "add" : l.tag === "-" ? "del" : "";
        return `<div class="${cls}">${esc(l.tag === " " ? "  " : l.tag + " ")}${esc(l.text)}</div>`;
      }).join("")
    : '<div class="gap">No text differences (metadata-only update).</div>';
  box.hidden = false;
}

async function summariseChange(id, btn) {
  if (!state.settings || !state.settings.llm_enabled) { showView("settings"); return; }
  btn.disabled = true;
  btn.textContent = "Summarising…";
  try {
    await invoke("tracker_summarise", { id });
    tracker = await invoke("tracker_state");
    renderTracker();
  } catch (e) {
    btn.disabled = false;
    btn.textContent = "Summarise with AI";
    alertInline(btn, e);
  }
}

// ---------- wiring ----------
function wireNewTabs() {
  $("ask-form").addEventListener("submit", (e) => { e.preventDefault(); ask($("ask-q").value); });
  $("ask-q").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask($("ask-q").value); }
  });
  $("new-chat").addEventListener("click", newChat);
  document.querySelectorAll(".example").forEach((b) => b.addEventListener("click", () => ask(b.textContent)));
  $("g-form").addEventListener("submit", (e) => { e.preventDefault(); guidanceSearch(); });
  $("c-form").addEventListener("submit", (e) => { e.preventDefault(); caselawSearch(true); });
  $("c-more").addEventListener("click", () => caselawSearch(false));
  $("u-check").addEventListener("click", checkNow);
  // Badge on start-up: show unsummarised detected changes.
  invoke("tracker_state").then((t) => { tracker = t; renderTracker(); }).catch(() => {});
}
