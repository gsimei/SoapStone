//! Sending one SOAP request.

use std::collections::HashMap;
use std::error::Error as _;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue};
use reqwest::{Client, ClientBuilder, Identity, Request, Response, StatusCode, Url};

use crate::{
    Auth, Header, HttpResponse, SendError, SoapCall, SoapVersion, WsSecurity, decode_body, fault_string, ntlm, pretty_xml, render,
    render_xml, validate_xml, wss,
};

/// Shared client: 60 s timeout, `User-Agent: Soapstone/<version>`. Redirects are returned, not followed:
/// following one would resend the SOAP POST as a GET.
pub fn client() -> Client {
    base().build().expect("HTTP client with static settings")
}

/// What every client shares; requests with their own TLS settings get a client built from this.
fn base() -> ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Soapstone/", env!("CARGO_PKG_VERSION")))
}

/// Renders `{{variables}}` in the endpoint, body, headers and auth (nothing is sent when one is undefined),
/// then POSTs the body with SoapUI's automatic headers. User headers replace automatic ones of the same name.
pub fn prepare(call: &SoapCall, vars: &HashMap<String, String>) -> Result<SoapCall, SendError> {
    let mut missing: Vec<String> = Vec::new();
    let mut r = |template: &str, xml: bool| {
        (if xml { render_xml(template, vars) } else { render(template, vars) }).unwrap_or_else(|names| {
            for name in names {
                if !missing.contains(&name) {
                    missing.push(name);
                }
            }
            String::new()
        })
    };
    let endpoint = r(&call.endpoint, false);
    let body = r(&call.body, true);
    let headers: Vec<Header> =
        call.headers.iter().map(|h| Header { name: r(&h.name, false).trim().to_string(), value: r(&h.value, false) }).collect();
    let auth = match &call.auth {
        Auth::None => Auth::None,
        Auth::Basic { username, password } => Auth::Basic { username: r(username, false), password: r(password, false) },
        Auth::Bearer { token } => Auth::Bearer { token: r(token, false) },
        Auth::Ntlm { username, password, domain } => {
            Auth::Ntlm { username: r(username, false), password: r(password, false), domain: r(domain, false) }
        }
    };
    let wss = &call.ws_security;
    let wss = wss.enabled().then(|| WsSecurity {
        username: if wss.username_token { r(&wss.username, false) } else { String::new() },
        password: if wss.username_token { r(&wss.password, false) } else { String::new() },
        ..wss.clone()
    });
    let options = &call.options;
    let cert = (!options.cert_path.trim().is_empty()).then(|| (r(options.cert_path.trim(), false), r(&options.cert_password, false)));
    if !missing.is_empty() {
        return Err(SendError::MissingVariables(missing));
    }
    for header in headers.iter().filter(|h| !h.name.is_empty()) {
        let invalid = || SendError::Http(format!("Invalid header \"{}\"", header.name));
        HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| invalid())?;
        HeaderValue::from_str(&header.value).map_err(|_| invalid())?;
    }
    let headers = headers.into_iter().filter(|h| !h.name.is_empty()).collect();

    Url::parse(endpoint.trim())
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| SendError::InvalidUrl(endpoint.clone()))?;
    validate_xml(&body).map_err(SendError::InvalidXml)?;

    let body = match wss {
        Some(wss) => {
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).map_err(|e| SendError::WsSecurity(format!("no random nonce ({e})")))?;
            wss::apply(&body, &wss, chrono::Utc::now(), nonce).map_err(SendError::WsSecurity)?
        }
        None => body,
    };

    let mut options = options.clone();
    if let Some((path, password)) = cert {
        options.cert_path = path;
        options.cert_password = password;
    }
    Ok(SoapCall { endpoint: endpoint.trim().into(), body, headers, auth, options, ws_security: WsSecurity::default(), ..call.clone() })
}

