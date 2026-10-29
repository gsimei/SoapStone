// Shared test helpers. Each test crate compiles this file separately and uses only part of it.
#![allow(dead_code)]

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

/// Tiny HTTP server on a random local port: answers each request whose target (path + query)
/// matches a route with 200 and the body, anything else with 404. Returns the port.
pub fn serve(routes: Vec<(&'static str, String)>) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            // Drain the headers so the client sees a clean close.
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let target = request_line.split_whitespace().nth(1).unwrap_or("");
            let response = match routes.iter().find(|(path, _)| *path == target) {
                Some((_, body)) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()),
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            };
            let _ = (&stream).write_all(response.as_bytes());
        }
    });
    port
}

/// `p:Local[dims]` → `{namespace of p}Local[dims]`, resolving `p` on `node`.
fn expand_qname(node: Node, value: &str) -> String {
    let (prefix, rest) = value.split_once(':').map_or((None, value), |(p, r)| (Some(p), r));
    format!("{{{}}}{rest}", node.lookup_namespace_uri(prefix).unwrap_or(""))
}
