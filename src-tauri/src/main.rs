// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod lex;
mod llm;
mod settings;

use settings::Settings;
use tauri::AppHandle;

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
