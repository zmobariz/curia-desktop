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
  // update://error is non-fatal (offline / not configured) — stay silent and let the app run.
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
  $("s-api-key").value = s.api_key || "";
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
    api_key: $("s-api-key").value,
  };
  await invoke("save_settings", { settings });
  state.settings = settings;
  const saved = $("settings-saved");
  saved.hidden = false;
  setTimeout(() => (saved.hidden = true), 1500);
  checkHealth();
}
function showView(which) {
  $("view-search").hidden = which !== "search";
  $("view-settings").hidden = which !== "settings";
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
  const t = ev.target.closest("[data-open],[data-ext],[data-fulltext],[data-explain]");
  if (!t) return;
  if (t.dataset.open) { ev.preventDefault(); invoke("open_url", { url: t.dataset.open }); }
  else if (t.dataset.ext) { ev.preventDefault(); invoke("open_url", { url: t.dataset.ext }); }
  else if (t.dataset.fulltext) { showFullText(t.dataset.fulltext); }
  else if (t.dataset.explain) { explain(t.dataset.explain, t.dataset.title || ""); }
});

window.addEventListener("DOMContentLoaded", async () => {
  $("search-form").addEventListener("submit", (e) => { e.preventDefault(); runSearch(true); });
  $("load-more").addEventListener("click", () => runSearch(false));
  $("open-settings").addEventListener("click", () => showView("settings"));
  $("close-settings").addEventListener("click", () => showView("search"));
  $("save-settings").addEventListener("click", saveSettings);
  $("s-provider").addEventListener("change", syncProviderRows);
  $("panel-close").addEventListener("click", closePanel);
  $("scrim").addEventListener("click", closePanel);

  wireUpdater();

  try {
    state.settings = await invoke("get_settings");
    fillSettings(state.settings);
  } catch (e) {
    state.settings = { llm_enabled: false };
  }
  checkHealth();
  $("q").focus();
});