pub async fn send(client: &Client, call: &SoapCall, vars: &HashMap<String, String>) -> Result<HttpResponse, SendError> {
    let call = prepare(call, vars)?;
    let options = &call.options;
    let auth = &call.auth;
    let cert = !options.cert_path.is_empty();

    let own;
    let ntlm = matches!(auth, Auth::Ntlm { .. });
    let client = if options.insecure || cert || ntlm {
        let mut builder = base().danger_accept_invalid_certs(options.insecure);
        if ntlm {
            // The NTLM handshake authenticates one TCP connection: keep exactly one, HTTP/1.1.
            builder = builder.http1_only().pool_max_idle_per_host(1);
        }
        if cert {
            builder = builder.identity(identity(&options.cert_path, &options.cert_password)?);
        }
        own = builder.build().map_err(http_error)?;
        &own
    } else {
        client
    };

    let action = call.soap_action.as_deref().filter(|a| !a.is_empty());
    let timeout = Duration::from_secs(options.timeout_seconds.max(1).into());
    let mut builder = client.post(&call.endpoint).body(call.body).timeout(timeout);
    builder = match call.soap_version {
        SoapVersion::V11 => {
            builder.header(CONTENT_TYPE, "text/xml;charset=UTF-8").header("SOAPAction", format!("\"{}\"", action.unwrap_or("")))
        }
        SoapVersion::V12 => builder.header(
            CONTENT_TYPE,
            match action {
                Some(a) => format!("application/soap+xml;charset=UTF-8;action=\"{a}\""),
                None => "application/soap+xml;charset=UTF-8".into(),
            },
        ),
    };
    builder = match &auth {
        Auth::None | Auth::Ntlm { .. } => builder,
        Auth::Basic { username, password } => builder.basic_auth(username, Some(password)),
        Auth::Bearer { token } => builder.bearer_auth(token),
    };
    let mut request = builder.build().map_err(http_error)?;

    let mut user = Vec::new();
    for header in &call.headers {
        let name = header.name.trim();
        if name.is_empty() {
            continue;
        }
        let invalid = || SendError::Http(format!("Invalid header \"{name}\""));
        user.push((
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?,
            HeaderValue::from_str(&header.value).map_err(|_| invalid())?,
        ));
    }
    for (name, _) in &user {
        request.headers_mut().remove(name);
    }
    for (name, value) in user {
        request.headers_mut().append(name, value);
    }

    let start = Instant::now();
    let mut response = match auth {
        Auth::Ntlm { username, password, domain } => ntlm_exchange(client, request, username, password, domain).await?,
        _ => client.execute(request).await.map_err(http_error)?,
    };
    let status = response.status();
    let headers: Vec<Header> = response
        .headers()
        .iter()
        .map(|(n, v)| Header { name: n.as_str().to_string(), value: String::from_utf8_lossy(v.as_bytes()).into_owned() })
        .collect();
    let content_type = response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(String::from);
    const MAX_RESPONSE: usize = 32 * 1024 * 1024;
    if response.content_length().is_some_and(|size| size > MAX_RESPONSE as u64) {
        return Err(SendError::Http("Response exceeds the 32 MB limit".into()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(http_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err(SendError::Http("Response exceeds the 32 MB limit".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    let time_ms = start.elapsed().as_millis() as u64;

    let body = decode_body(&bytes, content_type.as_deref());
    let pretty = (body.len() <= 256 * 1024).then(|| pretty_xml(&body)).flatten();
    let fault = fault_string(&body);
    Ok(HttpResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        time_ms,
        size: bytes.len() as u64,
        headers,
        body,
        pretty,
        fault,
    })
}

/// Type 1 → 401 with a Type 2 challenge → Type 3, on one connection. A server that does not answer the
/// first leg with an NTLM challenge gets its response returned as is.
async fn ntlm_exchange(client: &Client, mut request: Request, username: &str, password: &str, domain: &str) -> Result<Response, SendError> {
    let token = |t: String| HeaderValue::from_str(&format!("NTLM {t}")).expect("base64 is a valid header value");
    let mut first = request.try_clone().expect("a text body can be cloned");
    first.headers_mut().insert(AUTHORIZATION, token(ntlm::negotiate()));
    let response = client.execute(first).await.map_err(http_error)?;
    let challenge = match ntlm::challenge(response.headers()) {
        Some(challenge) if response.status() == StatusCode::UNAUTHORIZED => challenge,
        _ => return Ok(response),
    };
    response.bytes().await.map_err(http_error)?; // read to the end so the connection is reused
    let answer = ntlm::authenticate(&challenge, username, password, domain)
        .ok_or_else(|| SendError::Http("Invalid NTLM challenge from the server".into()))?;
    request.headers_mut().insert(AUTHORIZATION, token(answer));
    client.execute(request).await.map_err(http_error)
}

/// A `.p12`/`.pfx` file as a rustls identity: its first private key with that key's certificate chain.
fn identity(path: &str, password: &str) -> Result<Identity, SendError> {
    let error = |m: String| SendError::Certificate(m);
    let bytes = std::fs::read(path).map_err(|e| error(format!("cannot read {path}: {e}")))?;
    let store = p12_keystore::KeyStore::from_pkcs12(&bytes, password, p12_keystore::Pkcs12ImportPolicy::Relaxed)
        .map_err(|e| error(format!("cannot open {path}: wrong password or not a PKCS#12 file ({e})")))?;
    let (_, chain) = store
        .private_key_chain()
        .filter(|(_, chain)| !chain.certs().is_empty())
        .ok_or_else(|| error(format!("{path} has no private key with a certificate")))?;
    let mut pem = pem_block("PRIVATE KEY", chain.key().as_der());
    for cert in chain.certs() {
        pem += &pem_block("CERTIFICATE", cert.as_der());
    }
    Identity::from_pem(pem.as_bytes()).map_err(|e| error(format!("cannot use {path}: {e}")))
}

fn pem_block(label: &str, der: &[u8]) -> String {
    let b64 = STANDARD.encode(der);
    let lines: Vec<&str> = b64.as_bytes().chunks(64).map(|l| std::str::from_utf8(l).expect("base64 is ASCII")).collect();
    format!("-----BEGIN {label}-----\n{}\n-----END {label}-----\n", lines.join("\n"))
}

/// reqwest's message plus its source chain ("error sending request: ... : Connection refused").
fn http_error(e: reqwest::Error) -> SendError {
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        message.push_str(": ");
        message.push_str(&s.to_string());
        source = s.source();
    }
    SendError::Http(message)
}
