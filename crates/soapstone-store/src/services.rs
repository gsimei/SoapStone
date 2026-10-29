//! Imported services, their operations and saved requests.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use soapstone_core::{Definition, Document, FetchOptions, Operation, SoapVersion};
use soapstone_http::{SoapCall, render};

use crate::{Auth, Header, HttpResponse, RequestOptions, Result, Store, StoreError, WsSecurity};

const WSDL_NS: &str = "http://schemas.xmlsoap.org/wsdl/";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceNode {
    pub id: i64,
    pub name: String,
    pub operations: Vec<OperationNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationNode {
    pub id: i64,
    pub binding: String,
    pub name: String,
    pub orphan: bool,
    pub requests: Vec<RequestNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestNode {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: i64,
    pub name: String,
    pub operation: String,
    pub binding: String,
    /// "1.1" | "1.2"
    pub soap_version: String,
    pub soap_action: Option<String>,
    pub endpoint: String,
    pub body: String,
    pub headers: Vec<Header>,
    pub auth: Auth,
    pub ws_security: WsSecurity,
    pub options: RequestOptions,
    pub last_response: Option<HttpResponse>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEdit {
    pub id: i64,
    pub endpoint: String,
    pub body: String,
    pub headers: Vec<Header>,
    pub auth: Auth,
    #[serde(default)]
    pub ws_security: WsSecurity,
    #[serde(default)]
    pub options: RequestOptions,
}

/// How a service's WSDL is downloaded; username and password may use `{{variables}}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchSettings {
    pub username: String,
    pub password: String,
    pub insecure: bool,
}

/// Where a service's WSDL came from, for refreshing it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSource {
    pub project_id: i64,
    pub url: String,
    pub fetch: FetchSettings,
}

/// One entry of `services.documents`: `[{"url":..,"content":..}]`, root WSDL first.
#[derive(Serialize, Deserialize)]
struct StoredDocument<'a> {
    url: Cow<'a, str>,
    content: Cow<'a, str>,
}

pub(crate) fn documents_json(documents: &[Document]) -> Result<String> {
    let stored: Vec<_> = documents
        .iter()
        .map(|d| StoredDocument { url: Cow::Borrowed(&d.url), content: Cow::Borrowed(&d.content) })
        .collect();
    Ok(serde_json::to_string(&stored)?)
}

fn parse_documents(json: &str) -> Result<Definition> {
    let stored: Vec<StoredDocument> = serde_json::from_str(json)?;
    let documents = stored.into_iter().map(|d| Document { url: d.url.into_owned(), content: d.content.into_owned() }).collect();
    Ok(Definition::parse(documents)?)
}

fn version(v: SoapVersion) -> &'static str {
    match v {
        SoapVersion::V11 => "1.1",
        SoapVersion::V12 => "1.2",
    }
}

fn not_found(noun: &str, id: i64) -> StoreError {
    StoreError::NotFound(format!("{noun} {id}"))
}

/// `wsdl:service/@name` of the root document, else the URL's last path segment without extension, else "Service".
fn service_name(documents: &[Document]) -> String {
    let root = &documents[0];
    let from_wsdl = roxmltree::Document::parse(&root.content).ok().and_then(|doc| {
        doc.descendants()
            .find(|n| n.has_tag_name((WSDL_NS, "service")))
            .and_then(|n| n.attribute("name").map(String::from))
    });
    let from_url = || {
        let path = root.url.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
        let segment = path.rsplit('/').next().unwrap_or("");
        let stem = segment.rsplit_once('.').map_or(segment, |(stem, _)| stem);
        (!stem.is_empty()).then(|| stem.to_string())
    };
    from_wsdl.or_else(from_url).unwrap_or_else(|| "Service".into())
}

/// Each (binding, name) once, the first wins: overloaded operations (Java, .NET) repeat a name in one binding.
pub(crate) fn unique_operations(def: &Definition) -> impl Iterator<Item = &Operation> {
    let mut seen = HashSet::new();
    def.operations().iter().filter(move |op| seen.insert((op.binding.as_str(), op.name.as_str())))
}

