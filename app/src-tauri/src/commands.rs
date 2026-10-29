//! IPC commands. All are async so none runs on the main thread; errors reach JS as their message.

use std::collections::HashMap;
use std::fmt::Display;
use std::time::Instant;

use soapstone_core::{Document, FetchOptions, fetch_all_with, to_url};
use soapstone_http::{HttpResponse, SendError};
use soapstone_store::{
    Environments, FetchSettings, HistoryEntry, ImportReport, Kind, Project, Request, RequestEdit, ServiceNode, ServiceSource, TestStep,
    parse_soapui,
};
use tauri::{AppHandle, Manager, State};

use crate::AppState;

type Res<T> = Result<T, String>;

fn text(e: impl Display) -> String {
    e.to_string()
}

fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// ponytail: 64 MB stack because the core's schema recursion has no depth cap; a cap in core is the real fix.
pub(crate) fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
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
    blocking(app, move |state| state.store().create_request(operation_id).map_err(text)).await
}

/// Saves the edit, then sends it. A failed or cancelled send leaves the stored response unchanged.
#[tauri::command]
pub async fn send_request(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: RequestEdit,
    run_id: String,
    variables: Option<HashMap<String, String>>,
) -> Res<HttpResponse> {
    let (_guard, cancelled) = state.runs.start(edit.id, run_id)?;
    let saved = edit.clone();
    let work = async {
        let (call, mut vars) = blocking(app.clone(), move |state| {
            let store = state.store();
            store.save_request(&saved).map_err(text)?;
            store.prepare_send(saved.id).map_err(text)
        })
        .await?;
        if let Some(overrides) = variables {
            vars.extend(overrides);
        }
        soapstone_http::send(&state.client, &call, &vars).await.map_err(text)
    };
    let response = tokio::select! { biased;
        _ = cancelled => Err(SendError::Cancelled.to_string()),
        response = work => response,
    }?;
    // The HTTP exchange has completed. Persist atomically, without reporting a late cancel as a failed send.
    let stored = response.clone();
    blocking(app, move |state| state.store().save_response(edit.id, &stored).map_err(text)).await?;
    Ok(response)
}

#[tauri::command]
pub async fn cancel_send(state: State<'_, AppState>, request_id: i64, run_id: String) -> Res<()> {
    state.runs.cancel(request_id, &run_id);
    Ok(())
}

#[tauri::command]
pub async fn active_runs(state: State<'_, AppState>) -> Res<Vec<crate::runs::ActiveRun>> {
    Ok(state.runs.active())
}

#[tauri::command]
pub async fn duplicate_request(app: AppHandle, id: i64) -> Res<i64> {
    blocking(app, move |s| s.store().duplicate_request(id).map_err(text)).await
}

#[tauri::command]
pub async fn request_sample(app: AppHandle, id: i64) -> Res<String> {
    blocking(app, move |s| s.store().sample_for_request(id).map_err(text)).await
}

#[tauri::command]
pub async fn format_xml(xml: String) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        soapstone_http::validate_xml(&xml)?;
        soapstone_http::pretty_xml(&xml).ok_or_else(|| "Could not format XML".to_string())
    })
    .await
    .map_err(text)?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    endpoint: String,
    headers: Vec<soapstone_store::Header>,
    body: String,
}

#[tauri::command]
pub async fn preview_request(app: AppHandle, edit: RequestEdit) -> Res<Preview> {
    blocking(app, move |s| {
        let store = s.store();
        let (mut call, vars) = store.prepare_send(edit.id).map_err(text)?;
        call.endpoint = edit.endpoint;
        call.body = edit.body;
        call.auth = edit.auth;
        call.headers = edit.headers;
        call.ws_security = edit.ws_security;
        call.options = edit.options;
        let secret_names = store.secret_variable_names(edit.id).map_err(text)?;
        drop(store);
        build_preview(&call, &vars, &secret_names)
    })
    .await
}

