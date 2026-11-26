//! The Curia Assistant: a research chat that searches primary sources live and
//! answers with numbered citations that link to the exact place in the source.
//!
//! The model never answers from memory alone. It is given tools that search
//! legislation (Lex), the Immigration Rules and Home Office guidance (GOV.UK)
//! and case law (Find Case Law); every passage it sees is registered as a
//! numbered source [n] with a deep link, and it must cite those numbers.

use crate::llm::{self, Call, ToolDef};
use crate::settings::Settings;
use crate::{caselaw, govuk, lex, net};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

const MAX_ROUNDS: usize = 8;
const MAX_TOOL_CHARS: usize = 9000;

#[derive(Deserialize, Clone)]
pub struct Turn {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct AskRequest {
    #[serde(default)]
    pub history: Vec<Turn>,
    pub question: String,
    /// Numbering continues across a conversation so earlier [n] stay valid.
    #[serde(default = "one")]
    pub first_source: u32,
    #[serde(default)]
    pub today: String,
}
fn one() -> u32 {
    1
}

#[derive(Serialize, Clone)]
pub struct Source {
    pub n: u32,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub url: String,
    pub excerpt: String,
}

#[derive(Serialize)]
pub struct Step {
    pub tool: String,
    pub summary: String,
    pub ok: bool,
}

#[derive(Serialize)]
pub struct AskResponse {
    pub answer: String,
    /// Sources cited in the answer (or all retrieved, if none were cited).
    pub sources: Vec<Source>,
    pub retrieved: usize,
    pub steps: Vec<Step>,
    pub next_source: u32,
}

fn system_prompt(today: &str) -> String {
    format!(
        "You are Curia, a research assistant for UK immigration, asylum and nationality law (and \
UK law generally). Today is {today}. Use British English.

How to work:
- ALWAYS research with the tools before answering. Do not answer from memory. Typical plan: \
search the Immigration Rules and Home Office guidance (search_guidance), the relevant statute \
sections (search_legislation), and leading cases (search_case_law); then read the most \
relevant documents with read_guidance / read_judgment using a focused query.
- Keep case-law search queries short (2–5 distinctive words, e.g. \"117B little weight\"): the \
search requires every word to appear.
- Prefer primary sources: Immigration Rules and legislation state the law; Home Office \
guidance states policy (say so when relying on it); tribunal and court decisions interpret both. \
Prefer reported/starred Upper Tribunal decisions and higher courts.
- Each passage you are shown has a number like [7]. Support every legal statement with these \
numbers, placed right after the statement, e.g. \"... must be given little weight [3].\" Cite \
ONLY numbers you have been shown. Never invent cases, paragraphs, sections or URLs.
- If the sources do not answer the question, say so plainly and say what you did find.
- Note when guidance or rules may have changed recently, or when transitional provisions may matter.
- Be concise and structured: a short direct answer first, then the detail with headings or bullet \
points, then 'Key sources' if useful. End with one line: 'Research aid, not legal advice — check \
the linked sources.'
- Do not ask the user for personal or client-identifying details."
    )
}

fn tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "search_guidance",
            description: "Search GOV.UK for the Immigration Rules (each Part/Appendix) and Home Office / UKVI caseworker guidance. Returns matching documents and the most relevant passages from the top results, each numbered for citation.",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Natural-language search, e.g. 'appendix FM financial requirement self-employed'"},
                    "scope": {"type": "string", "enum": ["immigration", "rules", "all"], "description": "'rules' = Immigration Rules only; 'immigration' (default) = Rules + Home Office/UKVI guidance; 'all' = guidance from any department"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "read_guidance",
            description: "Read a GOV.UK guidance document or Immigration Rules section (by its path from search_guidance) and return the passages most relevant to a focused query, numbered for citation.",
            schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "GOV.UK path, e.g. /guidance/immigration-rules/immigration-rules-appendix-fm-family-members"},
                    "query": {"type": "string", "description": "What to look for in the document"}
                },
                "required": ["path", "query"]
            }),
        },
        ToolDef {
            name: "search_legislation",
            description: "Semantic search over sections of UK Acts and Statutory Instruments (via the i.AI Lex API). Returns section text, numbered for citation. Optionally restrict to one piece of legislation, e.g. 'ukpga/2002/41' (Nationality, Immigration and Asylum Act 2002), 'ukpga/1971/77' (Immigration Act 1971), 'ukpga/1981/61' (British Nationality Act 1981), 'ukpga/1998/42' (Human Rights Act 1998).",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "legislation_id": {"type": "string", "description": "Optional, e.g. ukpga/2002/41"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "search_case_law",
            description: "Search judgments on The National Archives' Find Case Law, ranked by relevance. Default courts: Upper Tribunal (IAC), Court of Appeal (Civil), Supreme Court, Administrative Court, Privy Council. Returns the case list and relevant paragraphs from the top results, numbered for citation. Every word in the query must appear in the judgment, so keep queries short.",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "2-5 distinctive words"},
                    "courts": {"type": "string", "description": "'immigration' (default), 'all', or a court code such as 'ukut/iac', 'uksc', 'ewca/civ'"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "read_judgment",
            description: "Read a judgment (by its Find Case Law reference from search_case_law, e.g. 'ukut/iac/2019/72') and return the paragraphs most relevant to a focused query, numbered for citation.",
            schema: json!({
                "type": "object",
                "properties": {
                    "uri": {"type": "string"},
                    "query": {"type": "string"}
                },
                "required": ["uri", "query"]
            }),
        },
    ]
}

