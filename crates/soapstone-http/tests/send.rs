use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use soapstone_http::{Auth, Header, PasswordType, RequestOptions, SendError, SoapCall, SoapVersion, WsSecurity, client, send};

/// Raw HTTP/1.1 response with Content-Length and `Connection: close`.
fn http(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Binds 127.0.0.1:0, answers one connection with `response` and sends the raw request (headers + body,
/// read up to Content-Length) through the channel. Returns ("http://127.0.0.1:<port>", receiver).
fn serve_once(response: Vec<u8>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..end]).into_owned();
                let len: usize = header(&head, "content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                if raw.len() >= end + 4 + len {
                    break;
                }
            }
        }
        stream.write_all(&response).unwrap();
        let _ = tx.send(String::from_utf8_lossy(&raw).into_owned());
    });
    (format!("http://127.0.0.1:{port}"), rx)
}

/// Value of header `name` (case-insensitive) in a raw request.
fn header(raw: &str, name: &str) -> Option<String> {
    raw.split("\r\n\r\n").next()?.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim_start().to_string())
    })
}

/// Body "<x/>", no headers, Auth::None.
fn call(endpoint: &str, version: SoapVersion, action: Option<&str>) -> SoapCall {
    SoapCall {
        endpoint: endpoint.into(),
        soap_version: version,
        soap_action: action.map(String::from),
        headers: vec![],
        auth: Auth::None,
        body: "<x/>".into(),
        ws_security: WsSecurity::default(),
        options: RequestOptions::default(),
    }
}

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn received(rx: mpsc::Receiver<String>) -> String {
    rx.recv_timeout(Duration::from_secs(5)).unwrap()
}

#[tokio::test]
async fn soap11_request_has_soapui_headers_and_rendered_body() {
    let (url, rx) = serve_once(http("200 OK", &[("Content-Type", "text/xml")], b"<r><v>1</v></r>"));
    let mut c = call(&format!("{url}/svc"), SoapVersion::V11, Some("urn:a/Op"));
    c.body = "<x>{{v}}</x>".into();
    let resp = send(&client(), &c, &vars(&[("v", "1")])).await.unwrap();
    let raw = received(rx);
    assert!(raw.starts_with("POST /svc HTTP/1.1\r\n"), "{raw}");
    assert_eq!(header(&raw, "content-type").as_deref(), Some("text/xml;charset=UTF-8"));
    assert_eq!(header(&raw, "soapaction").as_deref(), Some("\"urn:a/Op\""));
    assert!(raw.ends_with("\r\n\r\n<x>1</x>"), "{raw}");
    assert_eq!((resp.status, resp.status_text.as_str(), resp.size), (200, "OK", 15));
    assert_eq!(resp.body, "<r><v>1</v></r>");
    assert_eq!(resp.pretty.as_deref(), Some("<r>\n   <v>1</v>\n</r>"));
    assert_eq!(resp.fault, None);
    assert!(resp.headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value == "text/xml"));
}

#[tokio::test]
async fn soap12_puts_the_action_in_the_content_type() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    send(&client(), &call(&url, SoapVersion::V12, Some("urn:a/Op")), &vars(&[])).await.unwrap();
    let raw = received(rx);
    assert_eq!(header(&raw, "content-type").as_deref(), Some("application/soap+xml;charset=UTF-8;action=\"urn:a/Op\""));
    assert_eq!(header(&raw, "soapaction"), None);
}

