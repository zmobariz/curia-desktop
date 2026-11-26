// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod lex;
mod llm;
mod settings;

use settings::Settings;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Forced auto-update for the PORTABLE build: on launch, check the signed update
/// endpoint. If a newer version exists, download it (the updater plugin verifies
/// the minisign signature against the embedded pubkey), swap the running .exe in
/// place, and relaunch — the user cannot skip it. Tauri's built-in installer only
/// supports NSIS/MSI, so for a portable single-exe we do the swap ourselves via
/// `self_replace` instead of `update.install`.
///
/// Fails OPEN: if anything errors (offline, 404 before the first release, a
/// read-only folder), we log and let the app run, so legal research still works.
async fn check_and_force_update(app: AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let updater = app.updater()?;
    if let Some(update) = updater.check().await? {
        let _ = app.emit("update://available", &update.version);
        let progress_app = app.clone();
        let mut downloaded: u64 = 0;
        // Downloads AND verifies the signature, returning the new exe as bytes.
        let bytes = update
            .download(
                move |chunk, total| {
                    downloaded += chunk as u64;
                    let _ = progress_app.emit("update://progress", (downloaded, total));
                },
                move || {},
            )
            .await?;

        // Write the verified bytes to a temp file, then atomically replace the
        // currently running portable exe with it (handles the Windows
        // can't-overwrite-a-running-exe rename dance).
        // Stage next to the running exe (same volume, so the swap is a rename)
        // under a per-process name, rather than a fixed name in %TEMP% that
        // another process could pre-create or race.
        let current = std::env::current_exe()?;
        let mut tmp = current.clone();
        tmp.set_file_name(format!(".curia-update-{}.exe", std::process::id()));
        std::fs::write(&tmp, &bytes)?;
        let swapped = self_replace::self_replace(&tmp);
        let _ = std::fs::remove_file(&tmp);
        swapped?;

        // Relaunch the now-updated exe and exit this (old) process.
        std::process::Command::new(current).spawn()?;
        app.exit(0);
    }
    Ok(())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    settings::load_public(&app)
}

#[tauri::command]
fn clear_api_key() -> Result<(), String> {
    settings::clear_key()
}

/// Settings for a Lex call, refusing a non-https base URL (e.g. one left in an
/// older settings file) so research queries are never sent in clear text.
fn lex_settings(app: &AppHandle) -> Result<Settings, String> {
    let s = settings::load(app);
    if !settings::is_safe_endpoint(&s.lex_base_url) {
        return Err("Lex base URL must use https:// — fix it in Settings.".into());
    }
    Ok(s)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings::save(&app, &settings)
}

#[tauri::command]
async fn lex_search(app: AppHandle, params: lex::SearchParams) -> Result<serde_json::Value, String> {
    let s = lex_settings(&app)?;
    lex::search(&s.lex_base_url, &s.contact, &params).await
}

#[tauri::command]
async fn lex_full_text(
    app: AppHandle,
    legislation_id: String,
    include_schedules: bool,
) -> Result<serde_json::Value, String> {
    let s = lex_settings(&app)?;
    lex::full_text(&s.lex_base_url, &s.contact, &legislation_id, include_schedules).await
}

#[tauri::command]
async fn lex_health(app: AppHandle) -> Result<serde_json::Value, String> {
    let s = lex_settings(&app)?;
    lex::health(&s.lex_base_url, &s.contact).await
}

#[tauri::command]
async fn lex_amendments(
    app: AppHandle,
    legislation_id: String,
    search_amended: bool,
) -> Result<serde_json::Value, String> {
    let s = lex_settings(&app)?;
    lex::amendments(&s.lex_base_url, &s.contact, &legislation_id, search_amended).await
}

#[tauri::command]
async fn lex_explanatory_notes(
    app: AppHandle,
    legislation_id: String,
) -> Result<serde_json::Value, String> {
    let s = lex_settings(&app)?;
    lex::explanatory_notes(&s.lex_base_url, &s.contact, &legislation_id).await
}

#[tauri::command]
async fn llm_explain(app: AppHandle, question: String, context: String) -> Result<String, String> {
    let s = settings::load(&app);
    llm::explain(&s, &question, &context).await
}

/// Links the UI may open in the user's browser.
const ALLOWED_OPEN_PREFIXES: &[&str] = &[
    "https://www.legislation.gov.uk/",
    "https://legislation.gov.uk/",
    "https://ollama.com/",
];

/// Open an external URL (e.g. legislation.gov.uk) in the user's default browser.
/// URLs can come from Lex API responses, so they are untrusted: `open::that`
/// hands its argument to the Windows shell, which would happily launch a local
/// program or file:// path. Only https links to allow-listed hosts are opened.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let mut u = url.trim().to_string();
    // Lex sometimes returns http:// legislation.gov.uk links; upgrade them.
    for host in ["http://www.legislation.gov.uk/", "http://legislation.gov.uk/"] {
        if u.starts_with(host) {
            u = format!("https://{}", &u["http://".len()..]);
        }
    }
    if u.chars().any(|c| c.is_control() || c.is_whitespace())
        || !ALLOWED_OPEN_PREFIXES.iter().any(|p| u.starts_with(p))
    {
        return Err("Refused to open a link outside legislation.gov.uk.".into());
    }
    open::that(u).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            // Only self-update release builds — a debug build would try to
            // replace the dev exe under target/debug.
            #[cfg(not(debug_assertions))]
            tauri::async_runtime::spawn(async move {
                if let Err(e) = check_and_force_update(handle.clone()).await {
                    eprintln!("auto-update check failed (continuing): {e}");
                    let _ = handle.emit("update://error", e.to_string());
                }
            });
            #[cfg(debug_assertions)]
            let _ = handle; // updater disabled in dev builds
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            clear_api_key,
            lex_search,
            lex_full_text,
            lex_health,
            lex_amendments,
            lex_explanatory_notes,
            llm_explain,
            open_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running Curia desktop");
}
