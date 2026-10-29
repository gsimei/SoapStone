//! Soapstone local storage: projects, environments, services and requests in one SQLite file.

mod services;
mod soapui;
mod vault;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use services::*;
pub use soapui::*;
pub use vault::Vault;
pub use soapstone_http::{Auth, Header, HttpResponse, PasswordType, RequestOptions, WsSecurity};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Load(#[from] soapstone_core::LoadError),
    /// e.g. "request 42"
    #[error("{0} not found")]
    NotFound(String),
    #[error("Undefined variables: {}", .0.join(", "))]
    Variables(Vec<String>),
    #[error("System keychain: {0}")]
    Vault(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    pub value: String,
    /// Its value is kept in the system keychain, not in the database.
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub name: String,
    pub variables: Vec<Variable>,
}

/// A project's base variables, its named environments and the active one's name.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environments {
    pub base: Vec<Variable>,
    pub environments: Vec<Environment>,
    pub active: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Project,
    Service,
    Request,
}

impl Kind {
    fn table(self) -> &'static str {
        match self {
            Kind::Project => "projects",
            Kind::Service => "services",
            Kind::Request => "requests",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Project => "project",
            Kind::Service => "service",
            Kind::Request => "request",
        }
    }
}

const SCHEMA: &str = r#"
CREATE TABLE projects (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  environments TEXT NOT NULL DEFAULT '{"base":[],"environments":[],"active":null}'
);
CREATE TABLE services (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  source_url TEXT NOT NULL,
  documents TEXT NOT NULL
);
CREATE TABLE operations (
  id INTEGER PRIMARY KEY,
  service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
  binding TEXT NOT NULL,
  name TEXT NOT NULL,
  soap_version TEXT NOT NULL,
  soap_action TEXT,
  endpoint TEXT,
  orphan INTEGER NOT NULL DEFAULT 0,
  UNIQUE (service_id, binding, name)
);
CREATE TABLE requests (
  id INTEGER PRIMARY KEY,
  operation_id INTEGER NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  body TEXT NOT NULL,
  headers TEXT NOT NULL DEFAULT '[]',
  auth TEXT NOT NULL DEFAULT '{"type":"none"}',
  last_response TEXT
);
"#;

/// Phase 1c: request WS-Security and settings, WSDL download settings.
const MIGRATION_2: &str = r#"
ALTER TABLE requests ADD COLUMN ws_security TEXT NOT NULL DEFAULT '{}';
ALTER TABLE requests ADD COLUMN options TEXT NOT NULL DEFAULT '{}';
ALTER TABLE services ADD COLUMN fetch TEXT NOT NULL DEFAULT '{}';
"#;

pub struct Store {
    conn: Connection,
    vault: Vault,
}

/// Secret values of one project, as stored in its vault entry: scope ("" for base, else the
/// environment's name) → variable name → value.
type Secrets = BTreeMap<String, BTreeMap<String, String>>;

/// Each scope of `envs` with its variables: base as "", then every environment by name.
fn scopes(envs: &mut Environments) -> impl Iterator<Item = (&String, &mut Vec<Variable>)> {
    static BASE: String = String::new();
    std::iter::once((&BASE, &mut envs.base)).chain(envs.environments.iter_mut().map(|e| (&e.name, &mut e.variables)))
}

/// The database form of a project's environments: also how many keychain entries hold its secret values
/// (0 when none do), so the keychain is read only when it has something to give.
#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    envs: Environments,
    #[serde(default, rename = "keychainParts")]
    parts: usize,
}

/// Keychain entry `k` of a project's secrets: "project-1", "project-1.1", …
fn account(project_id: i64, k: usize) -> String {
    match k {
        0 => format!("project-{project_id}"),
        _ => format!("project-{project_id}.{k}"),
    }
}

/// Characters per keychain entry. Windows Credential Manager holds at most 2560 bytes per entry, and 600
/// characters stay under that even when every one takes 4 bytes.
const PART: usize = 600;

impl Store {
    /// Opens (creating if needed) the database file and its schema.
    /// Secrets go to the system keychain.
    pub fn open(path: &Path) -> Result<Store> {
        Store::init(Connection::open(path)?, Vault::System)
    }