#[tokio::test]
async fn missing_soap_action_sends_empty_soapaction_or_no_action_parameter() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    send(&client(), &call(&url, SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!(header(&received(rx), "soapaction").as_deref(), Some("\"\""));
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    send(&client(), &call(&url, SoapVersion::V12, None), &vars(&[])).await.unwrap();
    assert_eq!(header(&received(rx), "content-type").as_deref(), Some("application/soap+xml;charset=UTF-8"));
}

#[tokio::test]
async fn basic_auth_is_preemptive_and_bearer_uses_the_token() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    let mut c = call(&url, SoapVersion::V11, None);
    c.auth = Auth::Basic { username: "{{user}}".into(), password: "p".into() };
    send(&client(), &c, &vars(&[("user", "u")])).await.unwrap();
    assert_eq!(header(&received(rx), "authorization").as_deref(), Some("Basic dTpw"));
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    let mut c = call(&url, SoapVersion::V11, None);
    c.auth = Auth::Bearer { token: "{{t}}".into() };
    send(&client(), &c, &vars(&[("t", "abc")])).await.unwrap();
    assert_eq!(header(&received(rx), "authorization").as_deref(), Some("Bearer abc"));
}

#[tokio::test]
async fn user_headers_replace_automatic_ones_and_are_rendered() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    let mut c = call(&url, SoapVersion::V11, Some("a"));
    c.headers = vec![
        Header { name: "Content-Type".into(), value: "text/xml".into() },
        Header { name: "X-Env".into(), value: "{{env}}".into() },
        Header { name: "".into(), value: "ignored".into() },
    ];
    send(&client(), &c, &vars(&[("env", "dev")])).await.unwrap();
    let raw = received(rx);
    assert_eq!(raw.to_lowercase().matches("content-type:").count(), 1, "{raw}");
    assert_eq!(header(&raw, "content-type").as_deref(), Some("text/xml"));
    assert_eq!(header(&raw, "x-env").as_deref(), Some("dev"));
}

#[tokio::test]
async fn missing_variables_block_the_request() {
    let mut c = call("{{baseUrl}}/svc", SoapVersion::V11, None);
    c.body = "<x>{{a}}</x>".into();
    c.headers = vec![Header { name: "X-T".into(), value: "{{token}}".into() }];
    c.auth = Auth::Basic { username: "u".into(), password: "{{pass}}".into() };
    let expected = ["baseUrl", "a", "token", "pass"].map(String::from).to_vec();
    assert_eq!(send(&client(), &c, &vars(&[])).await, Err(SendError::MissingVariables(expected)));
}

#[tokio::test]
async fn invalid_endpoint_is_a_readable_error() {
    for endpoint in ["", "localhost:8080/svc", "ftp://example.com/x"] {
        let r = send(&client(), &call(endpoint, SoapVersion::V11, None), &vars(&[])).await;
        assert!(matches!(r, Err(SendError::InvalidUrl(_))), "{endpoint}: {r:?}");
    }
}

