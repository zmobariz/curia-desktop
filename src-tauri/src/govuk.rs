//! Home Office / UKVI guidance and the Immigration Rules, read live from GOV.UK.
//!
//!   Search:  GET https://www.gov.uk/api/search.json   (Search API)
//!   Content: GET https://www.gov.uk/api/content{path} (Content API)
//!
//! Both are public, need no key, and the content is Crown copyright under the
//! Open Government Licence v3.0. The Immigration Rules are published as a GOV.UK
//! "manual" at /guidance/immigration-rules, one section per Part/Appendix.

use crate::net::{self, squash};
use crate::passages::{self, Block, Passage};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use serde_json::Value;

const GOVUK: &str = "https://www.gov.uk";
pub const RULES_MANUAL: &str = "/guidance/immigration-rules";

const IMMIGRATION_ORGS: &[&str] = &["uk-visas-and-immigration", "home-office"];
const GUIDANCE_TYPES: &[&str] = &[
    "guidance",
    "manual_section",
    "manual",
    "detailed_guide",
    "statutory_guidance",
    "policy_paper",
    "guide",
    "answer",
];

#[derive(Serialize, Clone)]
pub struct Hit {
    pub title: String,
    /// GOV.UK path, e.g. /government/publications/private-life-caseworker-guidance
    pub path: String,
    pub url: String,
    pub kind: String,
    pub description: String,
    pub updated: String,
}

#[derive(Serialize, Clone)]
pub struct Change {
    pub note: String,
    pub timestamp: String,
}

#[derive(Serialize, Clone)]
pub struct Doc {
    pub title: String,
    pub path: String,
    pub url: String,
    pub kind: String,
    pub updated: String,
    pub change_history: Vec<Change>,
    /// PDF versions, when the guidance is (also) published as a PDF.
    pub pdfs: Vec<String>,
    #[serde(skip)]
    pub blocks: Vec<Block>,
}

/// scope: "immigration" (Home Office + UKVI guidance and the Rules), "rules"
/// (Immigration Rules only) or "all" (all of GOV.UK guidance).
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    scope: &str,
    count: u32,
) -> Result<Vec<Hit>, String> {
    let mut params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("count", count.clamp(1, 50).to_string()),
        (
            "fields",
            "title,link,description,public_timestamp,content_store_document_type".into(),
        ),
    ];
    match scope {
        "rules" => params.push(("filter_manual", RULES_MANUAL.into())),
        "all" => {
            for t in GUIDANCE_TYPES {
                params.push(("filter_content_store_document_type[]", t.to_string()));
            }
        }
        _ => {
            for o in IMMIGRATION_ORGS {
                params.push(("filter_organisations[]", o.to_string()));
            }
            for t in GUIDANCE_TYPES {
                params.push(("filter_content_store_document_type[]", t.to_string()));
            }
        }
    }
    let url = reqwest::Url::parse_with_params(&format!("{GOVUK}/api/search.json"), &params)
        .map_err(|e| e.to_string())?;
    let text = net::get_text(client, url.as_str(), "GOV.UK search").await?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(hits_from(&v))
}

/// Most recently updated Home Office / UKVI guidance and Rules sections.
pub async fn recent(client: &reqwest::Client, count: u32) -> Result<Vec<Hit>, String> {
    let mut params: Vec<(&str, String)> = vec![
        ("order", "-public_timestamp".into()),
        ("count", count.clamp(1, 100).to_string()),
        (
            "fields",
            "title,link,description,public_timestamp,content_store_document_type".into(),
        ),
    ];
    for o in IMMIGRATION_ORGS {
        params.push(("filter_organisations[]", o.to_string()));
    }
    for t in ["guidance", "manual_section", "detailed_guide", "statutory_guidance"] {
        params.push(("filter_content_store_document_type[]", t.to_string()));
    }
    let url = reqwest::Url::parse_with_params(&format!("{GOVUK}/api/search.json"), &params)
        .map_err(|e| e.to_string())?;
    let text = net::get_text(client, url.as_str(), "GOV.UK search").await?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(hits_from(&v))
}

