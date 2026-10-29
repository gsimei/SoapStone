use std::collections::HashMap;
use soapstone_store::{Environment, Environments, Kind, Project, RequestOptions, Store, Variable, Vault, WsSecurity};

fn var(name: &str, value: &str) -> Variable { Variable { name: name.into(), value: value.into(), secret: false } }
fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn a_new_store_has_one_default_project() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.projects().unwrap(), vec![Project { id: 1, name: "My Project".into() }]);
}

#[test]
fn projects_can_be_created_renamed_and_deleted() {
    let s = Store::open_in_memory().unwrap();
    let id = s.create_project("Optitravel").unwrap();
    s.rename(Kind::Project, id, "Opti").unwrap();
    assert_eq!(s.projects().unwrap()[1].name, "Opti");
    s.delete(Kind::Project, id).unwrap();
    assert_eq!(s.projects().unwrap().len(), 1);
    assert!(s.rename(Kind::Project, 999, "x").is_err());
}

#[test]
fn environments_round_trip() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.environments(1, true).unwrap(), Environments::default());
    let envs = Environments {
        base: vec![var("a", "1")],
        environments: vec![Environment { name: "dev".into(), variables: vec![var("b", "2")] }],
        active: Some("dev".into()),
    };
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.environments(1, true).unwrap(), envs);
}

#[test]
fn active_environment_overrides_base_variables() {
    let s = Store::open_in_memory().unwrap();
    let mut envs = Environments {
        base: vec![var("a", "1"), var("b", "2"), var("", "ignored")],
        environments: vec![Environment { name: "dev".into(), variables: vec![var("b", "3"), var("c", "4")] }],
        active: None,
    };
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap(), map(&[("a", "1"), ("b", "2")]));
    envs.active = Some("dev".into());
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap(), map(&[("a", "1"), ("b", "3"), ("c", "4")]));
    envs.active = Some("gone".into());
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap(), map(&[("a", "1"), ("b", "2")]));
}

#[test]
fn data_survives_reopening_the_file() {
    let path = std::env::temp_dir().join(format!("soapstone-store-test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let s = Store::open(&path).unwrap();
        s.create_project("Kept").unwrap();
    }
    let s = Store::open(&path).unwrap();
    let names: Vec<String> = s.projects().unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["My Project", "Kept"]);
    std::fs::remove_file(&path).unwrap();
}

/// Phase 1b's schema, exactly as it shipped: a historical fixture for the migration test.
const SCHEMA_V1: &str = r#"
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
PRAGMA user_version = 1;
INSERT INTO projects (name) VALUES ('Old');
INSERT INTO services (project_id, name, source_url, documents) VALUES (1, 'S', 'file:///s.wsdl', '[]');
INSERT INTO operations (service_id, binding, name, soap_version) VALUES (1, 'B', 'Op', '1.1');
INSERT INTO requests (operation_id, name, endpoint, body) VALUES (1, 'Request 1', 'http://x', '<b/>');
"#;

#[test]
fn version_1_database_is_migrated() {
    let path = std::env::temp_dir().join(format!("soapstone-v1-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    rusqlite::Connection::open(&path).unwrap().execute_batch(SCHEMA_V1).unwrap();
    let s = Store::open(&path).unwrap();
    let r = s.request(1).unwrap();
    assert_eq!((r.endpoint.as_str(), r.ws_security, r.options), ("http://x", WsSecurity::default(), RequestOptions::default()));
    drop(s);
    let version: i64 = rusqlite::Connection::open(&path).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, 2);
    std::fs::remove_file(&path).unwrap();
}

#[test]
#[ignore = "touches the system keychain"]
fn system_keychain_round_trip() {
    let account = format!("test-{}", std::process::id());
    let vault = Vault::System;
    vault.set(&account, "x").unwrap();
    assert_eq!(vault.get(&account).unwrap().as_deref(), Some("x"));
    vault.delete(&account).unwrap();
    assert_eq!(vault.get(&account).unwrap(), None);
    vault.delete(&account).unwrap();
}
