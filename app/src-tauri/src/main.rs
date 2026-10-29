#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use soapstone_store::Store;
use tauri::Manager;
use tokio::sync::oneshot;

pub struct AppState {
    pub store: Mutex<Store>,
    pub client: soapstone_http::Client,
    /// Request id → cancel signal of the send in flight.
    pub inflight: Mutex<HashMap<i64, oneshot::Sender<()>>>,
    pub started: Instant,
}

impl AppState {
    /// Recovers from a poisoned lock so one panic does not disable every later command.
    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn inflight(&self) -> MutexGuard<'_, HashMap<i64, oneshot::Sender<()>>> {
        self.inflight.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn main() {
    let started = Instant::now();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(&dir.join("soapstone.db"))?;
            app.manage(AppState {
                store: Mutex::new(store),
                client: soapstone_http::client(),
                inflight: Mutex::default(),
                started,
            });
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
            commands::get_environments,
            commands::set_environments,
            commands::set_active_environment,
            commands::log_metric,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Soapstone");
}
