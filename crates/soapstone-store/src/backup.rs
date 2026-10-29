//! Portable project files. Responses, keychain values, and literal authentication secrets are omitted.
use crate::services::{documents_json, insert_operation_row, insert_service};
use crate::{Auth, Environments, FetchSettings, RequestEdit, Result, Store, StoreError, TestStep};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use soapstone_core::{Definition, Document};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Backup {
    format: String,
    version: u32,
    name: String,
    environments: Environments,
    services: Vec<Service>,
    #[serde(default)]
    steps: Vec<TestStep>,
}

#[derive(Serialize, Deserialize)]
struct Service {
    name: String,
    documents: Vec<Part>,
    fetch: FetchSettings,
    operations: Vec<Operation>,
}

#[derive(Serialize, Deserialize)]
struct Part {
    url: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct Operation {
    namespace: String,
    binding: String,
    name: String,
    soap_version: String,
    soap_action: Option<String>,
    endpoint: Option<String>,
    requests: Vec<SavedRequest>,
}

#[derive(Serialize, Deserialize)]
struct SavedRequest {
    name: String,
    edit: RequestEdit,
}

fn template_or_empty(value: &mut String) {
    if !(value.starts_with("{{") && value.ends_with("}}")) {
        value.clear();
    }
}

fn sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["password", "passwd", "secret", "token", "authorization", "cookie", "api-key", "apikey", "api_key"].iter().any(|p| name.contains(p))
}

fn sanitize_body(body: &str) -> String {
    let Ok(doc) = roxmltree::Document::parse(body) else { return body.to_string() };
    let mut ranges = Vec::new();
    for node in doc.descendants().filter(|n| n.is_text() && n.parent_element().is_some_and(|p| sensitive(p.tag_name().name()))) {
        if !node.text().is_some_and(|v| v.trim().starts_with("{{") && v.trim().ends_with("}}")) {
            ranges.push(node.range());
        }
    }
    let mut out = body.to_string();
    for range in ranges.into_iter().rev() {
        out.replace_range(range, "");
    }
    out
}

fn sanitize_edit(edit: &mut RequestEdit) {
    match &mut edit.auth {
        Auth::Basic { password, .. } | Auth::Ntlm { password, .. } => template_or_empty(password),
        Auth::Bearer { token } => template_or_empty(token),
        Auth::None => {}
    }
    template_or_empty(&mut edit.ws_security.password);
    template_or_empty(&mut edit.options.cert_password);
    // A certificate path is machine-specific and may disclose a local account name.
    template_or_empty(&mut edit.options.cert_path);
    for header in &mut edit.headers {
        if sensitive(&header.name) {
            template_or_empty(&mut header.value);
        }
    }
    edit.body = sanitize_body(&edit.body);
    if let Ok(mut url) = url::Url::parse(&edit.endpoint)
        && !url.password().unwrap_or("").is_empty()
    {
        let _ = url.set_password(None);
        edit.endpoint = url.to_string();
    }
}

