//! Updates tracker: watch Home Office guidance / Immigration Rules sections and
//! see exactly what changed, like a tracked-changes view.
//!
//! Snapshots of watched documents are kept as plain text in the per-user app
//! config folder. On "Check now" each watched document's GOV.UK
//! `public_updated_at` is compared; when it moves, the new text is fetched,
//! diffed against the snapshot, and the change is recorded with GOV.UK's own
//! change note. An optional AI summary uses the user's own provider key.

use crate::govuk;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MAX_DIFF_LINES: usize = 600;
const MAX_CHANGES_KEPT: usize = 300;

#[derive(Serialize, Deserialize, Clone)]
pub struct Watched {
    pub path: String,
    pub title: String,
    pub updated: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DiffLine {
    /// "+", "-" or " " (context)
    pub tag: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChangeRec {
    pub id: String,
    pub path: String,
    pub title: String,
    pub from: String,
    pub to: String,
    pub note: String,
    pub added: usize,
    pub removed: usize,
    pub diff: Vec<DiffLine>,
    #[serde(default)]
    pub summary: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    pub watched: Vec<Watched>,
    pub changes: Vec<ChangeRec>,
    #[serde(default)]
    pub last_checked: String,
}

#[derive(Serialize)]
pub struct CheckResult {
    pub checked: usize,
    pub changed: usize,
    pub errors: Vec<String>,
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("tracker");
    fs::create_dir_all(d.join("snap")).map_err(|e| e.to_string())?;
    Ok(d)
}

fn snap_path(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let safe: String = path
        .trim_matches('/')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    Ok(dir(app)?.join("snap").join(format!("{safe}.txt")))
}

pub fn load(app: &AppHandle) -> State {
    dir(app)
        .ok()
        .and_then(|d| fs::read_to_string(d.join("state.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, st: &State) -> Result<(), String> {
    let d = dir(app)?;
    let data = serde_json::to_string_pretty(st).map_err(|e| e.to_string())?;
    fs::write(d.join("state.json"), data).map_err(|e| e.to_string())
}

pub async fn watch(app: &AppHandle, s: &Settings, path: &str) -> Result<State, String> {
    let client = crate::net::client(&s.contact)?;
    let doc = govuk::fetch(&client, path).await?;
    let mut st = load(app);
    if !st.watched.iter().any(|w| w.path == doc.path) {
        fs::write(snap_path(app, &doc.path)?, govuk::plain_text(&doc)).map_err(|e| e.to_string())?;
        st.watched.push(Watched {
            path: doc.path.clone(),
            title: doc.title.clone(),
            updated: doc.updated.clone(),
        });
        save(app, &st)?;
    }
    Ok(st)
}

pub fn unwatch(app: &AppHandle, path: &str) -> Result<State, String> {
    let mut st = load(app);
    st.watched.retain(|w| w.path != path);
    if let Ok(p) = snap_path(app, path) {
        // Snapshot is ours to manage; ignore if already gone.
        let _ = fs::remove_file(p);
    }
    save(app, &st)?;
    Ok(st)
}

pub async fn check(app: &AppHandle, s: &Settings, now: &str) -> Result<CheckResult, String> {
    let client = crate::net::client(&s.contact)?;
    let mut st = load(app);
    let mut res = CheckResult { checked: 0, changed: 0, errors: vec![] };
    for i in 0..st.watched.len() {
        let w = st.watched[i].clone();
        res.checked += 1;
        let (_, updated, history) = match govuk::meta(&client, &w.path).await {
            Ok(m) => m,
            Err(e) => {
                res.errors.push(format!("{}: {e}", w.title));
                continue;
            }
        };
        if updated == w.updated {
            continue;
        }
        let doc = match govuk::fetch(&client, &w.path).await {
            Ok(d) => d,
            Err(e) => {
                res.errors.push(format!("{}: {e}", w.title));
                continue;
            }
        };
        let new_text = govuk::plain_text(&doc);
        let sp = snap_path(app, &w.path)?;
        let old_text = fs::read_to_string(&sp).unwrap_or_default();
        let (diff, added, removed) = diff_lines(&old_text, &new_text);
        let note = history.first().map(|c| c.note.clone()).unwrap_or_default();
        st.changes.insert(
            0,
            ChangeRec {
                id: format!("{}@{}", w.path, updated),
                path: w.path.clone(),
                title: doc.title.clone(),
                from: w.updated.clone(),
                to: updated.clone(),
                note,
                added,
                removed,
                diff,
                summary: String::new(),
            },
        );
        fs::write(&sp, new_text).map_err(|e| e.to_string())?;
        st.watched[i].updated = updated;
        st.watched[i].title = doc.title;
        res.changed += 1;
    }
    st.changes.truncate(MAX_CHANGES_KEPT);
    st.last_checked = now.to_string();
    save(app, &st)?;
    Ok(res)
}

fn diff_lines(old: &str, new: &str) -> (Vec<DiffLine>, usize, usize) {
    use similar::{ChangeTag, TextDiff};
    let d = TextDiff::from_lines(old, new);
    let (mut added, mut removed) = (0, 0);
    let mut out = vec![];
    for group in d.grouped_ops(2) {
        if !out.is_empty() {
            out.push(DiffLine { tag: "…".into(), text: String::new() });
        }
        for op in group {
            for ch in d.iter_changes(&op) {
                let tag = match ch.tag() {
                    ChangeTag::Insert => {
                        added += 1;
                        "+"
                    }
                    ChangeTag::Delete => {
                        removed += 1;
                        "-"
                    }
                    ChangeTag::Equal => " ",
                };
                if out.len() < MAX_DIFF_LINES {
                    out.push(DiffLine {
                        tag: tag.into(),
                        text: ch.to_string().trim_end().to_string(),
                    });
                }
            }
        }
    }
    (out, added, removed)
}

pub async fn summarise(app: &AppHandle, s: &Settings, id: &str) -> Result<String, String> {
    let mut st = load(app);
    let Some(idx) = st.changes.iter().position(|c| c.id == id) else {
        return Err("Change not found.".into());
    };
    let c = st.changes[idx].clone();
    let mut diff = String::new();
    for l in &c.diff {
        diff.push_str(&l.tag);
        diff.push(' ');
        diff.push_str(&l.text);
        diff.push('\n');
        if diff.len() > 24_000 {
            break;
        }
    }
    let system = "You summarise changes to UK Home Office immigration guidance and the Immigration \
Rules for immigration lawyers. Use British English. Describe only what the diff shows: lines \
starting '+' were added, '-' removed. Lead with the substantive legal or policy effect (who is \
affected and how), then list notable wording changes. Ignore pure formatting. If the diff is \
too partial to be sure, say so. Do not speculate beyond the text. 150 words maximum.";
    let user = format!(
        "Document: {}\nPublished change note: {}\n\nDiff:\n{}",
        c.title, c.note, diff
    );
    let summary = crate::llm::complete(s, system, &user, 700).await?;
    st.changes[idx].summary = summary.clone();
    save(app, &st)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    #[test]
    fn diff_marks_changes_with_context() {
        let old = "## Eligibility\nA must be 18.\nB must pay the fee.\nC applies.\n## Suitability\nD.\n";
        let new = "## Eligibility\nA must be 21.\nB must pay the fee.\nC applies.\n## Suitability\nD.\nE added.\n";
        let (d, added, removed) = super::diff_lines(old, new);
        assert_eq!((added, removed), (2, 1));
        assert!(d.iter().any(|l| l.tag == "-" && l.text == "A must be 18."));
        assert!(d.iter().any(|l| l.tag == "+" && l.text == "A must be 21."));
        assert!(d.iter().any(|l| l.tag == "+" && l.text == "E added."));
    }
}
