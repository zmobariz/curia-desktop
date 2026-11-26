//! Picking the passages of a long document that are relevant to a question.
//!
//! Documents (guidance, Rules sections, judgments) are split into blocks that
//! each carry the nearest anchor, so every passage can link to its exact place
//! in the source. Blocks are ranked with a small BM25-style keyword score —
//! no model or index needed, and the result is deterministic and auditable.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize)]
pub struct Block {
    /// Page this block belongs to (without fragment).
    pub url: String,
    /// Fragment id in the source page (e.g. "para_12" or "eligibility-requirements").
    pub anchor: Option<String>,
    /// Nearest heading above the block, for context.
    pub heading: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Passage {
    pub heading: Option<String>,
    pub text: String,
    /// Full URL including the #anchor where available.
    pub url: String,
    pub score: f32,
}

const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "has",
    "have", "how", "i", "if", "in", "is", "it", "its", "may", "must", "of", "on", "or", "should",
    "that", "the", "their", "there", "this", "to", "under", "was", "what", "when", "where",
    "which", "who", "will", "with", "would", "you", "your", "any", "not", "no", "my", "me",
];

pub fn terms(s: &str) -> Vec<String> {
    let stop: HashSet<&str> = STOP.iter().copied().collect();
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '(' || c == ')'))
        .map(|w| w.trim_matches(|c| c == '(' || c == ')'))
        .filter(|w| w.len() > 1 && !stop.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Return up to `k` of the most relevant blocks, in document order, merging a
/// little context.
pub fn best(blocks: &[Block], query: &str, k: usize, max_chars: usize) -> Vec<Passage> {
    let q: Vec<String> = {
        let mut seen = HashSet::new();
        terms(query).into_iter().filter(|t| seen.insert(t.clone())).collect()
    };
    if blocks.is_empty() {
        return vec![];
    }
    let docs: Vec<Vec<String>> = blocks
        .iter()
        .map(|b| {
            let mut t = terms(&b.text);
            if let Some(h) = &b.heading {
                // Headings count double: they say what the block is about.
                let ht = terms(h);
                t.extend(ht.clone());
                t.extend(ht);
            }
            t
        })
        .collect();
    let n = docs.len() as f32;
    let avg = docs.iter().map(|d| d.len()).sum::<usize>() as f32 / n.max(1.0);
    let mut df: HashMap<&str, f32> = HashMap::new();
    for d in &docs {
        let uniq: HashSet<&str> = d.iter().map(|s| s.as_str()).collect();
        for t in uniq {
            *df.entry(t).or_default() += 1.0;
        }
    }
    let (k1, b) = (1.2f32, 0.75f32);
    let mut scored: Vec<(usize, f32)> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut tf: HashMap<&str, f32> = HashMap::new();
            for t in d {
                *tf.entry(t.as_str()).or_default() += 1.0;
            }
            let len = d.len() as f32;
            let mut s = 0.0;
            for t in &q {
                let f = *tf.get(t.as_str()).unwrap_or(&0.0);
                if f == 0.0 {
                    continue;
                }
                let dfi = *df.get(t.as_str()).unwrap_or(&0.0);
                let idf = ((n - dfi + 0.5) / (dfi + 0.5) + 1.0).ln();
                s += idf * f * (k1 + 1.0) / (f + k1 * (1.0 - b + b * len / avg.max(1.0)));
            }
            (i, s)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut picked: Vec<(usize, f32)> = scored
        .into_iter()
        .filter(|(_, s)| *s > 0.0)
        .take(k)
        .collect();
    if picked.is_empty() {
        // Nothing matched: fall back to the opening of the document.
        picked = (0..blocks.len().min(2)).map(|i| (i, 0.0)).collect();
    }
    picked.sort_by_key(|(i, _)| *i);

    let per = (max_chars / picked.len().max(1)).max(300);
    picked
        .into_iter()
        .map(|(i, s)| {
            let blk = &blocks[i];
            // Short blocks (a single rule sub-paragraph) read better with the next one.
            let mut text = blk.text.clone();
            if text.len() < 200 {
                if let Some(next) = blocks.get(i + 1) {
                    text.push(' ');
                    text.push_str(&next.text);
                }
            }
            let url = match &blk.anchor {
                Some(a) => format!("{}#{}", blk.url, a),
                None => blk.url.clone(),
            };
            Passage {
                heading: blk.heading.clone(),
                text: crate::net::truncate(&text, per),
                url,
                score: s,
            }
        })
        .collect()
}

/// Merge consecutive small blocks that share a heading/anchor into chunks of
/// roughly `target` characters, so ranking works on meaningful units.
pub fn coalesce(blocks: Vec<Block>, target: usize) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for b in blocks {
        if let Some(last) = out.last_mut() {
            if last.url == b.url
                && last.anchor == b.anchor
                && last.heading == b.heading
                && last.text.len() + b.text.len() < target
            {
                last.text.push('\n');
                last.text.push_str(&b.text);
                continue;
            }
        }
        out.push(b);
    }
    out
}
