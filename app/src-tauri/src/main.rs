#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod runs;

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use soapstone_store::Store;
use tauri::Manager;

pub struct AppState {
    pub store: Mutex<Store>,
    pub client: soapstone_http::Client,
    /// Request id → cancel signal of the send in flight.
    pub runs: runs::Runs,
    pub started: Instant,
}

impl AppState {
    /// Recovers from a poisoned lock so one panic does not disable every later command.
    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn main() {
    let started = Instant::now();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = commands::on_big_stack(|| Store::open(&dir.join("soapstone.db")))?;
            app.manage(AppState { store: Mutex::new(store), client: soapstone_http::client(), runs: runs::Runs::default(), started });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::create_project,
            commands::rename,
            commands::delete,
            commands::get_tree,
            commands::import_service,
            commands::refresh_service,
            commands::get_service_source,
            commands::import_soapui_project,
            commands::get_request,
            commands::save_request,
            commands::create_request,
            commands::send_request,
            commands::cancel_send,
            commands::active_runs,
            commands::duplicate_request,
            commands::request_sample,
            commands::format_xml,
            commands::preview_request,
            commands::request_history,
            commands::history_response,
            commands::clear_history,
            commands::get_test_plan,
            commands::save_test_plan,
            commands::export_project,
            commands::import_project,
            commands::get_environments,
            commands::set_environments,
            commands::set_active_environment,
            commands::log_metric,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Soapstone");
}