impl Store {
    pub fn export_project(&self, project_id: i64) -> Result<String> {
        let name: String = self.conn.query_row("SELECT name FROM projects WHERE id = ?1", [project_id], |r| r.get(0))?;
        let mut environments = self.environments(project_id, false)?;
        for vars in std::iter::once(&mut environments.base).chain(environments.environments.iter_mut().map(|e| &mut e.variables)) {
            for variable in vars {
                if variable.secret || sensitive(&variable.name) {
                    variable.value.clear();
                    variable.secret = true;
                }
            }
        }
        let mut services = Vec::new();
        for service in self.tree(project_id)? {
            let json: String = self.conn.query_row("SELECT documents FROM services WHERE id = ?1", [service.id], |r| r.get(0))?;
            let mut fetch = self.service_source(service.id)?.fetch;
            template_or_empty(&mut fetch.password);
            let mut operations = Vec::new();
            for op in service.operations {
                let (soap_version, soap_action, endpoint) =
                    self.conn.query_row("SELECT soap_version, soap_action, endpoint FROM operations WHERE id = ?1", [op.id], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })?;
                let mut requests = Vec::new();
                for request in op.requests {
                    let r = self.request_config(request.id)?;
                    let mut edit = RequestEdit {
                        id: r.id,
                        endpoint: r.endpoint,
                        body: r.body,
                        headers: r.headers,
                        auth: r.auth,
                        ws_security: r.ws_security,
                        options: r.options,
                    };
                    sanitize_edit(&mut edit);
                    requests.push(SavedRequest { name: r.name, edit });
                }
                operations.push(Operation {
                    namespace: op.binding_namespace,
                    binding: op.binding,
                    name: op.name,
                    soap_version,
                    soap_action,
                    endpoint,
                    requests,
                });
            }
            services.push(Service { name: service.name, documents: serde_json::from_str(&json)?, fetch, operations });
        }
        let request_ids: std::collections::HashSet<_> =
            services.iter().flat_map(|s| &s.operations).flat_map(|o| &o.requests).map(|r| r.edit.id).collect();
        let steps = self.test_plan(project_id)?.into_iter().filter(|s| request_ids.contains(&s.request_id)).collect();
        Ok(serde_json::to_string_pretty(&Backup { format: "soapstone".into(), version: 1, name, environments, services, steps })?)
    }

    /// Preflight every WSDL before starting a transaction; never fetches anything from the network.
    pub fn import_project(&mut self, json: &str) -> Result<i64> {
        if json.len() > 64 * 1024 * 1024 {
            return Err(StoreError::Invalid("Project file exceeds 64 MB.".into()));
        }
        let mut backup: Backup = serde_json::from_str(json)?;
        if backup.format != "soapstone" || backup.version != 1 {
            return Err(StoreError::Invalid("Unsupported Soapstone project format.".into()));
        }
        let definitions = backup
            .services
            .iter()
            .map(|s| Definition::parse(s.documents.iter().map(|d| Document { url: d.url.clone(), content: d.content.clone() }).collect()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for vars in
            std::iter::once(&mut backup.environments.base).chain(backup.environments.environments.iter_mut().map(|e| &mut e.variables))
        {
            for v in vars.iter_mut().filter(|v| v.secret) {
                v.value.clear();
            }
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO projects (name, environments) VALUES (?1, ?2)",
            params![backup.name, serde_json::to_string(&backup.environments)?],
        )?;
        let project_id = tx.last_insert_rowid();
        let mut ids = HashMap::new();
        for (service, def) in backup.services.into_iter().zip(definitions) {
            let sid = insert_service(&tx, project_id, &def, &service.fetch)?;
            tx.execute(
                "UPDATE services SET name = ?1, documents = ?2 WHERE id = ?3",
                params![service.name, documents_json(def.documents())?, sid],
            )?;
            for op in service.operations {
                let source = def.operation_in(&op.namespace, &op.binding, &op.name);
                let oid = if let Some(source) = source {
                    let oid = insert_operation_row(&tx, sid, source)?;
                    tx.execute("UPDATE operations SET sample_body = ?1 WHERE id = ?2", params![def.sample_request(source), oid])?;
                    oid
                } else {
                    tx.execute("INSERT INTO operations (service_id, binding_namespace, binding, name, soap_version, soap_action, endpoint, orphan) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)", params![sid, op.namespace, op.binding, op.name, op.soap_version, op.soap_action, op.endpoint])?;
                    tx.last_insert_rowid()
                };
                for r in op.requests {
                    tx.execute("INSERT INTO requests (operation_id, name, endpoint, body, headers, auth, ws_security, options) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![oid, r.name, r.edit.endpoint, r.edit.body, serde_json::to_string(&r.edit.headers)?, serde_json::to_string(&r.edit.auth)?, serde_json::to_string(&r.edit.ws_security)?, serde_json::to_string(&r.edit.options)?])?;
                    ids.insert(r.edit.id, tx.last_insert_rowid());
                }
            }
        }
        let mut steps = backup.steps;
        for step in &mut steps {
            step.request_id =
                *ids.get(&step.request_id).ok_or_else(|| StoreError::Invalid("A sequence refers to a missing request.".into()))?;
        }
        tx.execute("INSERT INTO test_plans (project_id, steps) VALUES (?1, ?2)", params![project_id, serde_json::to_string(&steps)?])?;
        tx.commit()?;
        Ok(project_id)
    }
}
