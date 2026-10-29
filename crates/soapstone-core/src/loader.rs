//! Fetches a WSDL and every document it imports or includes, from disk or over HTTP.

use std::collections::{HashSet, VecDeque};
use std::fmt::Display;
use std::time::Duration;

use reqwest::blocking::Client;
use url::Url;

use crate::xsd::XS;

pub(crate) const WSDL_NS: &str = "http://schemas.xmlsoap.org/wsdl/";
const TIMEOUT: Duration = Duration::from_secs(60);

/// Imports of these namespaces are never fetched: every SOAP tool knows them already.
pub(crate) const BUILTIN_NAMESPACES: &[&str] = &[
    "http://schemas.xmlsoap.org/soap/envelope/",
    "http://www.w3.org/2003/05/soap-envelope",
    "http://schemas.xmlsoap.org/soap/encoding/",
    "http://www.w3.org/2003/05/soap-encoding",
    "http://www.w3.org/XML/1998/namespace",
    "http://www.w3.org/2001/XMLSchema",
    "http://www.w3.org/2004/11/xmlmime",
    "http://www.w3.org/2005/05/xmlmime",
    "http://www.w3.org/2004/08/xop/include",
    "http://ws-i.org/profiles/basic/1.1/xsd",
];

#[derive(Debug, Clone)]
pub struct Document {
    pub url: String,
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not fetch {url}: {reason}")]
    Fetch { url: String, reason: String },
    #[error("invalid XML in {url}: {reason}")]
    Xml { url: String, reason: String },
    #[error("{url} is not a WSDL 1.1 document")]
    NotWsdl { url: String },
}

fn fetch_err(url: &str, reason: impl Display) -> LoadError {
    LoadError::Fetch { url: url.to_string(), reason: reason.to_string() }
}

/// http(s) and file URLs are taken as they are; anything else is a path on disk.
pub fn to_url(input: &str) -> Result<Url, LoadError> {
    let lower = input.to_ascii_lowercase();
    if ["http://", "https://", "file:"].iter().any(|s| lower.starts_with(s)) {
        return Url::parse(input).map_err(|e| fetch_err(input, e));
    }
    let path = std::fs::canonicalize(input).map_err(|e| fetch_err(input, e))?;
    Url::from_file_path(&path).map_err(|()| fetch_err(input, "not a usable file path"))
}

/// How documents are downloaded over HTTP(S).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchOptions {
    /// Basic credentials, sent only to the root document's origin (scheme, host and port).
    pub username: String,
    pub password: String,
    /// Accept any server certificate.
    pub insecure: bool,
}

/// The HTTP client, built on first use, and what every request needs to know.
struct Http<'a> {
    client: Option<Client>,
    origin: url::Origin,
    options: &'a FetchOptions,
}

/// The root document first, then every imported or included document in discovery order, each once.
pub fn fetch_all(root: &Url) -> Result<Vec<Document>, LoadError> {
    fetch_all_with(root, &FetchOptions::default())
}

/// `fetch_all` with credentials and TLS settings.
pub fn fetch_all_with(root: &Url, options: &FetchOptions) -> Result<Vec<Document>, LoadError> {
    let mut http = Http { client: None, origin: root.origin(), options };
    let (root, content, refs) = load_root(&mut http, root)?;
    let mut seen = HashSet::from([root.to_string()]);
    let mut queue = VecDeque::new();
    enqueue(&root, refs, &mut seen, &mut queue)?;
    let mut docs = vec![Document { url: root.to_string(), content }];
    while let Some(url) = queue.pop_front() {
        let content = fetch(&mut http, &url)?;
        let (_, refs) = scan(url.as_str(), &content)?;
        enqueue(&url, refs, &mut seen, &mut queue)?;
        docs.push(Document { url: url.to_string(), content });
    }
    Ok(docs)
}

/// The root WSDL. A service URL without a query often answers with an HTML page or an error status
/// (CXF, PHP SoapServer); then ask again with `?wsdl`, the way people type it in SoapUI. If that fails
/// too, report the first error: it names the URL the user gave. Unreachable hosts are not retried,
/// so a timeout is never paid twice.
fn load_root(http: &mut Http, url: &Url) -> Result<(Url, String, Vec<String>), LoadError> {
    let error = match fetch_bytes(http, url)?.and_then(|bytes| wsdl_root(url, &bytes)) {
        Ok((content, refs)) => return Ok((url.clone(), content, refs)),
        Err(e) => e,
    };
    if url.query().is_some() || url.scheme() == "file" {
        return Err(error);
    }
    let mut retry = url.clone();
    retry.set_query(Some("wsdl"));
    match fetch_bytes(http, &retry) {
        Ok(Ok(bytes)) => wsdl_root(&retry, &bytes).map(|(content, refs)| (retry, content, refs)).map_err(|_| error),
        _ => Err(error),
    }
}

/// The text and references of a WSDL document; `NotWsdl` for any other XML.
fn wsdl_root(url: &Url, bytes: &[u8]) -> Result<(String, Vec<String>), LoadError> {
    let content = decode(bytes, url.as_str())?;
    match scan(url.as_str(), &content)? {
        (true, refs) => Ok((content, refs)),
        (false, _) => Err(LoadError::NotWsdl { url: url.to_string() }),
    }
}

