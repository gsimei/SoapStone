use std::path::{Path, PathBuf};

use soapstone_store::{Environment, Environments, Kind, Store, StoreError, Variable, Vault};

fn var(name: &str, value: &str, secret: bool) -> Variable {
    Variable { name: name.into(), value: value.into(), secret }
}

fn envs() -> Environments {
    Environments {
        base: vec![var("token", "t0p", true)],
        environments: vec![Environment { name: "prod".into(), variables: vec![var("pw", "p@ss", true), var("host", "h", false)] }],
        active: None,
    }
}

/// A store in a temp file with an in-memory vault, so a test can read the raw JSON next to it.
fn store(test: &str) -> (Store, PathBuf) {
    let path = std::env::temp_dir().join(format!("soapstone-{test}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    (Store::open(&path).unwrap().with_vault(Vault::memory()), path)
}

fn raw(path: &Path, project: i64) -> String {
    rusqlite::Connection::open(path).unwrap().query_row("SELECT environments FROM projects WHERE id = ?1", [project], |r| r.get(0)).unwrap()
}

#[test]
fn secret_values_live_in_the_vault_not_in_the_database() {
    let (s, path) = store("live");
    assert_eq!(s.set_environments(1, &envs()).unwrap(), None);
    let json = raw(&path, 1);
    assert!(!json.contains("t0p") && !json.contains("p@ss") && json.contains("\"h\""), "{json}");
    assert_eq!(s.environments(1, true).unwrap(), envs());
    let hidden = s.environments(1, false).unwrap();
    let prod = &hidden.environments[0].variables;
    assert_eq!((hidden.base[0].value.as_str(), prod[0].value.as_str(), prod[1].value.as_str()), ("", "", "h"));
    s.set_environments(1, &Environments { active: Some("prod".into()), ..envs() }).unwrap();
    let vars = s.variables(1).unwrap();
    assert_eq!((vars["token"].as_str(), vars["pw"].as_str(), vars["host"].as_str()), ("t0p", "p@ss", "h"));
}

#[test]
fn switching_the_active_environment_keeps_secrets() {
    let (s, _) = store("switch");
    s.set_environments(1, &envs()).unwrap();
    s.set_active_environment(1, Some("prod")).unwrap();
    assert_eq!(s.variables(1).unwrap()["pw"], "p@ss");
    assert_eq!(s.environments(1, false).unwrap().active.as_deref(), Some("prod"));
}

#[test]
fn unmarking_or_removing_secrets_updates_the_vault() {
    let (s, path) = store("unmark");
    s.set_environments(1, &envs()).unwrap();
    let mut plain = envs();
    plain.base[0].secret = false;
    s.set_environments(1, &plain).unwrap();
    assert!(raw(&path, 1).contains("t0p"));
    let entry = s.vault().get("project-1").unwrap().unwrap();
    assert!(!entry.contains("t0p") && entry.contains("p@ss"), "{entry}");
    plain.environments[0].variables[0].secret = false;
    s.set_environments(1, &plain).unwrap();
    assert_eq!(s.vault().get("project-1").unwrap(), None);
}

#[test]
fn deleting_a_project_deletes_its_secrets() {
    let (s, _) = store("delete");
    let p = s.create_project("P").unwrap();
    s.set_environments(p, &envs()).unwrap();
    let account = format!("project-{p}");
    assert!(s.vault().get(&account).unwrap().is_some());
    s.delete(Kind::Project, p).unwrap();
    assert_eq!(s.vault().get(&account).unwrap(), None);
}

#[test]
fn unavailable_keychain_falls_back_with_a_warning() {
    let (s, path) = store("unavailable");
    let s = s.with_vault(Vault::Unavailable);
    let warning = s.set_environments(1, &envs()).unwrap().expect("a warning");
    assert!(warning.starts_with("The system keychain is unavailable"), "{warning}");
    assert!(raw(&path, 1).contains("t0p"));
    assert_eq!(s.variables(1).unwrap()["token"], "t0p");
}

#[test]
fn unreadable_keychain_is_an_error_not_an_empty_secret() {
    let (s, _) = store("unreadable");
    s.set_environments(1, &envs()).unwrap();
    let s = s.with_vault(Vault::Unavailable);
    assert!(matches!(s.variables(1), Err(StoreError::Vault(_))));
    assert!(s.environments(1, false).is_ok(), "hidden values never touch the keychain");
}

#[test]
fn empty_secret_without_a_keychain_does_not_lock_the_project() {
    // Review 1c #1: "Secret" ticked before typing a value, on a machine whose keychain fails.
    let (s, _) = store("empty-secret");
    let s = s.with_vault(Vault::Unavailable);
    let envs = Environments { base: vec![var("token", "", true), var("host", "h", false)], ..Default::default() };
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap()["host"], "h");
    assert_eq!(s.environments(1, true).unwrap(), envs);
}

#[test]
fn a_secret_missing_from_the_keychain_is_an_error() {
    let (s, _) = store("missing-entry");
    s.set_environments(1, &envs()).unwrap();
    s.vault().delete("project-1").unwrap();
    assert!(matches!(s.variables(1), Err(StoreError::Vault(_))));
}

#[test]
fn long_secret_values_are_split_across_entries() {
    // Review 1c #2: Windows Credential Manager holds at most 2560 bytes per entry.
    let (s, _) = store("long");
    let long = Environments { base: vec![var("jwt", &"x".repeat(2500), true)], ..Default::default() };
    s.set_environments(1, &long).unwrap();
    let Vault::Memory(entries) = s.vault() else { unreachable!() };
    assert!(entries.borrow().values().all(|v| v.chars().count() <= 1100));
    assert!(entries.borrow().contains_key("project-1.2"));
    assert_eq!(s.variables(1).unwrap()["jwt"].len(), 2500);

    s.set_environments(1, &envs()).unwrap();
    assert!(!entries.borrow().contains_key("project-1.1"), "parts of the old value are removed");
    assert_eq!(s.variables(1).unwrap()["token"], "t0p");
    s.delete(Kind::Project, 1).unwrap();
    assert!(entries.borrow().is_empty());
}
