// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod lex;
mod llm;
mod settings;

use settings::Settings;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Forced auto-update: on launch, check the signed update endpoint. If a newer
/// version exists, download and install it, then relaunch — the user cannot
/// skip it. Fails OPEN: if the check errors (e.g. offline), we log and let the
/// app run, so legal research still works without a connection. To hard-block
/// instead, surface the error to the UI and refuse to continue.
async fn check_and_force_update(app: AppHandle) -> tauri_plugin_updater::Result<()> {
    let updater = app.updater()?;
    if let Some(update) = updater.check().await? {
        let _ = app.emit("update://available", &update.version);
        let progress_app = app.clone();
        let mut downloaded: u64 = 0;
        update
            .download_and_install(
                move |chunk, total| {
                    downloaded += chunk as u64;
                    let _ = progress_app.emit("update://progress", (downloaded, total));
                },
                move || {},
            )
            .await?;
        // Installed — relaunch into the new version.
        app.restart();
    }
    Ok(())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings::save(&app, &settings)
}

#[tauri::command]
async fn lex_search(app: AppHandle, params: lex::SearchParams) -> Result<serde_json::Value, String> {
    let s = settings::load(&app);
    lex::search(&s.lex_base_url, &s.contact, &params).await
}

#[tauri::command]
async fn lex_full_text(
    app: AppHandle,
    legislation_id: String,
    include_schedules: bool,
) -> Result<serde_json::Value, String> {
    let s = settings::load(&app);
    lex::full_text(&s.lex_base_url, &s.contact, &legislation_id, include_schedules).await
}

#[tauri::command]
async fn lex_health(app: AppHandle) -> Result<serde_json::Value, String> {
    let s = settings::load(&app);
    lex::health(&s.lex_base_url, &s.contact).await
}

#[tauri::command]
async fn llm_explain(app: AppHandle, question: String, context: String) -> Result<String, String> {
    let s = settings::load(&app);
    llm::explain(&s, &question, &context).await
}

/// Open an external URL (e.g. legislation.gov.uk) in the user's default browser.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    open::that(url).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = check_and_force_update(handle.clone()).await {
                    eprintln!("auto-update check failed (continuing): {e}");
                    let _ = handle.emit("update://error", e.to_string());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            lex_search,
            lex_full_text,
            lex_health,
            llm_explain,
            open_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running Curia desktop");
}