fn hits_from(v: &Value) -> Vec<Hit> {
    v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let link = r["link"].as_str()?;
                    if !link.starts_with('/') {
                        return None;
                    }
                    Some(Hit {
                        title: r["title"].as_str().unwrap_or("").to_string(),
                        path: link.to_string(),
                        url: format!("{GOVUK}{link}"),
                        kind: kind_label(r["content_store_document_type"].as_str().unwrap_or("")),
                        description: r["description"].as_str().unwrap_or("").to_string(),
                        updated: r["public_timestamp"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn kind_label(t: &str) -> String {
    match t {
        "manual_section" | "manual" => "Immigration Rules",
        "guidance" | "detailed_guide" | "statutory_guidance" | "html_publication" => "Guidance",
        "policy_paper" => "Policy paper",
        "guide" | "answer" => "GOV.UK guide",
        other => other,
    }
    .to_string()
}

/// Only GOV.UK paths are accepted (no hosts, no traversal).
pub fn clean_path(path: &str) -> Result<String, String> {
    let p = path.trim();
    let p = p.strip_prefix("https://www.gov.uk").unwrap_or(p);
    let p = p.split('#').next().unwrap_or(p);
    if !p.starts_with('/') || p.contains("..") || p.contains("//") || p.contains('?') {
        return Err(format!("Not a GOV.UK page path: {path}"));
    }
    Ok(p.to_string())
}

async fn content(client: &reqwest::Client, path: &str) -> Result<Value, String> {
    let text = net::get_text(client, &format!("{GOVUK}/api/content{path}"), "GOV.UK content").await?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Fetch a guidance document / Rules section, following HTML attachments so
/// the actual caseworker guidance text is included (not just the landing page).
pub async fn fetch(client: &reqwest::Client, path: &str) -> Result<Doc, String> {
    let path = clean_path(path)?;
    let v = content(client, &path).await?;
    let det = &v["details"];
    let page_url = format!("{GOVUK}{path}");
    let mut blocks = html_blocks(det["body"].as_str().unwrap_or(""), &page_url);

    let mut pdfs = vec![];
    let mut html_attachments = vec![];
    if let Some(atts) = det["attachments"].as_array() {
        for a in atts {
            let url = a["url"].as_str().unwrap_or("");
            if a["attachment_type"].as_str() == Some("html") && url.starts_with('/') {
                html_attachments.push(url.to_string());
            } else if a["content_type"].as_str() == Some("application/pdf") && url.starts_with("https://") {
                pdfs.push(url.to_string());
            }
        }
    }
    for att in html_attachments.iter().take(3) {
        if let Ok(av) = content(client, att).await {
            let body = av["details"]["body"].as_str().unwrap_or("");
            blocks.extend(html_blocks(body, &format!("{GOVUK}{att}")));
        }
    }

    let change_history = det["change_history"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Change {
                    note: c["note"].as_str().unwrap_or("").trim().to_string(),
                    timestamp: c["public_timestamp"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Doc {
        title: v["title"].as_str().unwrap_or("").to_string(),
        path: path.clone(),
        url: page_url,
        kind: kind_label(v["document_type"].as_str().unwrap_or("")),
        updated: v["public_updated_at"].as_str().unwrap_or("").to_string(),
        change_history,
        pdfs,
        blocks: passages::coalesce(blocks, 900),
    })
}

/// Lightweight metadata (title + last updated + change notes) without the body.
pub async fn meta(client: &reqwest::Client, path: &str) -> Result<(String, String, Vec<Change>), String> {
    let path = clean_path(path)?;
    let v = content(client, &path).await?;
    let ch = v["details"]["change_history"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Change {
                    note: c["note"].as_str().unwrap_or("").trim().to_string(),
                    timestamp: c["public_timestamp"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((
        v["title"].as_str().unwrap_or("").to_string(),
        v["public_updated_at"].as_str().unwrap_or("").to_string(),
        ch,
    ))
}

pub fn relevant(doc: &Doc, query: &str, k: usize, max_chars: usize) -> Vec<Passage> {
    passages::best(&doc.blocks, query, k, max_chars)
}

/// Plain-text rendering (headings marked) for snapshots and diffs.
pub fn plain_text(doc: &Doc) -> String {
    let mut out = String::new();
    let mut last_heading: Option<&String> = None;
    for b in &doc.blocks {
        if b.heading.as_ref() != last_heading {
            if let Some(h) = &b.heading {
                out.push_str("\n## ");
                out.push_str(h);
                out.push('\n');
            }
            last_heading = b.heading.as_ref();
        }
        out.push_str(&b.text);
        out.push('\n');
    }
    out
}

/// Split GOV.UK body HTML into text blocks, each tagged with the nearest heading
/// and its id so passages can deep-link (#anchor) into the page.
fn html_blocks(html: &str, page_url: &str) -> Vec<Block> {
    if html.trim().is_empty() {
        return vec![];
    }
    let doc = Html::parse_fragment(html);
    let sel = Selector::parse("h1,h2,h3,h4,h5,h6,p,li,td,th,dt,dd,pre").unwrap();
    let mut out = vec![];
    let mut heading: Option<String> = None;
    let mut anchor: Option<String> = None;
    for el in doc.select(&sel) {
        let name = el.value().name();
        let text = squash(&el.text().collect::<String>());
        if text.is_empty() {
            continue;
        }
        if name.len() == 2 && name.starts_with('h') && name != "hr" {
            heading = Some(text);
            anchor = el.value().id().map(|s| s.to_string()).or(anchor);
            continue;
        }
        // Avoid double-counting: a list item / cell whose text is carried by
        // nested paragraphs is emitted via those paragraphs instead.
        if (name == "li" || name == "td" || name == "th" || name == "dd") && has_block_child(&el) {
            continue;
        }
        let own_anchor = el.value().id().map(|s| s.to_string());
        out.push(Block {
            url: page_url.to_string(),
            anchor: own_anchor.or_else(|| anchor.clone()),
            heading: heading.clone(),
            text,
        });
    }
    out
}

fn has_block_child(el: &ElementRef) -> bool {
    el.descendants().skip(1).any(|n| {
        n.value()
            .as_element()
            .map(|e| matches!(e.name(), "p" | "li" | "ul" | "ol" | "table"))
            .unwrap_or(false)
    })
}
