//! WS-Security UsernameToken and Timestamp, inserted into the SOAP Header.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use quick_xml::escape::escape;
use sha1::{Digest, Sha1};

use crate::{PasswordType, WsSecurity};

const SOAP11: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP12: &str = "http://www.w3.org/2003/05/soap-envelope";
const WSSE: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd";
const WSU: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd";
const TOKEN_PROFILE: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0";
const BASE64_BINARY: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary";

/// `envelope` with a `<wsse:Security>` block as the first child of its SOAP Header, which is created when
/// missing. Call only when `wss.enabled()`.
pub(crate) fn apply(envelope: &str, wss: &WsSecurity, now: DateTime<Utc>, nonce: [u8; 16]) -> Result<String, String> {
    let doc = roxmltree::Document::parse(envelope).map_err(|e| format!("the body is not well-formed XML ({e})"))?;
    let root = doc.root_element();
    let ns = root
        .tag_name()
        .namespace()
        .filter(|ns| root.tag_name().name() == "Envelope" && [SOAP11, SOAP12].contains(ns))
        .ok_or("the body is not a SOAP Envelope")?;
    let child = |local: &str| root.children().find(|n| n.is_element() && n.has_tag_name((ns, local)));
    let body = child("Body").ok_or("the SOAP Envelope has no Body")?;
    let security = security(wss, ns, prefix(&envelope[root.range().start..]), now, nonce);

    let Some(header) = child("Header") else {
        let at = body.range().start;
        let p = match prefix(&envelope[at..]) {
            "" => String::new(),
            p => format!("{p}:"),
        };
        return Ok(format!("{}<{p}Header>{security}</{p}Header>{}", &envelope[..at], &envelope[at..]));
    };
    let start = header.range().start;
    let end = start + start_tag_len(&envelope[start..]);
    Ok(if envelope[..end].ends_with("/>") {
        // <p:Header/> becomes <p:Header>…</p:Header>
        let name = qname(&envelope[start..]);
        format!("{}>{security}</{name}>{}", &envelope[..end - 2], &envelope[end..])
    } else {
        format!("{}{security}{}", &envelope[..end], &envelope[end..])
    })
}