struct Registry {
    next: u32,
    by_url: HashMap<String, u32>,
    sources: Vec<Source>,
}

impl Registry {
    fn add(&mut self, kind: &str, title: &str, detail: &str, url: &str, excerpt: &str) -> u32 {
        if let Some(n) = self.by_url.get(url) {
            return *n;
        }
        let n = self.next;
        self.next += 1;
        self.by_url.insert(url.to_string(), n);
        self.sources.push(Source {
            n,
            kind: kind.into(),
            title: title.into(),
            detail: detail.into(),
            url: url.into(),
            excerpt: net::truncate(excerpt, 600),
        });
        n
    }
}

fn arg<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("").trim()
}

/// Turns a Lex section URI (http://www.legislation.gov.uk/id/ukpga/2002/41/section/117B)
/// into a public link and a short label.
fn leg_link(uri: &str) -> (String, String) {
    let path = uri
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("www.legislation.gov.uk/")
        .trim_start_matches("id/");
    let label = path
        .replace("/section/", " s.")
        .replace("/regulation/", " reg.")
        .replace("/article/", " art.")
        .replace("/schedule/", " Sch.");
    (format!("https://www.legislation.gov.uk/{path}"), label)
}

async fn run_tool(
    call: &Call,
    s: &Settings,
    client: &reqwest::Client,
    reg: &mut Registry,
) -> (String, String) {
    let a = &call.args;
    let res: Result<String, String> = match call.name.as_str() {
        "search_guidance" => {
            let q = arg(a, "query");
            let scope = match arg(a, "scope") {
                "" => "immigration",
                x => x,
            };
            match govuk::search(client, q, scope, 8).await {
                Err(e) => Err(e),
                Ok(hits) if hits.is_empty() => Ok("No GOV.UK documents matched. Try different words or scope.".into()),
                Ok(hits) => {
                    let mut out = String::from("Documents found (use read_guidance with a path to read more):\n");
                    for h in &hits {
                        out.push_str(&format!(
                            "- {} [{}] path={} updated={}\n",
                            h.title,
                            h.kind,
                            h.path,
                            h.updated.get(..10).unwrap_or("")
                        ));
                    }
                    // Read the top two straight away to save a round trip.
                    let tops: Vec<_> = hits.iter().take(2).collect();
                    let docs = futures::future::join_all(tops.iter().map(|h| govuk::fetch(client, &h.path))).await;
                    for d in docs.into_iter().flatten() {
                        out.push_str(&format_guidance(&d, q, 3, reg));
                    }
                    Ok(out)
                }
            }
        }
        "read_guidance" => match govuk::fetch(client, arg(a, "path")).await {
            Ok(d) => Ok(format_guidance(&d, arg(a, "query"), 6, reg)),
            Err(e) => Err(e),
        },
        "search_legislation" => {
            let id = arg(a, "legislation_id");
            match lex::section_search(&s.lex_base_url, &s.contact, arg(a, "query"), Some(id), 6).await {
                Err(e) => Err(e),
                Ok(v) => {
                    let mut out = String::new();
                    for sec in v.as_array().cloned().unwrap_or_default() {
                        let uri = sec["uri"].as_str().unwrap_or("");
                        if uri.is_empty() {
                            continue;
                        }
                        let (url, label) = leg_link(uri);
                        let heading = sec["title"].as_str().unwrap_or("");
                        let text = sec["text"].as_str().unwrap_or("");
                        let n = reg.add("Legislation", &label, heading, &url, text);
                        out.push_str(&format!(
                            "\n[{n}] {label} — {heading}\n{url}\n{}\n",
                            net::truncate(text, 1800)
                        ));
                    }
                    if out.is_empty() {
                        out = "No legislation sections matched.".into();
                    }
                    Ok(out)
                }
            }
        }
        "search_case_law" => {
            let q = arg(a, "query");
            let courts = match arg(a, "courts") {
                "" => "immigration",
                x => x,
            };
            match caselaw::search(client, q, courts, 1, 8).await {
                Err(e) => Err(e),
                Ok(cases) if cases.is_empty() => Ok(
                    "No judgments contain all of those words. Try fewer or different words.".into(),
                ),
                Ok(cases) => {
                    let mut out = String::from("Judgments found (use read_judgment with uri to read more):\n");
                    for c in &cases {
                        out.push_str(&format!(
                            "- {} {} ({}, {}) uri={}\n",
                            c.title, c.citation, c.court, c.date, c.uri
                        ));
                    }
                    let tops: Vec<_> = cases.iter().take(2).collect();
                    let js = futures::future::join_all(tops.iter().map(|c| caselaw::fetch(client, &c.uri))).await;
                    for j in js.into_iter().flatten() {
                        out.push_str(&format_judgment(&j, q, 3, reg));
                    }
                    Ok(out)
                }
            }
        }
        "read_judgment" => match caselaw::fetch(client, arg(a, "uri")).await {
            Ok(j) => Ok(format_judgment(&j, arg(a, "query"), 6, reg)),
            Err(e) => Err(e),
        },
        other => Err(format!("Unknown tool {other}")),
    };
    let ok = res.is_ok();
    let text = match res {
        Ok(t) => net::truncate(&t, MAX_TOOL_CHARS),
        Err(e) => format!("Tool error: {e}"),
    };
    (if ok { "ok".into() } else { "error".into() }, text)
}