#[tokio::test]
async fn connection_refused_is_a_readable_error() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    match send(&client(), &call(&format!("http://127.0.0.1:{port}/"), SoapVersion::V11, None), &vars(&[])).await {
        // "actively refused" is Windows' wording
        Err(SendError::Http(msg)) => {
            let m = msg.to_lowercase();
            assert!(m.contains("connection refused") || m.contains("actively refused"), "{msg}")
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}

#[tokio::test]
async fn soap_fault_with_latin1_body_is_decoded_and_detected() {
    let body = b"<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><faultcode>s:Server</faultcode><faultstring>Servi\xE7o indispon\xEDvel</faultstring></s:Fault></s:Body></s:Envelope>";
    let (url, _rx) = serve_once(http("500 Internal Server Error", &[("Content-Type", "text/xml; charset=ISO-8859-1")], body));
    let resp = send(&client(), &call(&url, SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!(resp.status, 500);
    assert_eq!(resp.fault.as_deref(), Some("Serviço indisponível"));
    assert!(resp.body.contains("Serviço"));
}

#[tokio::test]
async fn html_error_page_is_kept_raw() {
    let page = "<html><body><p>Bad gateway</body></html>";
    let (url, _rx) = serve_once(http("502 Bad Gateway", &[("Content-Type", "text/html")], page.as_bytes()));
    let resp = send(&client(), &call(&url, SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!((resp.status, resp.body.as_str(), resp.pretty, resp.fault), (502, page, None, None));
}

#[tokio::test]
async fn redirects_are_shown_not_followed() {
    // Following a 301/302 would resend the SOAP POST as a GET and show the target page as the answer.
    let (url, rx) = serve_once(http("302 Found", &[("Location", "/other")], b""));
    let resp = send(&client(), &call(&format!("{url}/svc"), SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!(resp.status, 302);
    assert!(resp.headers.iter().any(|h| h.name == "location" && h.value == "/other"), "{:?}", resp.headers);
    assert!(received(rx).starts_with("POST /svc "));
}

/// Text between the first `open` (an element start, attributes allowed) and `close`.
fn element_text<'a>(raw: &'a str, open: &str, close: &str) -> &'a str {
    let after = &raw[raw.find(open).unwrap() + open.len()..];
    let after = &after[after.find('>').unwrap() + 1..];
    &after[..after.find(close).unwrap()]
}

#[tokio::test]
async fn ws_security_digest_is_sent_and_verifies() {
    use base64::Engine;
    use sha1::{Digest, Sha1};
    let (url, rx) = serve_once(http("200 OK", &[], b"<ok/>"));
    let mut c = call(&url, SoapVersion::V11, None);
    c.body = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Header/><s:Body><x/></s:Body></s:Envelope>"#.into();
    c.ws_security = WsSecurity {
        username_token: true,
        username: "{{u}}".into(),
        password: "{{p}}".into(),
        password_type: PasswordType::Digest,
        timestamp: true,
        ttl_seconds: 60,
        ..Default::default()
    };
    send(&client(), &c, &vars(&[("u", "alice"), ("p", "s3cret")])).await.unwrap();
    let raw = received(rx);
    assert!(raw.contains("<wsse:Username>alice</wsse:Username>"), "{raw}");
    let b64 = base64::engine::general_purpose::STANDARD;
    let nonce = b64.decode(element_text(&raw, "<wsse:Nonce", "</wsse:Nonce>")).unwrap();
    let token = element_text(&raw, "<wsse:UsernameToken", "</wsse:UsernameToken>");
    let created = element_text(token, "<wsu:Created", "</wsu:Created>");
    let mut sha = Sha1::new();
    sha.update(&nonce);
    sha.update(created.as_bytes());
    sha.update(b"s3cret");
    assert_eq!(element_text(&raw, "<wsse:Password", "</wsse:Password>"), b64.encode(sha.finalize()));
    let time = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap();
    let stamp = element_text(&raw, "<wsu:Timestamp", "</wsu:Timestamp>");
    let (from, to) = (element_text(stamp, "<wsu:Created", "</wsu:Created>"), element_text(stamp, "<wsu:Expires", "</wsu:Expires>"));
    assert_eq!((time(to) - time(from)).num_seconds(), 60);
}

#[tokio::test]
async fn disabled_ws_security_fields_never_block_a_send() {
    let (url, rx) = serve_once(http("200 OK", &[], b"<ok/>"));
    let mut c = call(&url, SoapVersion::V11, None);
    c.ws_security.username = "{{nope}}".into();
    send(&client(), &c, &vars(&[])).await.unwrap();
    assert!(received(rx).ends_with("\r\n\r\n<x/>"));
}

#[tokio::test]
async fn per_request_timeout_applies() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_secs(10)); // never answers
    });
    let mut c = call(&format!("http://127.0.0.1:{port}/"), SoapVersion::V11, None);
    c.options.timeout_seconds = 1;
    let start = std::time::Instant::now();
    match send(&client(), &c, &vars(&[])).await {
        Err(SendError::Http(m)) => assert!(m.to_lowercase().contains("timed out"), "{m}"),
        other => panic!("expected a timeout, got {other:?}"),
    }
    assert!(start.elapsed() < Duration::from_secs(5));
}