/// The `<wsse:Security>` element: Timestamp first, then UsernameToken.
fn security(wss: &WsSecurity, ns: &str, prefix: &str, now: DateTime<Utc>, nonce: [u8; 16]) -> String {
    let stamp = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let created = stamp(now);
    let must = if ns == SOAP12 { "true" } else { "1" };
    let must_understand = match prefix {
        "" => format!(r#"xmlns:soapenv="{ns}" soapenv:mustUnderstand="{must}""#),
        p => format!(r#"{p}:mustUnderstand="{must}""#),
    };
    let mut s = format!(r#"<wsse:Security {must_understand} xmlns:wsse="{WSSE}" xmlns:wsu="{WSU}">"#);
    if wss.timestamp {
        let expires = stamp(now + TimeDelta::seconds(wss.ttl_seconds.into()));
        s += &format!("<wsu:Timestamp><wsu:Created>{created}</wsu:Created><wsu:Expires>{expires}</wsu:Expires></wsu:Timestamp>");
    }
    if wss.username_token {
        let (kind, password) = match wss.password_type {
            PasswordType::Text => ("PasswordText", escape(wss.password.as_str()).into_owned()),
            PasswordType::Digest => {
                let mut sha = Sha1::new();
                if wss.nonce {
                    sha.update(nonce);
                }
                if wss.created {
                    sha.update(created.as_bytes());
                }
                sha.update(wss.password.as_bytes());
                ("PasswordDigest", STANDARD.encode(sha.finalize()))
            }
        };
        s += &format!("<wsse:UsernameToken><wsse:Username>{}</wsse:Username>", escape(wss.username.as_str()));
        s += &format!(r#"<wsse:Password Type="{TOKEN_PROFILE}#{kind}">{password}</wsse:Password>"#);
        if wss.nonce {
            s += &format!(r#"<wsse:Nonce EncodingType="{BASE64_BINARY}">{}</wsse:Nonce>"#, STANDARD.encode(nonce));
        }
        if wss.created {
            s += &format!("<wsu:Created>{created}</wsu:Created>");
        }
        s += "</wsse:UsernameToken>";
    }
    s + "</wsse:Security>"
}

/// The qualified name of the start tag at the beginning of `tag` ("<p:Name ...").
fn qname(tag: &str) -> &str {
    let name = &tag[1..];
    &name[..name.find(|c: char| c.is_whitespace() || c == '/' || c == '>').unwrap_or(name.len())]
}

/// The prefix of that name, "" when it has none.
fn prefix(tag: &str) -> &str {
    qname(tag).split_once(':').map_or("", |(p, _)| p)
}

/// Bytes up to and including the `>` that ends the start tag at the beginning of `tag`; quoted attribute
/// values may contain `>`.
fn start_tag_len(tag: &str) -> usize {
    let mut quote = None;
    for (i, b) in tag.bytes().enumerate() {
        match (quote, b) {
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), b) if b == q => quote = None,
            (None, b'>') => return i + 1,
            _ => {}
        }
    }
    tag.len()
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::{PasswordType, WsSecurity};
    use chrono::{TimeZone, Utc};

    const NONCE: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    const E11: &str = r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:tem="http://tempuri.org/"><soapenv:Header/><soapenv:Body><tem:Add/></soapenv:Body></soapenv:Envelope>"#;

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
    }

    fn full() -> WsSecurity {
        WsSecurity {
            username_token: true,
            username: "alice".into(),
            password: "secret".into(),
            password_type: PasswordType::Digest,
            nonce: true,
            created: true,
            timestamp: true,
            ttl_seconds: 300,
        }
    }

    fn token_only(password_type: PasswordType, nonce: bool, created: bool) -> WsSecurity {
        WsSecurity { password_type, nonce, created, timestamp: false, ..full() }
    }

    #[test]
    fn digest_token_and_timestamp_match_the_profile() {
        let out = apply(E11, &full(), now(), NONCE).unwrap();
        assert_eq!(
            out,
            concat!(
                r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:tem="http://tempuri.org/"><soapenv:Header>"#,
                r#"<wsse:Security soapenv:mustUnderstand="1" xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">"#,
                r#"<wsu:Timestamp><wsu:Created>2026-10-29T12:00:00.000Z</wsu:Created><wsu:Expires>2026-10-29T12:05:00.000Z</wsu:Expires></wsu:Timestamp>"#,
                r#"<wsse:UsernameToken><wsse:Username>alice</wsse:Username>"#,
                r#"<wsse:Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest">RnwQC+GBXPoduhcjUk+3fhZrHfA=</wsse:Password>"#,
                r#"<wsse:Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary">AAECAwQFBgcICQoLDA0ODw==</wsse:Nonce>"#,
                r#"<wsu:Created>2026-10-29T12:00:00.000Z</wsu:Created></wsse:UsernameToken></wsse:Security>"#,
                r#"</soapenv:Header><soapenv:Body><tem:Add/></soapenv:Body></soapenv:Envelope>"#
            )
        );
    }

    #[test]
    fn digest_leaves_out_a_missing_nonce() {
        let out = apply(E11, &token_only(PasswordType::Digest, false, true), now(), NONCE).unwrap();
        assert!(out.contains(">qjlu86mTo+hE8l3DVgMLssW5X6M=</wsse:Password>"), "{out}");
        assert!(!out.contains("<wsse:Nonce"), "{out}");
    }

    #[test]
    fn text_password_without_nonce_or_created() {
        let out = apply(E11, &token_only(PasswordType::Text, false, false), now(), NONCE).unwrap();
        assert!(out.contains("#PasswordText\">secret</wsse:Password></wsse:UsernameToken>"), "{out}");
        assert!(!out.contains("<wsu:"), "{out}");
    }

    #[test]
    fn missing_header_is_created_before_the_body() {
        let out = apply(&E11.replace("<soapenv:Header/>", ""), &full(), now(), NONCE).unwrap();
        assert!(out.contains("<soapenv:Header><wsse:Security"), "{out}");
        assert!(out.contains("</wsse:Security></soapenv:Header><soapenv:Body>"), "{out}");
    }

    #[test]
    fn existing_header_children_follow_the_security_block() {
        let e = E11.replace("<soapenv:Header/>", r#"<soapenv:Header><a:Trace xmlns:a="urn:a">1</a:Trace></soapenv:Header>"#);
        let out = apply(&e, &full(), now(), NONCE).unwrap();
        assert!(out.contains(r#"<soapenv:Header><wsse:Security"#), "{out}");
        assert!(out.contains(r#"</wsse:Security><a:Trace xmlns:a="urn:a">1</a:Trace></soapenv:Header>"#), "{out}");
    }

    #[test]
    fn must_understand_follows_version_and_prefix() {
        let e12 = r#"<env:Envelope xmlns:env="http://www.w3.org/2003/05/soap-envelope"><env:Header/><env:Body/></env:Envelope>"#;
        let out = apply(e12, &full(), now(), NONCE).unwrap();
        assert!(out.contains(r#"<env:Header><wsse:Security env:mustUnderstand="true" "#), "{out}");

        let bare = r#"<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/"><Body/></Envelope>"#;
        let out = apply(bare, &full(), now(), NONCE).unwrap();
        assert!(
            out.contains(r#"<Header><wsse:Security xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" soapenv:mustUnderstand="1" "#),
            "{out}"
        );
        assert!(out.contains("</wsse:Security></Header><Body/>"), "{out}");
    }

    #[test]
    fn username_and_password_are_escaped() {
        let w = WsSecurity { username: "a&b".into(), password: "<p>".into(), ..token_only(PasswordType::Text, false, false) };
        let out = apply(E11, &w, now(), NONCE).unwrap();
        assert!(out.contains("<wsse:Username>a&amp;b</wsse:Username>"), "{out}");
        assert!(out.contains(">&lt;p&gt;</wsse:Password>"), "{out}");
    }

    #[test]
    fn body_that_is_not_an_envelope_is_rejected() {
        assert_eq!(apply("<x/>", &full(), now(), NONCE), Err("the body is not a SOAP Envelope".to_string()));
        assert!(apply("oops", &full(), now(), NONCE).unwrap_err().starts_with("the body is not well-formed XML"));
        let no_body = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Header/></s:Envelope>"#;
        assert_eq!(apply(no_body, &full(), now(), NONCE), Err("the SOAP Envelope has no Body".to_string()));
    }
}