fn format_guidance(d: &govuk::Doc, query: &str, k: usize, reg: &mut Registry) -> String {
    let mut out = format!(
        "\n=== {} ({}; last updated {}) {}\n",
        d.title,
        d.kind,
        d.updated.get(..10).unwrap_or(""),
        d.url
    );
    if let Some(c) = d.change_history.first() {
        if !c.note.is_empty() {
            out.push_str(&format!("Latest change note: {}\n", net::truncate(&c.note, 300)));
        }
    }
    let ps = govuk::relevant(d, query, k, 4000);
    if ps.is_empty() {
        if !d.pdfs.is_empty() {
            out.push_str("(This guidance is only published as PDF, which Curia cannot read; link: ");
            out.push_str(&d.pdfs[0]);
            out.push_str(")\n");
        } else {
            out.push_str("(No text found.)\n");
        }
    }
    for p in ps {
        let detail = p.heading.clone().unwrap_or_default();
        let n = reg.add(&d.kind, &d.title, &detail, &p.url, &p.text);
        out.push_str(&format!("\n[{n}] {} — {}\n{}\n", d.title, detail, p.text));
    }
    out
}

fn format_judgment(j: &caselaw::Judgment, query: &str, k: usize, reg: &mut Registry) -> String {
    let mut out = format!("\n=== {} {} {} ({} paragraphs)\n", j.title, j.citation, j.url, j.paragraphs);
    for p in caselaw::relevant(j, query, k, 4000) {
        let para = p.url.rsplit('#').next().filter(|x| x.starts_with("para_")).map(|x| format!("para {}", &x[5..])).unwrap_or_default();
        let title = if j.citation.is_empty() || j.title.contains(&j.citation) {
            j.title.clone()
        } else {
            format!("{} {}", j.title, j.citation)
        };
        let n = reg.add("Case law", &title, &para, &p.url, &p.text);
        out.push_str(&format!("\n[{n}] {title} {para}\n{}\n", p.text));
    }
    out
}

