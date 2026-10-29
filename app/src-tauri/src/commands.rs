//! IPC commands. All are async so none runs on the main thread; errors reach JS as their message.

use std::fmt::Display;
use std::time::Instant;

use soapstone_core::{Document, FetchOptions, fetch_all_with, to_url};
use soapstone_http::{HttpResponse, SendError};
use soapstone_store::{Environments, FetchSettings, ImportReport, Kind, Project, Request, RequestEdit, ServiceNode, ServiceSource, parse_soapui};
use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;

use crate::AppState;

type Res<T> = Result<T, String>;

fn text(e: impl Display) -> String {
    e.to_string()
}

fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// ponytail: 64 MB stack because the core's schema recursion has no depth cap; a cap in core is the real fix.
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(s, f)
            .expect("spawn worker thread")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// Blocking: downloads (or reads) the WSDL and everything it imports.
fn fetch(source: &str, options: &FetchOptions) -> Res<Vec<Document>> {
    let start = Instant::now();
    let documents = fetch_all_with(&to_url(source).map_err(text)?, options).map_err(text)?;
    eprintln!("metric fetch: {:.1} ms ({} documents)", ms_since(start), documents.len());
    Ok(documents)
}

fn kb(documents: &[Document]) -> usize {
    documents.iter().map(|d| d.content.len()).sum::<usize>() / 1024
}

/// Runs blocking work off the async runtime with the app state at hand. Anything that may touch the system
/// keychain runs here: a keychain prompt must not hold an async worker.
async fn blocking<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppState) -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>())).await.map_err(text)?
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Res<Vec<Project>> {
    state.store().projects().map_err(text)
}

#[tauri::command]
pub async fn create_project(state: State<'_, AppState>, name: String) -> Res<i64> {
    state.store().create_project(&name).map_err(text)
}

#[tauri::command]
pub async fn rename(state: State<'_, AppState>, kind: Kind, id: i64, name: String) -> Res<()> {
    state.store().rename(kind, id, &name).map_err(text)
}

#[tauri::command]
pub async fn delete(app: AppHandle, kind: Kind, id: i64) -> Res<()> {
    blocking(app, move |state| state.store().delete(kind, id).map_err(text)).await
}

#[tauri::command]
pub async fn get_tree(state: State<'_, AppState>, project_id: i64) -> Res<Vec<ServiceNode>> {
    state.store().tree(project_id).map_err(text)
}

#[tauri::command]
pub async fn import_service(app: AppHandle, project_id: i64, source: String, fetch: FetchSettings) -> Res<i64> {
    blocking(app, move |state| {
        let options = state.store().fetch_options(project_id, &fetch).map_err(text)?;
        let documents = self::fetch(source.trim(), &options)?;
        let size = kb(&documents);
        let start = Instant::now();
        let id = on_big_stack(|| state.store().import_service(project_id, documents, &fetch)).map_err(text)?;
        eprintln!("metric import: {:.1} ms ({size} KB)", ms_since(start));
        Ok(id)
    })
    .await
}

/// With `source` and `fetch`, downloads from there and keeps them only when the refresh succeeds;
/// otherwise uses the stored ones.
#[tauri::command]
pub async fn refresh_service(app: AppHandle, service_id: i64, source: Option<String>, fetch: Option<FetchSettings>) -> Res<()> {
    blocking(app, move |state| {
        let stored = state.store().service_source(service_id).map_err(text)?;
        let (url, fetch) = match (source, fetch) {
            (Some(source), Some(fetch)) => (source.trim().to_string(), fetch),
            _ => (stored.url, stored.fetch),
        };
        let options = state.store().fetch_options(stored.project_id, &fetch).map_err(text)?;
        let documents = self::fetch(&url, &options)?;
        let size = kb(&documents);
        let start = Instant::now();
        on_big_stack(|| state.store().refresh_service(service_id, documents, &fetch)).map_err(text)?;
        eprintln!("metric import: {:.1} ms ({size} KB)", ms_since(start));
        Ok(())
    })
    .await
}

