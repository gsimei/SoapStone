use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use soapstone_http::{Auth, RequestOptions, SendError, SoapCall, SoapVersion, WsSecurity, client, send};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/tls/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// One HTTPS connection on 127.0.0.1 with the test server certificate (signed by the test CA, which the client
/// does not trust). Answers 200 `<ok/>` and sends the raw request through the channel; nothing when the
/// handshake fails.
fn serve_tls(require_client_cert: bool) -> (String, mpsc::Receiver<String>) {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let certs: Vec<_> = CertificateDer::pem_file_iter(fixture("server.pem")).unwrap().map(Result::unwrap).collect();
    let key = PrivateKeyDer::from_pem_file(fixture("server.key")).unwrap();
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().unwrap();
    let builder = if require_client_cert {
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_file_iter(fixture("ca.pem")).unwrap() {
            roots.add(cert.unwrap()).unwrap();
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(roots.into(), provider).build().unwrap();
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    let config = Arc::new(builder.with_single_cert(certs, key).unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), tcp);
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match tls.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
            }
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let len: usize = head.split("content-length:").nth(1).and_then(|v| v.lines().next()?.trim().parse().ok()).unwrap_or(0);
                if raw.len() >= end + 4 + len {
                    break;
                }
            }
        }
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n<ok/>");
        tls.conn.send_close_notify();
        let _ = tls.flush();
        let _ = tx.send(String::from_utf8_lossy(&raw).into_owned());
    });
    (format!("https://127.0.0.1:{port}/svc"), rx)
}

fn call(endpoint: &str, options: RequestOptions) -> SoapCall {
    SoapCall {
        endpoint: endpoint.into(),
        soap_version: SoapVersion::V11,
        soap_action: None,
        headers: vec![],
        auth: Auth::None,
        body: "<x/>".into(),
        ws_security: WsSecurity::default(),
        options,
    }
}

fn vars() -> HashMap<String, String> {
    HashMap::from([("pw".to_string(), "test".to_string())])
}

fn with_cert(file: &str) -> RequestOptions {
    RequestOptions { insecure: true, cert_path: fixture(file), cert_password: "{{pw}}".into(), ..Default::default() }
}

#[tokio::test]
async fn untrusted_server_certificate_fails_unless_ignored() {
    let (url, _rx) = serve_tls(false);
    match send(&client(), &call(&url, RequestOptions::default()), &vars()).await {
        Err(SendError::Http(m)) => assert!(m.to_lowercase().contains("certificate"), "{m}"),
        other => panic!("expected a certificate error, got {other:?}"),
    }
    let (url, rx) = serve_tls(false);
    let insecure = RequestOptions { insecure: true, ..Default::default() };
    assert_eq!(send(&client(), &call(&url, insecure), &vars()).await.unwrap().status, 200);
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().starts_with("POST /svc "));
}

#[tokio::test]
async fn client_certificate_from_p12_is_presented() {
    for file in ["client.p12", "client-legacy.p12"] {
        let (url, _rx) = serve_tls(true);
        let r = send(&client(), &call(&url, with_cert(file)), &vars()).await;
        assert_eq!(r.map(|r| r.status), Ok(200), "{file}");
    }
}

#[tokio::test]
async fn server_requiring_a_certificate_rejects_a_send_without_one() {
    let (url, _rx) = serve_tls(true);
    let insecure = RequestOptions { insecure: true, ..Default::default() };
    let r = send(&client(), &call(&url, insecure), &vars()).await;
    assert!(matches!(r, Err(SendError::Http(_))), "{r:?}");
}

#[tokio::test]
async fn wrong_password_or_missing_file_is_a_certificate_error() {
    let wrong = RequestOptions { cert_password: "nope".into(), ..with_cert("client.p12") };
    let missing = RequestOptions { cert_path: fixture("absent.p12"), ..with_cert("client.p12") };
    for options in [wrong, missing] {
        let (url, rx) = serve_tls(true);
        let path = options.cert_path.clone();
        match send(&client(), &call(&url, options), &vars()).await {
            Err(SendError::Certificate(m)) => assert!(m.contains(&path), "{m}"),
            other => panic!("expected a certificate error, got {other:?}"),
        }
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "nothing must be sent");
    }
}