fn summarise_call(c: &Call) -> String {
    let a = &c.args;
    match c.name.as_str() {
        "search_guidance" => format!("Searched guidance/Rules: “{}”", arg(a, "query")),
        "read_guidance" => format!("Read {} for “{}”", arg(a, "path"), arg(a, "query")),
        "search_legislation" => {
            let id = arg(a, "legislation_id");
            if id.is_empty() {
                format!("Searched legislation: “{}”", arg(a, "query"))
            } else {
                format!("Searched {id}: “{}”", arg(a, "query"))
            }
        }
        "search_case_law" => format!("Searched case law: “{}”", arg(a, "query")),
        "read_judgment" => format!("Read judgment {} for “{}”", arg(a, "uri"), arg(a, "query")),
        o => o.to_string(),
    }
}

pub async fn ask(s: &Settings, req: AskRequest) -> Result<AskResponse, String> {
    let provider = llm::resolve(s)?;
    let client = net::client(&s.contact)?;
    let tools = tools();
    let today = if req.today.trim().is_empty() { "today".to_string() } else { req.today.clone() };
    let system = system_prompt(&today);

    let mut msgs: Vec<Value> = vec![];
    // Earlier turns are passed as plain text so follow-up questions have context.
    for t in req.history.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev() {
        let role = if t.role == "assistant" { "assistant" } else { "user" };
        if !t.content.trim().is_empty() {
            msgs.push(json!({ "role": role, "content": net::truncate(&t.content, 6000) }));
        }
    }
    msgs.push(json!({ "role": "user", "content": req.question }));

    let mut reg = Registry {
        next: req.first_source.max(1),
        by_url: HashMap::new(),
        sources: vec![],
    };
    let mut steps = vec![];
    let mut answer = String::new();

    for round in 0..MAX_ROUNDS {
        let last = round == MAX_ROUNDS - 1;
        let out = llm::step(&provider, &system, &mut msgs, &tools, !last, 4096).await?;
        if out.calls.is_empty() {
            answer = out.text;
            break;
        }
        let mut results = vec![];
        for c in &out.calls {
            let (status, text) = run_tool(c, s, &client, &mut reg).await;
            steps.push(Step { tool: c.name.clone(), summary: summarise_call(c), ok: status == "ok" });
            results.push((c.id.clone(), text));
        }
        llm::push_results(&provider, &mut msgs, results);
    }
    if answer.trim().is_empty() {
        // Out of rounds: ask for a final answer from what was gathered.
        msgs.push(json!({"role": "user", "content": "Please now answer using only the numbered sources gathered so far."}));
        answer = llm::step(&provider, &system, &mut msgs, &tools, false, 4096).await?.text;
    }

    let cited: std::collections::HashSet<u32> = {
        let mut set = std::collections::HashSet::new();
        let bytes: Vec<char> = answer.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '[' {
                let mut j = i + 1;
                let mut num = String::new();
                while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == ',' || bytes[j] == ' ' || bytes[j] == '–' || bytes[j] == '-') {
                    num.push(bytes[j]);
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == ']' {
                    for part in num.split(',') {
                        let part = part.trim();
                        if let Some((a, b)) = part.split_once(|c| c == '–' || c == '-') {
                            if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                                for x in a..=b.min(a + 50) {
                                    set.insert(x);
                                }
                            }
                        } else if let Ok(x) = part.parse::<u32>() {
                            set.insert(x);
                        }
                    }
                }
                i = j;
            } else {
                i += 1;
            }
        }
        set
    };
    let retrieved = reg.sources.len();
    let mut sources: Vec<Source> = reg.sources.iter().filter(|x| cited.contains(&x.n)).cloned().collect();
    if sources.is_empty() {
        sources = reg.sources.clone();
    }
    Ok(AskResponse {
        answer,
        sources,
        retrieved,
        steps,
        next_source: reg.next,
    })
}