fn build_preview(
    call: &soapstone_http::SoapCall,
    vars: &HashMap<String, String>,
    secret_names: &std::collections::HashSet<String>,
) -> Res<Preview> {
    let prepared = soapstone_http::prepare(call, vars).map_err(text)?;
    let mut secrets: Vec<String> =
        vars.iter().filter(|(name, v)| secret_names.contains(*name) && !v.is_empty()).map(|(_, value)| value.clone()).collect();
    match &call.auth {
        soapstone_store::Auth::Basic { password, .. } | soapstone_store::Auth::Ntlm { password, .. } => {
            secrets.push(soapstone_http::render(password, vars).map_err(|v| v.join(", "))?)
        }
        soapstone_store::Auth::Bearer { token } => secrets.push(soapstone_http::render(token, vars).map_err(|v| v.join(", "))?),
        soapstone_store::Auth::None => {}
    }
    if call.ws_security.username_token {
        secrets.push(soapstone_http::render(&call.ws_security.password, vars).map_err(|v| v.join(", "))?);
    }
    secrets.sort_by_key(|v| std::cmp::Reverse(v.len()));
    let redact = |value: String| {
        secrets
            .iter()
            .filter(|v| !v.is_empty())
            .fold(value, |out, v| out.replace(&soapstone_http::escape_xml(v), "[redacted]").replace(v, "[redacted]"))
    };
    let mut headers = match call.soap_version {
        soapstone_core::SoapVersion::V11 => vec![
            soapstone_store::Header { name: "Content-Type".into(), value: "text/xml;charset=UTF-8".into() },
            soapstone_store::Header { name: "SOAPAction".into(), value: format!("\"{}\"", call.soap_action.as_deref().unwrap_or("")) },
        ],
        soapstone_core::SoapVersion::V12 => vec![soapstone_store::Header {
            name: "Content-Type".into(),
            value: format!(
                "application/soap+xml;charset=UTF-8{}",
                call.soap_action.as_ref().filter(|v| !v.is_empty()).map(|v| format!(";action=\"{v}\"")).unwrap_or_default()
            ),
        }],
    };
    if !matches!(call.auth, soapstone_store::Auth::None) {
        headers.push(soapstone_store::Header { name: "Authorization".into(), value: "[redacted]".into() });
    }
    for h in &prepared.headers {
        headers.retain(|a| !a.name.eq_ignore_ascii_case(&h.name));
    }
    headers.extend(prepared.headers.into_iter().map(|mut h| {
        h.value = if ["authorization", "cookie", "token", "key", "secret"].iter().any(|n| h.name.to_ascii_lowercase().contains(n)) {
            "[redacted]".into()
        } else {
            redact(h.value)
        };
        h
    }));
    let mut body = prepared.body;
    if let Ok(doc) = soapstone_http::password_ranges(&body) {
        for range in doc.into_iter().rev() {
            body.replace_range(range, "[redacted]");
        }
    }
    let mut endpoint = prepared.endpoint;
    if let Ok(mut url) = soapstone_core::to_url(&endpoint) {
        let password = url.password().is_some();
        let query: Vec<_> = url
            .query_pairs()
            .map(|(name, value)| {
                let sensitive = ["password", "token", "secret", "key"].iter().any(|key| name.to_ascii_lowercase().contains(key));
                (name.to_string(), if sensitive { "[redacted]".to_string() } else { value.to_string() }, sensitive)
            })
            .collect();
        if password {
            let _ = url.set_password(Some("[redacted]"));
        }
        let query_secret = query.iter().any(|(_, _, secret)| *secret);
        if query_secret {
            url.query_pairs_mut().clear().extend_pairs(query.iter().map(|(name, value, _)| (name, value)));
        }
        if password || query_secret {
            endpoint = url.to_string();
        }
    }
    Ok(Preview { endpoint: redact(endpoint), headers, body: redact(body) })
}

#[tauri::command]
pub async fn request_history(app: AppHandle, id: i64) -> Res<Vec<HistoryEntry>> {
    blocking(app, move |s| s.store().history(id).map_err(text)).await
}
#[tauri::command]
pub async fn history_response(app: AppHandle, id: i64) -> Res<HttpResponse> {
    blocking(app, move |s| s.store().history_response(id).map_err(text)).await
}
#[tauri::command]
pub async fn clear_history(app: AppHandle, id: i64) -> Res<()> {
    blocking(app, move |s| s.store().clear_history(id).map_err(text)).await
}
#[tauri::command]
pub async fn get_test_plan(app: AppHandle, project_id: i64) -> Res<Vec<TestStep>> {
    blocking(app, move |s| s.store().test_plan(project_id).map_err(text)).await
}
#[tauri::command]
pub async fn save_test_plan(app: AppHandle, project_id: i64, steps: Vec<TestStep>) -> Res<()> {
    blocking(app, move |s| s.store().save_test_plan(project_id, &steps).map_err(text)).await
}
#[tauri::command]
pub async fn export_project(app: AppHandle, project_id: i64, path: String) -> Res<()> {
    blocking(app, move |s| {
        let json = s.store().export_project(project_id).map_err(text)?;
        std::fs::write(path, json).map_err(text)
    })
    .await
}
#[tauri::command]
pub async fn import_project(app: AppHandle, path: String) -> Res<i64> {
    blocking(app, move |s| {
        if std::fs::metadata(&path).map_err(text)?.len() > 64 * 1024 * 1024 {
            return Err("Project exceeds 64 MB".into());
        }
        let json = std::fs::read_to_string(path).map_err(text)?;
        on_big_stack(|| s.store().import_project(&json)).map_err(text)
    })
    .await
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_resolves_normal_values_and_masks_xml_and_http_credentials() {
        let call = soapstone_http::SoapCall {
            endpoint: "{{host}}/service".into(),
            soap_version: soapstone_core::SoapVersion::V11,
            soap_action: Some("Op".into()),
            headers: vec![soapstone_store::Header { name: "X-Token".into(), value: "{{secret}}".into() }],
            auth: soapstone_store::Auth::Basic { username: "user".into(), password: "literal-password".into() },
            body: "<r><Password>inline-password</Password><Value>{{secret}}</Value><Normal>{{normal}}</Normal></r>".into(),
            ws_security: Default::default(),
            options: Default::default(),
        };
        let vars = HashMap::from([
            ("host".into(), "http://localhost".into()),
            ("normal".into(), "visible".into()),
            ("secret".into(), "demo&<secret".into()),
        ]);
        let preview = build_preview(&call, &vars, &std::collections::HashSet::from(["secret".into()])).unwrap();
        assert_eq!(preview.endpoint, "http://localhost/service");
        let output = format!("{} {:?}", preview.body, preview.headers);
        for secret in ["literal-password", "inline-password", "demo", "&lt;secret"] {
            assert!(!output.contains(secret), "leaked {secret}");
        }
        assert!(preview.body.contains("<Normal>visible</Normal>"));
        assert!(preview.headers.iter().any(|h| h.name == "Authorization" && h.value == "[redacted]"));
    }
}