/// Local files can be imported only by local files, and never from another machine's file share
/// (on Windows that opens an SMB connection and sends the user's credentials).
fn enqueue(base: &Url, refs: Vec<String>, seen: &mut HashSet<String>, queue: &mut VecDeque<Url>) -> Result<(), LoadError> {
    for location in refs {
        let url = base.join(&location).map_err(|e| fetch_err(&location, e))?;
        if url.scheme() == "file" && base.scheme() != "file" {
            return Err(fetch_err(url.as_str(), "a document fetched over the network cannot import a local file"));
        }
        if url.scheme() == "file" && url.host().is_some() {
            return Err(fetch_err(url.as_str(), "the file is on a network share, not on this computer"));
        }
        if seen.insert(url.to_string()) {
            queue.push_back(url);
        }
    }
    Ok(())
}

/// Parses every document, failing on the first one that is not well-formed XML.
pub(crate) fn parse_all(documents: &[Document]) -> Result<Vec<roxmltree::Document<'_>>, LoadError> {
    documents.iter().map(|d| parse_xml(&d.url, &d.content)).collect()
}

/// DOCTYPEs are allowed: W3C's xmldsig/xenc schemas (imported by WS-Security) start with one.
/// roxmltree expands internal entities only and never fetches an external DTD.
fn parse_xml<'i>(url: &str, content: &'i str) -> Result<roxmltree::Document<'i>, LoadError> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    roxmltree::Document::parse_with_options(content, options).map_err(|e| LoadError::Xml { url: url.to_string(), reason: e.to_string() })
}

/// Parses a document once: is its root `wsdl:definitions`, and which locations does it import or include?
fn scan(url: &str, content: &str) -> Result<(bool, Vec<String>), LoadError> {
    let xml = parse_xml(url, content)?;
    let root = xml.root_element().tag_name();
    let is_wsdl = root.namespace() == Some(WSDL_NS) && root.name() == "definitions";
    let mut refs = Vec::new();
    for node in xml.descendants().filter(|n| n.is_element()) {
        let tag = node.tag_name();
        let location = match (tag.namespace(), tag.name()) {
            (Some(WSDL_NS), "import") => node.attribute("location"),
            (Some(XS), "include") => node.attribute("schemaLocation"),
            (Some(XS), "import") if !node.attribute("namespace").is_some_and(|ns| BUILTIN_NAMESPACES.contains(&ns)) => {
                node.attribute("schemaLocation")
            }
            _ => None,
        };
        refs.extend(location.map(str::to_string));
    }
    Ok((is_wsdl, refs))
}

fn fetch(http: &mut Http, url: &Url) -> Result<String, LoadError> {
    decode(&fetch_bytes(http, url)??, url.as_str())
}

/// Outer error: the document could not be reached. Inner error: the server answered with an error status.
fn fetch_bytes(http: &mut Http, url: &Url) -> Result<Result<Vec<u8>, LoadError>, LoadError> {
    match url.scheme() {
        "file" => {
            let path = url.to_file_path().map_err(|()| fetch_err(url.as_str(), "not a usable file path"))?;
            Ok(Ok(std::fs::read(path).map_err(|e| fetch_err(url.as_str(), e))?))
        }
        "http" | "https" => {
            let options = http.options;
            if http.client.is_none() {
                let client = Client::builder().timeout(TIMEOUT).danger_accept_invalid_certs(options.insecure).build();
                http.client = Some(client.map_err(|e| fetch_err(url.as_str(), e))?);
            }
            let mut request = http.client.as_ref().unwrap().get(url.clone());
            if !options.username.is_empty() && url.origin() == http.origin {
                request = request.basic_auth(&options.username, Some(&options.password));
            }
            let response = request.send().map_err(|e| fetch_err(url.as_str(), e))?;
            if !response.status().is_success() {
                return Ok(Err(fetch_err(url.as_str(), format!("HTTP {}", response.status()))));
            }
            Ok(Ok(response.bytes().map_err(|e| fetch_err(url.as_str(), e))?.to_vec()))
        }
        other => Err(fetch_err(url.as_str(), format!("unsupported URL scheme {other}"))),
    }
}

/// UTF-8 (BOM stripped), or Latin-1 when the XML declaration says so.
// ponytail: UTF-8 and Latin-1 only; add UTF-16/other charsets when a real WSDL needs them.
pub(crate) fn decode(bytes: &[u8], url: &str) -> Result<String, LoadError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if declares_latin1(bytes) {
        return Ok(bytes.iter().map(|&b| char::from(b)).collect());
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| LoadError::Xml { url: url.to_string(), reason: e.to_string() })
}

fn declares_latin1(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"<?xml") {
        return false;
    }
    let end = bytes.windows(2).take(200).position(|w| w == b"?>").unwrap_or(0);
    let declaration = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
    declaration.contains("iso-8859-1") || declaration.contains("latin1")
}

#[cfg(test)]
mod tests {
    #[test]
    fn decode_handles_bom_and_latin1() {
        assert_eq!(super::decode(b"\xEF\xBB\xBF<a/>", "x").unwrap(), "<a/>");
        let latin1 = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>Servi\xE7o</a>";
        assert!(super::decode(latin1, "x").unwrap().contains("Serviço"));
    }
}