#[cfg(test)]
mod live_tests {
    //! Network tests against the real services: `cargo test -- --ignored --nocapture`
    use super::*;

    fn rt<F: std::future::Future>(f: F) -> F::Output {
        tauri::async_runtime::block_on(f)
    }

    #[test]
    #[ignore]
    fn tools_against_live_sources() {
        let s = Settings::default();
        let client = net::client("").unwrap();
        let mut reg = Registry { next: 1, by_url: HashMap::new(), sources: vec![] };
        let calls = vec![
            ("search_guidance", json!({"query": "appendix FM minimum income requirement", "scope": "rules"})),
            ("search_guidance", json!({"query": "private life caseworker guidance very significant obstacles"})),
            ("read_guidance", json!({"path": "/government/publications/private-life-caseworker-guidance", "query": "very significant obstacles to integration"})),
            ("search_legislation", json!({"query": "little weight private life precarious immigration status"})),
            ("search_legislation", json!({"query": "deprivation of citizenship conducive to the public good", "legislation_id": "ukpga/1981/61"})),
            ("search_case_law", json!({"query": "117B little weight precarious"})),
            ("read_judgment", json!({"uri": "ukut/iac/2019/72", "query": "section 117B(6) reasonable to expect child to leave"})),
        ];
        for (name, args) in calls {
            let c = Call { id: "t".into(), name: name.into(), args };
            let (status, text) = rt(run_tool(&c, &s, &client, &mut reg));
            println!("\n######## {name} -> {status} ({} chars)\n{}", text.len(), net::truncate(&text, 1500));
            assert_eq!(status, "ok", "{name} failed: {text}");
        }
        println!("\n######## {} sources registered", reg.sources.len());
        for src in reg.sources.iter().take(40) {
            println!("[{}] {} | {} | {} | {}", src.n, src.kind, src.title, src.detail, src.url);
        }
        assert!(reg.sources.len() > 10);
    }

    #[test]
    #[ignore]
    fn loop_against_mock_openai_server() {
        // Requires /tmp/mock/server.py listening on 127.0.0.1:8765.
        let mut s = Settings::default();
        s.llm_enabled = true;
        s.provider = "ollama".into();
        s.base_url = "http://127.0.0.1:8765/v1".into();
        s.model = "mock".into();
        let req = AskRequest {
            history: vec![Turn { role: "user".into(), content: "earlier q".into() }, Turn { role: "assistant".into(), content: "earlier a [1]".into() }],
            question: "Does precarious status reduce weight?".into(),
            first_source: 1,
            today: "26 November 2026".into(),
        };
        let r = rt(ask(&s, req)).expect("ask failed");
        println!("ANSWER: {}\nsteps: {:?}\nretrieved {} cited {:?} next {}", r.answer,
            r.steps.iter().map(|x| x.summary.clone()).collect::<Vec<_>>(), r.retrieved,
            r.sources.iter().map(|x| (x.n, x.title.clone())).collect::<Vec<_>>(), r.next_source);
        assert_eq!(r.steps.len(), 2);
        assert!(r.sources.iter().any(|x| x.n == 1));
        assert_eq!(r.sources.len(), 3);
    }
}