/// A SoapUI `.xml` project becomes a new project, offline (from SoapUI's cached WSDLs).
#[tauri::command]
pub async fn import_soapui_project(app: AppHandle, path: String) -> Res<ImportReport> {
    blocking(app, move |state| {
        let xml = std::fs::read_to_string(path.trim()).map_err(|e| format!("Cannot read {path}: {e}"))?;
        let start = Instant::now();
        let report = on_big_stack(|| state.store().import_soapui(parse_soapui(&xml)?).map_err(text))?;
        eprintln!("metric soapui-import: {:.1} ms ({} KB)", ms_since(start), xml.len() / 1024);
        Ok(report)
    })
    .await
}

#[tauri::command]
pub async fn get_service_source(state: State<'_, AppState>, service_id: i64) -> Res<ServiceSource> {
    state.store().service_source(service_id).map_err(text)
}

#[tauri::command]
pub async fn get_request(state: State<'_, AppState>, id: i64) -> Res<Request> {
    state.store().request(id).map_err(text)
}

#[tauri::command]
pub async fn save_request(state: State<'_, AppState>, edit: RequestEdit) -> Res<()> {
    state.store().save_request(&edit).map_err(text)
}

#[tauri::command]
pub async fn create_request(app: AppHandle, operation_id: i64) -> Res<i64> {
    blocking(app, move |state| on_big_stack(|| state.store().create_request(operation_id)).map_err(text)).await
}

/// Saves the edit, then sends it. A failed or cancelled send leaves the stored response unchanged.
#[tauri::command]
pub async fn send_request(app: AppHandle, state: State<'_, AppState>, edit: RequestEdit) -> Res<HttpResponse> {
    let saved = edit.clone();
    let (call, vars) = blocking(app, move |state| {
        let store = state.store();
        store.save_request(&saved).map_err(text)?;
        store.prepare_send(saved.id).map_err(text)
    })
    .await?;
    let (cancel, cancelled) = oneshot::channel();
    state.inflight().insert(edit.id, cancel);
    // Only an explicit cancel stops the send; a dropped sender (a second send of the same request) does not.
    let result = tokio::select! {
        r = soapstone_http::send(&state.client, &call, &vars) => r,
        Ok(()) = cancelled => Err(SendError::Cancelled),
    };
    state.inflight().remove(&edit.id);
    let response = result.map_err(text)?;
    state.store().save_response(edit.id, &response).map_err(text)?;
    Ok(response)
}

#[tauri::command]
pub async fn cancel_send(state: State<'_, AppState>, request_id: i64) -> Res<()> {
    if let Some(cancel) = state.inflight().remove(&request_id) {
        let _ = cancel.send(());
    }
    Ok(())
}

/// `reveal`: secret values from the system keychain; otherwise they are empty.
#[tauri::command]
pub async fn get_environments(app: AppHandle, project_id: i64, reveal: bool) -> Res<Environments> {
    blocking(app, move |state| state.store().environments(project_id, reveal).map_err(text)).await
}

/// Returns the warning to show when secrets could not go to the system keychain.
#[tauri::command]
pub async fn set_environments(app: AppHandle, project_id: i64, environments: Environments) -> Res<Option<String>> {
    blocking(app, move |state| state.store().set_environments(project_id, &environments).map_err(text)).await
}

#[tauri::command]
pub async fn set_active_environment(state: State<'_, AppState>, project_id: i64, active: Option<String>) -> Res<()> {
    state.store().set_active_environment(project_id, active.as_deref()).map_err(text)
}

/// `ms` absent: time since the process started.
#[tauri::command]
pub async fn log_metric(state: State<'_, AppState>, name: String, ms: Option<f64>) -> Res<()> {
    let ms = ms.unwrap_or_else(|| ms_since(state.started));
    eprintln!("metric {name}: {ms:.1} ms");
    Ok(())
}
