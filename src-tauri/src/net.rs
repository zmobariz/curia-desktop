//! Shared HTTP client for the public sources Curia reads (Lex, GOV.UK, Find
//! Case Law). All requests go over HTTPS from the Rust side; the web UI never
//! talks to the network directly.

use std::time::Duration;

pub fn client(contact: &str) -> Result<reqwest::Client, String> {
    let ua = if contact.trim().is_empty() {
        format!(
            "Curia-Desktop/{} (+https://github.com/zmobariz/curia-desktop)",
            env!("CARGO_PKG_VERSION")
        )
    } else {
        format!("Curia-Desktop/{} ({})", env!("CARGO_PKG_VERSION"), contact.trim())
    };
    reqwest::Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

/// GET a URL and return the body as text, turning HTTP errors into readable messages.
pub async fn get_text(client: &reqwest::Client, url: &str, what: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{what}: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("{what}: {e}"))?;
    if !status.is_success() {
        let hint = if status.as_u16() == 429 {
            " (rate-limited — wait a moment and try again)"
        } else {
            ""
        };
        return Err(format!(
            "{what} returned HTTP {}{hint}: {}",
            status.as_u16(),
            text.chars().take(200).collect::<String>()
        ));
    }
    Ok(text)
}

pub fn truncate(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

/// Collapse runs of whitespace to single spaces.
pub fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
