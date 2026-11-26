//! The user's OWN chosen LLM provider (bring your own key) — Curia ships no
//! model and requires no subscription.
//!
//! Supported providers:
//!   - "anthropic"         -> https://api.anthropic.com/v1/messages
//!   - "openai"            -> https://api.openai.com/v1/chat/completions
//!   - "openai_compatible" -> {base_url}/chat/completions (OpenRouter, Groq, z.ai, ...)
//!   - "ollama"            -> http://localhost:11434/v1/chat/completions (local, free)
//!
//! Two entry points: `complete` (single prompt -> text) and `step` (one round
//! of a tool-using conversation, used by the Assistant).

use crate::settings::Settings;
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-5-5";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";

const EXPLAIN_PROMPT: &str = "You are a careful UK legal-research assistant. Answer ONLY using the \
provided source extracts from official UK sources. Quote the relevant section numbers and titles, \
and link nothing you were not given. If the provided sources do not contain the answer, say so \
plainly — do NOT rely on prior knowledge and do NOT invent citations. Use British English. End \
with a short reminder that this is research assistance, not legal advice, and that the user \
should open and verify the linked primary source.";

pub enum Provider {
    Anthropic { key: String, model: String },
    OpenAi { base: String, key: String, model: String, official: bool },
}

impl Provider {
    pub fn is_anthropic(&self) -> bool {
        matches!(self, Provider::Anthropic { .. })
    }
}

