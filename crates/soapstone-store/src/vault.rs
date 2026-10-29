//! Where secret variable values live: the operating system's keychain.

use std::cell::RefCell;
use std::collections::HashMap;

const SERVICE: &str = "io.github.gsimei.soapstone";

/// One entry per account (`project-<id>`), each a text value.
pub enum Vault {
    /// macOS Keychain, Windows Credential Manager or the Secret Service on Linux.
    System,
    /// For tests and the in-memory store.
    Memory(RefCell<HashMap<String, String>>),
    /// Fails every call: a machine without a usable keychain.
    Unavailable,
}

impl Vault {
    pub fn memory() -> Vault {
        Vault::Memory(RefCell::default())
    }

    pub fn get(&self, account: &str) -> Result<Option<String>, String> {
        match self {
            Vault::System => match entry(account)?.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            Vault::Memory(map) => Ok(map.borrow().get(account).cloned()),
            Vault::Unavailable => Err(unavailable()),
        }
    }

    pub fn set(&self, account: &str, value: &str) -> Result<(), String> {
        match self {
            Vault::System => entry(account)?.set_password(value).map_err(|e| e.to_string()),
            Vault::Memory(map) => {
                map.borrow_mut().insert(account.into(), value.into());
                Ok(())
            }
            Vault::Unavailable => Err(unavailable()),
        }
    }

    /// A missing entry is Ok.
    pub fn delete(&self, account: &str) -> Result<(), String> {
        match self {
            Vault::System => match entry(account)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Vault::Memory(map) => {
                map.borrow_mut().remove(account);
                Ok(())
            }
            Vault::Unavailable => Err(unavailable()),
        }
    }
}

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

fn unavailable() -> String {
    "no keychain on this machine".into()
}
