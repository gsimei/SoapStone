// Shared test helpers. Each test crate compiles this file separately and uses only part of it.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use roxmltree::{Document, Node, NodeType};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const SOAP_ENC: &str = "http://schemas.xmlsoap.org/soap/encoding/";

/// Compares two XML documents ignoring prefixes, where namespaces are declared,
/// whitespace between elements and the random number in `cid:` values.
pub fn assert_xml_equiv(expected: &str, actual: &str) {
    assert_eq!(canonical(expected), canonical(actual), "XML documents differ");
}

fn canonical(xml: &str) -> String {
    let doc = Document::parse(xml).unwrap_or_else(|e| panic!("invalid XML: {e}\n{xml}"));
    let mut out = String::new();
    render(doc.root_element(), 0, &mut out);
    out
}

fn render(node: Node, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match node.node_type() {
        NodeType::Element => {
            let mut attrs: Vec<String> = node
                .attributes()
                .map(|a| {
                    let ns = a.namespace().unwrap_or("");
                    let qname_valued = (ns == XSI && a.name() == "type") || (ns == SOAP_ENC && a.name() == "arrayType");
                    let value = if qname_valued { expand_qname(node, a.value()) } else { a.value().to_string() };
                    format!("{{{ns}}}{}={value:?}", a.name())
                })
                .collect();
            attrs.sort();
            let tag = node.tag_name();
            out.push_str(&format!("{pad}{{{}}}{} {}\n", tag.namespace().unwrap_or(""), tag.name(), attrs.join(" ")));
            for child in node.children() {
                render(child, depth + 1, out);
            }
        }
        NodeType::Text => {
            let text = node.text().unwrap_or("").trim();
            let is_cid = text.strip_prefix("cid:").is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
            if is_cid {
                out.push_str(&format!("{pad}\"cid:*\"\n"));
            } else if !text.is_empty() {
                out.push_str(&format!("{pad}{text:?}\n"));
            }
        }
        NodeType::Comment => out.push_str(&format!("{pad}<!--{}-->\n", node.text().unwrap_or("").trim())),
        _ => {}
    }
}

/// Path of a fixture kept only on the developer's machine (real customer WSDLs, git-ignored).
/// `None` when it is absent, so the calling test returns early and passes as skipped.
pub fn local_fixture(rel: &str) -> Option<String> {
    let path = format!("{}/tests/fixtures/{rel}", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&path).exists() {
        Some(path)
    } else {
        eprintln!("skipped: local-only fixture {rel} is absent");
        None
    }
}

/// Tiny HTTP server on a random local port: answers each request whose target (path + query)
/// matches a route with 200 and the body, anything else with 404. Returns the port.
pub fn serve(routes: Vec<(&'static str, String)>) -> u16 {
    serve_recording(routes).0
}

/// `serve`, also recording each request's head (request line and headers) in arrival order.
pub fn serve_recording(routes: Vec<(&'static str, String)>) -> (u16, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local port");
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let recorded = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut head = String::new();
            // Read up to the blank line so the client sees a clean close.
            while reader.read_line(&mut head).is_ok_and(|n| n > 2) {}
            let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
            recorded.lock().unwrap().push(head);
            let response = match routes.iter().find(|(path, _)| *path == target) {
                Some((_, body)) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()),
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            };
            let _ = (&stream).write_all(response.as_bytes());
        }
    });
    (port, heads)
}

/// HTTPS on 127.0.0.1 with soapstone-http's test certificate (signed by a test CA nobody trusts):
/// answers every request with 200 and `body`. Returns the port.
pub fn serve_tls(body: String) -> u16 {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::io::{Read, Write};
    let fixture = |name: &str| format!("{}/../soapstone-http/tests/fixtures/tls/{name}", env!("CARGO_MANIFEST_DIR"));
    let certs: Vec<_> = CertificateDer::pem_file_iter(fixture("server.pem")).unwrap().map(Result::unwrap).collect();
    let key = PrivateKeyDer::from_pem_file(fixture("server.key")).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap();
    let config = Arc::new(config.with_no_client_auth().with_single_cert(certs, key).unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for tcp in listener.incoming().flatten() {
            let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config.clone()).unwrap(), tcp);
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let complete = loop {
                match tls.read(&mut buf) {
                    Ok(0) | Err(_) => break false, // a client that rejected the certificate
                    Ok(n) => raw.extend_from_slice(&buf[..n]),
                }
                if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break true;
                }
            };
            if complete {
                let _ = write!(tls, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        }
    });
    port
}

/// `p:Local[dims]` → `{namespace of p}Local[dims]`, resolving `p` on `node`.
fn expand_qname(node: Node, value: &str) -> String {
    let (prefix, rest) = value.split_once(':').map_or((None, value), |(p, r)| (Some(p), r));
    format!("{{{}}}{rest}", node.lookup_namespace_uri(prefix).unwrap_or(""))
}