    /// Secrets stay in memory.
    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?, Vault::memory())
    }

    pub fn with_vault(self, vault: Vault) -> Store {
        Store { vault, ..self }
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    fn init(conn: Connection, vault: Vault) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        // Each step in its own transaction; later phases append steps.
        if version < 1 {
            conn.execute_batch(&format!("BEGIN; {SCHEMA} PRAGMA user_version = 1; COMMIT;"))?;
        }
        if version < 2 {
            conn.execute_batch(&format!("BEGIN; {MIGRATION_2} PRAGMA user_version = 2; COMMIT;"))?;
        }
        conn.execute("INSERT INTO projects (name) SELECT 'My Project' WHERE NOT EXISTS (SELECT 1 FROM projects)", [])?;
        Ok(Store { conn, vault })
    }

    /// Ordered by id.
    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare("SELECT id, name FROM projects ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok(Project { id: r.get(0)?, name: r.get(1)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_project(&self, name: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO projects (name) VALUES (?1)", [name])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// NotFound when no such row.
    pub fn rename(&self, kind: Kind, id: i64, name: &str) -> Result<()> {
        let sql = format!("UPDATE {} SET name = ?1 WHERE id = ?2", kind.table());
        match self.conn.execute(&sql, params![name, id])? {
            0 => Err(StoreError::NotFound(format!("{} {id}", kind.noun()))),
            _ => Ok(()),
        }
    }

    /// Cascades to children; deleting a missing row is Ok. A project also loses its secrets.
    pub fn delete(&self, kind: Kind, id: i64) -> Result<()> {
        let parts = match kind {
            Kind::Project => self.stored_environments(id).map_or(0, |s| s.parts),
            _ => 0,
        };
        self.conn.execute(&format!("DELETE FROM {} WHERE id = ?1", kind.table()), [id])?;
        self.forget_secrets(id, 0..parts);
        Ok(())
    }

    /// Best effort: a leftover entry is harmless, and warning on every save without a keychain would be worse.
    fn forget_secrets(&self, project_id: i64, parts: std::ops::Range<usize>) {
        for k in parts {
            let _ = self.vault.delete(&account(project_id, k));
        }
    }

    fn stored_environments(&self, project_id: i64) -> Result<Stored> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT environments FROM projects WHERE id = ?1", [project_id], |r| r.get(0))
            .optional()?;
        let json = json.ok_or_else(|| StoreError::NotFound(format!("project {project_id}")))?;
        Ok(serde_json::from_str(&json)?)
    }

    fn store_environments(&self, project_id: i64, stored: &Stored) -> Result<()> {
        let json = serde_json::to_string(stored)?;
        match self.conn.execute("UPDATE projects SET environments = ?1 WHERE id = ?2", params![json, project_id])? {
            0 => Err(StoreError::NotFound(format!("project {project_id}"))),
            _ => Ok(()),
        }
    }

    /// With `reveal`, secret values come from the keychain (which may ask the user); without it they are
    /// empty and the keychain is not touched.
    pub fn environments(&self, project_id: i64, reveal: bool) -> Result<Environments> {
        let Stored { mut envs, parts } = self.stored_environments(project_id)?;
        if !reveal {
            for (_, vars) in scopes(&mut envs) {
                vars.iter_mut().filter(|v| v.secret).for_each(|v| v.value.clear());
            }
            return Ok(envs);
        }
        if parts == 0 {
            return Ok(envs); // secret values, if any, were kept in the database
        }
        let mut json = String::new();
        for k in 0..parts {
            let name = account(project_id, k);
            json += &self.vault.get(&name).map_err(StoreError::Vault)?.ok_or_else(|| {
                StoreError::Vault(format!("the entry {name} is missing. Enter the project's secret values again."))
            })?;
        }
        let secrets: Secrets = serde_json::from_str(&json)?;
        for (scope, vars) in scopes(&mut envs) {
            for v in vars.iter_mut().filter(|v| v.secret && v.value.is_empty()) {
                if let Some(value) = secrets.get(scope).and_then(|s| s.get(v.name.trim())) {
                    v.value = value.clone();
                }
            }
        }
        Ok(envs)
    }

    /// Secret values go to the keychain and are blanked in the database. When the keychain fails they are
    /// kept in the database instead, and the warning to show is returned.
    pub fn set_environments(&self, project_id: i64, environments: &Environments) -> Result<Option<String>> {
        let old = self.stored_environments(project_id)?.parts; // NotFound before touching the keychain
        let mut stored = Stored { envs: environments.clone(), parts: 0 };
        let mut secrets = Secrets::new();
        for (scope, vars) in scopes(&mut stored.envs) {
            for v in vars.iter().filter(|v| v.secret && !v.value.is_empty()) {
                secrets.entry(scope.clone()).or_default().insert(v.name.trim().to_string(), v.value.clone());
            }
        }
        let mut warning = None;
        if !secrets.is_empty() {
            let json: Vec<char> = serde_json::to_string(&secrets)?.chars().collect();
            let parts: Vec<String> = json.chunks(PART).map(|c| c.iter().collect()).collect();
            match parts.iter().enumerate().try_for_each(|(k, p)| self.vault.set(&account(project_id, k), p)) {
                Ok(()) => {
                    stored.parts = parts.len();
                    for (_, vars) in scopes(&mut stored.envs) {
                        vars.iter_mut().filter(|v| v.secret).for_each(|v| v.value.clear());
                    }
                }
                Err(e) => {
                    warning = Some(format!(
                        "The system keychain is unavailable ({e}). Secret values were saved in the local database instead."
                    ))
                }
            }
        }
        self.store_environments(project_id, &stored)?;
        self.forget_secrets(project_id, stored.parts..old);
        Ok(warning)
    }

    /// Changes only the active environment's name; secrets are not touched.
    pub fn set_active_environment(&self, project_id: i64, active: Option<&str>) -> Result<()> {
        let mut stored = self.stored_environments(project_id)?;
        stored.envs.active = active.map(String::from);
        self.store_environments(project_id, &stored)
    }

    /// Base variables overlaid by the active environment's; empty names skipped; an unknown active name means base only.
    pub fn variables(&self, project_id: i64) -> Result<HashMap<String, String>> {
        let envs = self.environments(project_id, true)?;
        let active = envs.active.as_deref().and_then(|a| envs.environments.iter().find(|e| e.name == a));
        Ok(envs
            .base
            .iter()
            .chain(active.into_iter().flat_map(|e| &e.variables))
            .filter(|v| !v.name.trim().is_empty())
            .map(|v| (v.name.trim().to_string(), v.value.clone()))
            .collect())
    }
}
