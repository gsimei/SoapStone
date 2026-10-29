//! Soapstone HTTP: render `{{variables}}` and send one SOAP request.

mod ntlm;
mod send;
mod vars;
mod wss;
mod xml;

use serde::{Deserialize, Serialize};

pub use reqwest::Client;
pub use send::{client, send};
pub use soapstone_core::SoapVersion;
pub use vars::render;
pub use xml::{decode_body, fault_string, pretty_xml};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// JSON: `{"type":"none"}`, `{"type":"basic","username":..,"password":..}`, `{"type":"bearer","token":..}` or
/// `{"type":"ntlm","username":..,"password":..,"domain":..}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Auth {
    #[default]
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    Ntlm {
        username: String,
        password: String,
        domain: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasswordType {
    #[default]
    Text,
    Digest,
}

/// WS-Security of one request. JSON camelCase; missing fields take their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WsSecurity {
    pub username_token: bool,
    pub username: String,
    pub password: String,
    pub password_type: PasswordType,
    pub nonce: bool,
    pub created: bool,
    pub timestamp: bool,
    pub ttl_seconds: u32,
}

impl Default for WsSecurity {
    fn default() -> Self {
        WsSecurity {
            username_token: false,
            username: String::new(),
            password: String::new(),
            password_type: PasswordType::Text,
            nonce: true,
            created: true,
            timestamp: false,
            ttl_seconds: 300,
        }
    }
}

impl WsSecurity {
    pub fn enabled(&self) -> bool {
        self.username_token || self.timestamp
    }
}

/// Connection settings of one request. JSON camelCase; missing fields take their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestOptions {
    /// 0 counts as 1.
    pub timeout_seconds: u32,
    /// Accept invalid TLS certificates (test servers only).
    pub insecure: bool,
    /// Client certificate, `.p12`/`.pfx`; empty for none.
    pub cert_path: String,
    pub cert_password: String,
}

impl Default for RequestOptions {
    fn default() -> Self {
        RequestOptions { timeout_seconds: 60, insecure: false, cert_path: String::new(), cert_password: String::new() }
    }
}

/// Everything needed to send one request, before variables are rendered.
#[derive(Debug, Clone, PartialEq)]
pub struct SoapCall {
    pub endpoint: String,
    pub soap_version: SoapVersion,
    pub soap_action: Option<String>,
    pub headers: Vec<Header>,
    pub auth: Auth,
    pub body: String,
    pub ws_security: WsSecurity,
    pub options: RequestOptions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    /// Canonical reason phrase, "" when unknown.
    pub status_text: String,
    /// From the start of the send until the whole body was read.
    pub time_ms: u64,
    /// Body size in bytes.
    pub size: u64,
    /// In received order, one entry per value.
    pub headers: Vec<Header>,
    pub body: String,
    /// Formatted XML, None when the body is not XML.
    pub pretty: Option<String>,
    /// SOAP Fault text when the body is a fault.
    pub fault: Option<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SendError {
    #[error("Undefined variables: {}", .0.join(", "))]
    MissingVariables(Vec<String>),
    #[error("Invalid endpoint URL: \"{0}\"")]
    InvalidUrl(String),
    #[error("{0}")]
    Http(String),
    #[error("Client certificate: {0}")]
    Certificate(String),
    #[error("WS-Security: {0}")]
    WsSecurity(String),
    #[error("Request cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shapes_match_the_frontend() {
        assert_eq!(serde_json::to_string(&Auth::None).unwrap(), r#"{"type":"none"}"#);
        let basic = Auth::Basic { username: "u".into(), password: "p".into() };
        assert_eq!(serde_json::to_string(&basic).unwrap(), r#"{"type":"basic","username":"u","password":"p"}"#);
        let ntlm = Auth::Ntlm { username: "u".into(), password: "p".into(), domain: "d".into() };
        assert_eq!(serde_json::to_string(&ntlm).unwrap(), r#"{"type":"ntlm","username":"u","password":"p","domain":"d"}"#);
        let r = HttpResponse {
            status: 200,
            status_text: "OK".into(),
            time_ms: 1,
            size: 2,
            headers: vec![],
            body: "<a/>".into(),
            pretty: None,
            fault: None,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!((json["statusText"].as_str(), json["timeMs"].as_u64()), (Some("OK"), Some(1)));
    }
}
