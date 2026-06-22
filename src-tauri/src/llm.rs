//! Optional "Explain" feature. Calls the user's OWN chosen LLM provider with
//! their own API key — Curia ships no model and requires no subscription.
//!
//! Supported providers:
//!   - "openai"            -> https://api.openai.com/v1/chat/completions
//!   - "openai_compatible" -> {base_url}/chat/completions (OpenRouter, Groq, z.ai, ...)
//!   - "ollama"            -> http://localhost:11434/v1/chat/completions (local, free)
//!   - "anthropic"         -> https://api.anthropic.com/v1/messages

use crate::settings::Settings;
use serde_json::Value;

const SYSTEM_PROMPT: &str = "You are a careful UK legal-research assistant. Answer ONLY using the \
provided source extracts from official UK legislation. Quote the relevant section numbers and Act \
titles, and link nothing you were not given. If the provided sources do not contain the answer, say \
so plainly — do NOT rely on prior knowledge and do NOT invent citations. End with a short reminder \
that this is research assistance, not legal advice, and that the user should open and verify the \
linked primary source.";

pub async fn explain(s: &Settings, question: &str, context: &str) -> Result<String, String> {
    if !s.llm_enabled {
        return Err("AI features are turned off. Turn them on in Settings and add your own API key.".into());
    }
    let user = format!(
        "Question:\n{}\n\nSource extracts retrieved from legislation.gov.uk via the Lex API:\n{}",
        question, context
    );
    let client = reqwest::Client::new();

    if s.provider == "anthropic" {
        if s.api_key.trim().is_empty() {
            return Err("Add your Anthropic API key in Settings.".into());
        }
        let model = if s.model.trim().is_empty() {
            "claude-opus-4-8".to_string()
        } else {
            s.model.clone()
        };
        let body = serde_json::json!({
            "model": model,
            "max_tokens": 1024,
            "system": SYSTEM_PROMPT,
            "messages": [{ "role": "user", "content": user }],
        });
        let resp = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", s.api_key.trim())
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let v = json_or_err(resp).await?;
        return v["content"][0]["text"]
            .as_str()
            .map(|x| x.to_string())
            .ok_or_else(|| format!("Unexpected Anthropic response: {}", trunc(&v)));
    }

    // OpenAI-compatible providers.
    let base = match s.provider.as_str() {
        "openai" => "https://api.openai.com/v1".to_string(),
        "ollama" => {
            if s.base_url.trim().is_empty() {
                "http://localhost:11434/v1".to_string()
            } else {
                s.base_url.trim_end_matches('/').to_string()
            }
        }
        "openai_compatible" => {
            if s.base_url.trim().is_empty() {
                return Err("Set the provider Base URL in Settings (e.g. https://openrouter.ai/api/v1).".into());
            }
            s.base_url.trim_end_matches('/').to_string()
        }
        other => return Err(format!("Unknown provider: {}", other)),
    };
    if s.provider != "ollama" && s.api_key.trim().is_empty() {
        return Err("Add your provider API key in Settings.".into());
    }
    let model = if s.model.trim().is_empty() {
        "gpt-4o-mini".to_string()
    } else {
        s.model.clone()
    };
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user }
        ],
    });
    let mut req = client
        .post(format!("{}/chat/completions", base))
        .json(&body);
    if !s.api_key.trim().is_empty() {
        req = req.bearer_auth(s.api_key.trim());
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let v = json_or_err(resp).await?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(|x| x.to_string())
        .ok_or_else(|| format!("Unexpected response: {}", trunc(&v)))
}

async fn json_or_err(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Provider error {}: {}",
            status.as_u16(),
            text.chars().take(300).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn trunc(v: &Value) -> String {
    v.to_string().chars().take(300).collect()
}
