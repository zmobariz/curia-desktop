//! Thin client for the UK i.AI Lex REST API.
//!
//! Contract confirmed against i-dot-ai/lex (src/backend/*/router.py):
//!   POST /legislation/search  -> { results: [...], total, offset, limit }
//!   POST /legislation/text    -> { legislation: {...}, full_text: "..." }
//!   GET  /healthcheck         -> { status, ... }
//! No authentication. CORS "*". Rate limits (defaults): 600/min, 10k/hour per IP.

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub year_from: Option<i32>,
    #[serde(default)]
    pub year_to: Option<i32>,
    /// Lex `LegislationType` values, e.g. ["ukpga", "uksi"]. Empty = all.
    #[serde(default)]
    pub legislation_type: Vec<String>,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default = "default_true")]
    pub include_text: bool,
}

fn default_limit() -> u32 {
    10
}
fn default_true() -> bool {
    true
}

fn client(contact: &str) -> Result<reqwest::Client, String> {
    let ua = if contact.trim().is_empty() {
        "Curia-Desktop/0.1 (+https://github.com/zmobariz/Curia)".to_string()
    } else {
        format!("Curia-Desktop/0.1 ({})", contact.trim())
    };
    reqwest::Client::builder()
        .user_agent(ua)
        .build()
        .map_err(|e| e.to_string())
}

fn base(url: &str) -> &str {
    url.trim_end_matches('/')
}

pub async fn search(base_url: &str, contact: &str, p: &SearchParams) -> Result<Value, String> {
    let mut body = serde_json::json!({
        "query": p.query,
        "offset": p.offset,
        "limit": p.limit,
        "include_text": p.include_text,
    });
    if let Some(y) = p.year_from {
        body["year_from"] = y.into();
    }
    if let Some(y) = p.year_to {
        body["year_to"] = y.into();
    }
    if !p.legislation_type.is_empty() {
        body["legislation_type"] = serde_json::json!(p.legislation_type);
    }
    let url = format!("{}/legislation/search", base(base_url));
    let resp = client(contact)?
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    parse(resp).await
}

pub async fn full_text(
    base_url: &str,
    contact: &str,
    legislation_id: &str,
    include_schedules: bool,
) -> Result<Value, String> {
    let url = format!("{}/legislation/text", base(base_url));
    let body = serde_json::json!({
        "legislation_id": legislation_id,
        "include_schedules": include_schedules,
    });
    let resp = client(contact)?
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    parse(resp).await
}

pub async fn health(base_url: &str, contact: &str) -> Result<Value, String> {
    let url = format!("{}/healthcheck", base(base_url));
    let resp = client(contact)?
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    parse(resp).await
}

async fn parse(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let hint = if status.as_u16() == 429 {
            " — the Lex API is rate-limited; wait a moment and try again."
        } else {
            ""
        };
        return Err(format!(
            "Lex API error {}{}: {}",
            status.as_u16(),
            hint,
            truncate(&text, 300)
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("Invalid JSON from Lex: {}", e))
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if t.len() < s.len() {
        format!("{}…", t)
    } else {
        t
    }
}