pub fn resolve(s: &Settings) -> Result<Provider, String> {
    if !s.llm_enabled {
        return Err("AI features are turned off. Turn them on in Settings and add your own API key.".into());
    }
    if s.provider == "anthropic" {
        if s.api_key.trim().is_empty() {
            return Err("Add your Anthropic API key in Settings.".into());
        }
        let model = if s.model.trim().is_empty() {
            DEFAULT_ANTHROPIC_MODEL.to_string()
        } else {
            s.model.trim().to_string()
        };
        return Ok(Provider::Anthropic { key: s.api_key.trim().to_string(), model });
    }
    let base = match s.provider.as_str() {
        "openai" => "https://api.openai.com/v1".to_string(),
        "ollama" => {
            if s.base_url.trim().is_empty() {
                "http://localhost:11434/v1".to_string()
            } else {
                s.base_url.trim().trim_end_matches('/').to_string()
            }
        }
        "openai_compatible" => {
            if s.base_url.trim().is_empty() {
                return Err("Set the provider Base URL in Settings (e.g. https://openrouter.ai/api/v1).".into());
            }
            s.base_url.trim().trim_end_matches('/').to_string()
        }
        other => return Err(format!("Unknown provider: {}", other)),
    };
    if !crate::settings::is_safe_endpoint(&base) {
        return Err("Refusing to send your prompt and API key over plain http to a non-local host. Use an https:// Base URL.".into());
    }
    if s.provider != "ollama" && s.api_key.trim().is_empty() {
        return Err("Add your provider API key in Settings.".into());
    }
    let model = if s.model.trim().is_empty() {
        DEFAULT_OPENAI_MODEL.to_string()
    } else {
        s.model.trim().to_string()
    };
    Ok(Provider::OpenAi { official: s.provider == "openai", base, key: s.api_key.trim().to_string(), model })
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

/// Legacy single-shot "Explain" over supplied extracts.
pub async fn explain(s: &Settings, question: &str, context: &str) -> Result<String, String> {
    let user = format!(
        "Question:\n{}\n\nSource extracts retrieved from official sources:\n{}",
        question, context
    );
    complete(s, EXPLAIN_PROMPT, &user, 1500).await
}

/// One prompt in, text out.
pub async fn complete(s: &Settings, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    let p = resolve(s)?;
    let mut msgs = vec![json!({ "role": "user", "content": user })];
    let out = step(&p, system, &mut msgs, &[], false, max_tokens).await?;
    Ok(out.text)
}

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

pub struct Call {
    pub id: String,
    pub name: String,
    pub args: Value,
}

pub struct StepOut {
    pub text: String,
    pub calls: Vec<Call>,
}

/// One model round. Appends the assistant message to `msgs` in the provider's
/// own format. When `allow_tools` is false the model must answer in text.
pub async fn step(
    p: &Provider,
    system: &str,
    msgs: &mut Vec<Value>,
    tools: &[ToolDef],
    allow_tools: bool,
    max_tokens: u32,
) -> Result<StepOut, String> {
    let client = http()?;
    match p {
        Provider::Anthropic { key, model } => {
            let mut body = json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": msgs,
            });
            if !tools.is_empty() {
                body["tools"] = Value::Array(
                    tools
                        .iter()
                        .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}))
                        .collect(),
                );
                body["tool_choice"] = if allow_tools { json!({"type": "auto"}) } else { json!({"type": "none"}) };
            }
            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let v = json_or_err(resp).await?;
            let content = v["content"].clone();
            let mut text = String::new();
            let mut calls = vec![];
            for blk in content.as_array().cloned().unwrap_or_default() {
                match blk["type"].as_str() {
                    Some("text") => text.push_str(blk["text"].as_str().unwrap_or("")),
                    Some("tool_use") => calls.push(Call {
                        id: blk["id"].as_str().unwrap_or("").to_string(),
                        name: blk["name"].as_str().unwrap_or("").to_string(),
                        args: blk["input"].clone(),
                    }),
                    _ => {}
                }
            }
            if !content.is_array() {
                return Err(format!("Unexpected Anthropic response: {}", trunc(&v)));
            }
            msgs.push(json!({ "role": "assistant", "content": content }));
            Ok(StepOut { text, calls })
        }
        Provider::OpenAi { base, key, model, official } => {
            let mut all = vec![json!({ "role": "system", "content": system })];
            all.extend(msgs.iter().cloned());
            let mut body = json!({ "model": model, "messages": all });
            // OpenAI's newer models only accept max_completion_tokens; most
            // compatible servers (Ollama, Groq, OpenRouter) still use max_tokens.
            body[if *official { "max_completion_tokens" } else { "max_tokens" }] = json!(max_tokens);
            if !tools.is_empty() {
                body["tools"] = Value::Array(
                    tools
                        .iter()
                        .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}}))
                        .collect(),
                );
                body["tool_choice"] = json!(if allow_tools { "auto" } else { "none" });
            }
            let mut req = client.post(format!("{}/chat/completions", base)).json(&body);
            if !key.is_empty() {
                req = req.bearer_auth(key);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let v = json_or_err(resp).await?;
            let msg = v["choices"][0]["message"].clone();
            if !msg.is_object() {
                return Err(format!("Unexpected response: {}", trunc(&v)));
            }
            let text = msg["content"].as_str().unwrap_or("").to_string();
            let mut calls = vec![];
            for tc in msg["tool_calls"].as_array().cloned().unwrap_or_default() {
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                calls.push(Call {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    args: serde_json::from_str(args_str).unwrap_or(json!({})),
                });
            }
            // Store only the fields the API accepts back.
            let mut stored = json!({ "role": "assistant", "content": msg["content"].clone() });
            if msg["tool_calls"].is_array() {
                stored["tool_calls"] = msg["tool_calls"].clone();
            }
            msgs.push(stored);
            Ok(StepOut { text, calls })
        }
    }
}

/// Append tool results after a `step` that requested tools.
pub fn push_results(p: &Provider, msgs: &mut Vec<Value>, results: Vec<(String, String)>) {
    if p.is_anthropic() {
        let blocks: Vec<Value> = results
            .into_iter()
            .map(|(id, content)| json!({"type": "tool_result", "tool_use_id": id, "content": content}))
            .collect();
        msgs.push(json!({ "role": "user", "content": blocks }));
    } else {
        for (id, content) in results {
            msgs.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
        }
    }
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