/// A service row for `def`, named from the WSDL; its source is the root document's URL. Returns its id.
pub(crate) fn insert_service(conn: &Connection, project_id: i64, def: &Definition, fetch: &FetchSettings) -> Result<i64> {
    conn.execute(
        "INSERT INTO services (project_id, name, source_url, documents, fetch) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            project_id,
            service_name(def.documents()),
            def.documents()[0].url,
            documents_json(def.documents())?,
            serde_json::to_string(fetch)?
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// An operation row without requests. Returns its id.
pub(crate) fn insert_operation_row(conn: &Connection, service_id: i64, op: &Operation) -> Result<i64> {
    conn.execute(
        "INSERT INTO operations (service_id, binding, name, soap_version, soap_action, endpoint) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![service_id, op.binding, op.name, version(op.soap_version), op.soap_action, op.endpoint],
    )?;
    Ok(conn.last_insert_rowid())
}

/// An operation row plus its "Request 1" with the sample body.
pub(crate) fn insert_operation(conn: &Connection, def: &Definition, service_id: i64, op: &Operation) -> Result<()> {
    let operation_id = insert_operation_row(conn, service_id, op)?;
    conn.execute(
        "INSERT INTO requests (operation_id, name, endpoint, body) VALUES (?1, 'Request 1', ?2, ?3)",
        params![operation_id, op.endpoint.as_deref().unwrap_or(""), def.sample_request(op)],
    )?;
    Ok(())
}

impl Store {
    /// Parses `documents` (root first); one service, its operations, and "Request 1" with the sample for each.
    /// Returns the service id. Nothing is stored when the documents are not a WSDL.
    pub fn import_service(&mut self, project_id: i64, documents: Vec<Document>, fetch: &FetchSettings) -> Result<i64> {
        let def = Definition::parse(documents)?;
        let tx = self.conn.transaction()?;
        let service_id = insert_service(&tx, project_id, &def, fetch)?;
        for op in unique_operations(&def) {
            insert_operation(&tx, &def, service_id, op)?;
        }
        tx.commit()?;
        Ok(service_id)
    }

    /// Replaces the documents; updates or adds operations (new ones get "Request 1"); marks the missing ones
    /// orphan; never touches requests or the service name. Keeps `fetch` for the next refresh.
    pub fn refresh_service(&mut self, service_id: i64, documents: Vec<Document>, fetch: &FetchSettings) -> Result<()> {
        let def = Definition::parse(documents)?;
        let tx = self.conn.transaction()?;
        let updated = tx.execute(
            "UPDATE services SET source_url = ?1, documents = ?2, fetch = ?3 WHERE id = ?4",
            params![def.documents()[0].url, documents_json(def.documents())?, serde_json::to_string(fetch)?, service_id],
        )?;
        if updated == 0 {
            return Err(not_found("service", service_id));
        }
        tx.execute("UPDATE operations SET orphan = 1 WHERE service_id = ?1", [service_id])?;
        for op in unique_operations(&def) {
            let kept = tx.execute(
                "UPDATE operations SET soap_version = ?1, soap_action = ?2, endpoint = ?3, orphan = 0
                 WHERE service_id = ?4 AND binding = ?5 AND name = ?6",
                params![version(op.soap_version), op.soap_action, op.endpoint, service_id, op.binding, op.name],
            )?;
            if kept == 0 {
                insert_operation(&tx, &def, service_id, op)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn service_source(&self, service_id: i64) -> Result<ServiceSource> {
        let (project_id, url, fetch): (i64, String, String) = self
            .conn
            .query_row("SELECT project_id, source_url, fetch FROM services WHERE id = ?1", [service_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?
            .ok_or_else(|| not_found("service", service_id))?;
        Ok(ServiceSource { project_id, url, fetch: serde_json::from_str(&fetch)? })
    }

    /// `fetch` with its `{{variables}}` rendered from the project's active environment.
    pub fn fetch_options(&self, project_id: i64, fetch: &FetchSettings) -> Result<FetchOptions> {
        let vars = self.variables(project_id)?;
        let mut missing: Vec<String> = Vec::new();
        let mut r = |template: &str| {
            render(template, &vars).unwrap_or_else(|names| {
                missing.extend(names.into_iter().filter(|n| !missing.contains(n)).collect::<Vec<_>>());
                String::new()
            })
        };
        let options = FetchOptions { username: r(&fetch.username), password: r(&fetch.password), insecure: fetch.insecure };
        if missing.is_empty() { Ok(options) } else { Err(StoreError::Variables(missing)) }
    }

    /// Services, operations and requests of a project, each ordered by id. Empty for an unknown project.
    pub fn tree(&self, project_id: i64) -> Result<Vec<ServiceNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, o.id, o.binding, o.name, o.orphan, r.id, r.name
             FROM services s
             LEFT JOIN operations o ON o.service_id = s.id
             LEFT JOIN requests r ON r.operation_id = o.id
             WHERE s.project_id = ?1
             ORDER BY s.id, o.id, r.id",
        )?;
        let mut rows = stmt.query([project_id])?;
        let mut tree: Vec<ServiceNode> = Vec::new();
        while let Some(row) = rows.next()? {
            let service_id: i64 = row.get(0)?;
            if tree.last().is_none_or(|s| s.id != service_id) {
                tree.push(ServiceNode { id: service_id, name: row.get(1)?, operations: vec![] });
            }
            let Some(op_id) = row.get::<_, Option<i64>>(2)? else { continue };
            let ops = &mut tree.last_mut().expect("pushed above").operations;
            if ops.last().is_none_or(|o| o.id != op_id) {
                ops.push(OperationNode {
                    id: op_id,
                    binding: row.get(3)?,
                    name: row.get(4)?,
                    orphan: row.get(5)?,
                    requests: vec![],
                });
            }
            if let Some(request_id) = row.get::<_, Option<i64>>(6)? {
                let op = ops.last_mut().expect("pushed above");
                op.requests.push(RequestNode { id: request_id, name: row.get(7)? });
            }
        }
        Ok(tree)
    }

    pub fn request(&self, id: i64) -> Result<Request> {
        let row = self
            .conn
            .query_row(
                "SELECT r.id, r.name, o.name, o.binding, o.soap_version, o.soap_action, r.endpoint, r.body,
                        r.headers, r.auth, r.last_response, r.ws_security, r.options
                 FROM requests r JOIN operations o ON o.id = r.operation_id
                 WHERE r.id = ?1",
                [id],
                |r| {
                    let request = Request {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        operation: r.get(2)?,
                        binding: r.get(3)?,
                        soap_version: r.get(4)?,
                        soap_action: r.get(5)?,
                        endpoint: r.get(6)?,
                        body: r.get(7)?,
                        headers: vec![],
                        auth: Auth::None,
                        ws_security: WsSecurity::default(),
                        options: RequestOptions::default(),
                        last_response: None,
                    };
                    let json: [String; 4] = [r.get(8)?, r.get(9)?, r.get(11)?, r.get(12)?];
                    Ok((request, json, r.get::<_, Option<String>>(10)?))
                },
            )
            .optional()?;
        let (mut request, [headers, auth, ws_security, options], last_response) = row.ok_or_else(|| not_found("request", id))?;
        request.headers = serde_json::from_str(&headers)?;
        request.auth = serde_json::from_str(&auth)?;
        request.ws_security = serde_json::from_str(&ws_security)?;
        request.options = serde_json::from_str(&options)?;
        request.last_response = last_response.as_deref().map(serde_json::from_str).transpose()?;
        Ok(request)
    }

    pub fn save_request(&self, edit: &RequestEdit) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE requests SET endpoint = ?1, body = ?2, headers = ?3, auth = ?4, ws_security = ?5, options = ?6 WHERE id = ?7",
            params![
                edit.endpoint,
                edit.body,
                serde_json::to_string(&edit.headers)?,
                serde_json::to_string(&edit.auth)?,
                serde_json::to_string(&edit.ws_security)?,
                serde_json::to_string(&edit.options)?,
                edit.id
            ],
        )?;
        if updated == 0 {
            return Err(not_found("request", edit.id));
        }
        Ok(())
    }

    /// "Request {n+1}" with a fresh sample (empty body for an orphan operation).
    pub fn create_request(&self, operation_id: i64) -> Result<i64> {
        let (binding, name, endpoint, documents, count): (String, String, Option<String>, String, i64) = self
            .conn
            .query_row(
                "SELECT o.binding, o.name, o.endpoint, s.documents,
                        (SELECT COUNT(*) FROM requests WHERE operation_id = o.id)
                 FROM operations o JOIN services s ON s.id = o.service_id
                 WHERE o.id = ?1",
                [operation_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| not_found("operation", operation_id))?;
        let def = parse_documents(&documents)?;
        let body = def.operation(&binding, &name).map(|op| def.sample_request(op)).unwrap_or_default();
        self.conn.execute(
            "INSERT INTO requests (operation_id, name, endpoint, body) VALUES (?1, ?2, ?3, ?4)",
            params![operation_id, format!("Request {}", count + 1), endpoint.unwrap_or_default(), body],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The request as stored plus its operation's version and action, and the project's variables.
    pub fn prepare_send(&self, request_id: i64) -> Result<(SoapCall, HashMap<String, String>)> {
        let request = self.request(request_id)?;
        let project_id: i64 = self.conn.query_row(
            "SELECT s.project_id FROM requests r
             JOIN operations o ON o.id = r.operation_id JOIN services s ON s.id = o.service_id
             WHERE r.id = ?1",
            [request_id],
            |r| r.get(0),
        )?;
        let call = SoapCall {
            endpoint: request.endpoint,
            soap_version: if request.soap_version == "1.2" { SoapVersion::V12 } else { SoapVersion::V11 },
            soap_action: request.soap_action,
            headers: request.headers,
            auth: request.auth,
            body: request.body,
            ws_security: request.ws_security,
            options: request.options,
        };
        Ok((call, self.variables(project_id)?))
    }

    pub fn save_response(&self, request_id: i64, response: &HttpResponse) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE requests SET last_response = ?1 WHERE id = ?2",
            params![serde_json::to_string(response)?, request_id],
        )?;
        if updated == 0 {
            return Err(not_found("request", request_id));
        }
        Ok(())
    }
}
