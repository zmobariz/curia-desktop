//! Case law from The National Archives' Find Case Law service.
//!
//!   Search:   GET https://caselaw.nationalarchives.gov.uk/atom.xml?query=…&court=…&order=relevance
//!   Judgment: GET https://caselaw.nationalarchives.gov.uk/{uri}/data.xml  (LegalDocML)
//!
//! Judgments are re-used under the Open Justice Licence. Curia only runs the
//! searches a user (or their assistant) asks for and reads judgments one at a
//! time — it does no bulk download, indexing or "computational analysis",
//! which would need a separate licence from The National Archives.

use crate::net;
use crate::passages::{self, Block, Passage};
use serde::Serialize;

const FCL: &str = "https://caselaw.nationalarchives.gov.uk";
const ATOM_NS: &str = "http://www.w3.org/2005/Atom";
const TNA_NS: &str = "https://caselaw.nationalarchives.gov.uk";

/// Courts used for the default, immigration-focused search.
pub const IMMIGRATION_COURTS: &[&str] = &["ukut/iac", "ewca/civ", "uksc", "ewhc/admin", "ukpc"];

#[derive(Serialize, Clone)]
pub struct Case {
    pub title: String,
    /// Path on Find Case Law, e.g. "ukut/iac/2019/72".
    pub uri: String,
    pub url: String,
    pub citation: String,
    pub court: String,
    pub date: String,
}

#[derive(Serialize, Clone)]
pub struct Judgment {
    pub title: String,
    pub uri: String,
    pub url: String,
    pub citation: String,
    pub paragraphs: usize,
    #[serde(skip)]
    pub blocks: Vec<Block>,
}

/// courts: "immigration" (Upper Tribunal IAC, Court of Appeal, Supreme Court,
/// Admin Court, Privy Council), "all", or a single court code such as "ukut/iac".
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    courts: &str,
    page: u32,
    per_page: u32,
) -> Result<Vec<Case>, String> {
    let mut params: Vec<(&str, String)> = vec![
        ("query", query.to_string()),
        ("order", "relevance".into()),
        ("per_page", per_page.clamp(1, 50).to_string()),
        ("page", page.max(1).to_string()),
    ];
    match courts {
        "all" | "" => {}
        "immigration" => {
            for c in IMMIGRATION_COURTS {
                params.push(("court", c.to_string()));
            }
        }
        one => {
            if !one.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-') {
                return Err("Invalid court code.".into());
            }
            params.push(("court", one.to_string()));
        }
    }
    let url = reqwest::Url::parse_with_params(&format!("{FCL}/atom.xml"), &params)
        .map_err(|e| e.to_string())?;
    let xml = net::get_text(client, url.as_str(), "Find Case Law search").await?;
    parse_feed(&xml)
}

fn parse_feed(xml: &str) -> Result<Vec<Case>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("Find Case Law feed: {e}"))?;
    let mut out = vec![];
    for entry in doc
        .descendants()
        .filter(|n| n.has_tag_name((ATOM_NS, "entry")))
    {
        let child_text = |ns: &str, name: &str| -> String {
            entry
                .children()
                .find(|c| c.has_tag_name((ns, name)))
                .and_then(|c| c.text())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let url = entry
            .children()
            .find(|c| {
                c.has_tag_name((ATOM_NS, "link"))
                    && c.attribute("rel") == Some("alternate")
                    && c.attribute("type").is_none()
            })
            .and_then(|c| c.attribute("href"))
            .unwrap_or("")
            .to_string();
        let Some(uri) = url.strip_prefix(&format!("{FCL}/")).map(|s| s.to_string()) else {
            continue;
        };
        let citation = entry
            .children()
            .filter(|c| c.has_tag_name((TNA_NS, "identifier")))
            .find(|c| c.attribute("type").map(|t| t.contains("ncn")).unwrap_or(true))
            .and_then(|c| c.text())
            .unwrap_or("")
            .trim()
            .to_string();
        let court = entry
            .children()
            .find(|c| c.has_tag_name((ATOM_NS, "author")))
            .and_then(|a| a.children().find(|c| c.has_tag_name((ATOM_NS, "name"))))
            .and_then(|c| c.text())
            .unwrap_or("")
            .to_string();
        out.push(Case {
            title: child_text(ATOM_NS, "title"),
            uri,
            url,
            citation,
            court,
            date: child_text(ATOM_NS, "published").chars().take(10).collect(),
        });
    }
    Ok(out)
}

/// Accept "ukut/iac/2019/72" or a full Find Case Law URL.
pub fn clean_uri(uri: &str) -> Result<String, String> {
    let u = uri.trim();
    let u = u.strip_prefix(FCL).unwrap_or(u);
    let u = u.split('#').next().unwrap_or(u);
    let u = u.trim_matches('/');
    let u = u.strip_suffix("/data.xml").unwrap_or(u);
    if u.is_empty()
        || u.contains("..")
        || !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_')
    {
        return Err(format!("Not a Find Case Law judgment reference: {uri}"));
    }
    Ok(u.to_string())
}

pub async fn fetch(client: &reqwest::Client, uri: &str) -> Result<Judgment, String> {
    let uri = clean_uri(uri)?;
    let xml = net::get_text(client, &format!("{FCL}/{uri}/data.xml"), "Find Case Law judgment").await?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("Judgment XML: {e}"))?;
    let page_url = format!("{FCL}/{uri}");

    let title = doc
        .descendants()
        .find(|n| n.tag_name().name() == "FRBRname")
        .and_then(|n| n.attribute("value"))
        .unwrap_or("")
        .to_string();
    let citation = doc
        .descendants()
        .find(|n| n.tag_name().name() == "cite" || n.tag_name().name() == "neutralCitation")
        .map(|n| net::squash(&n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect::<String>()))
        .unwrap_or_default();

    let mut blocks = vec![];
    for p in doc
        .descendants()
        .filter(|n| n.tag_name().name() == "paragraph" && n.attribute("eId").is_some())
    {
        // Skip outer paragraphs whose text is carried by nested numbered ones.
        if p.descendants()
            .skip(1)
            .any(|d| d.tag_name().name() == "paragraph" && d.attribute("eId").is_some())
        {
            continue;
        }
        let text = net::squash(&p.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect::<String>());
        if text.is_empty() {
            continue;
        }
        let eid = p.attribute("eId").unwrap_or("");
        // The public page anchors top-level paragraphs as #para_N.
        let anchor = eid.split("__").next().unwrap_or(eid).to_string();
        blocks.push(Block {
            url: page_url.clone(),
            anchor: Some(anchor),
            heading: None,
            text,
        });
    }
    if blocks.is_empty() {
        // Some older documents have no numbered paragraphs: fall back to <p>.
        for p in doc.descendants().filter(|n| n.tag_name().name() == "p") {
            let text = net::squash(&p.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect::<String>());
            if text.len() > 40 {
                blocks.push(Block {
                    url: page_url.clone(),
                    anchor: None,
                    heading: None,
                    text,
                });
            }
        }
    }
    let n = blocks.len();
    Ok(Judgment {
        title,
        uri: uri.clone(),
        url: page_url,
        citation,
        paragraphs: n,
        blocks,
    })
}

pub fn relevant(j: &Judgment, query: &str, k: usize, max_chars: usize) -> Vec<Passage> {
    passages::best(&j.blocks, query, k, max_chars)
}
