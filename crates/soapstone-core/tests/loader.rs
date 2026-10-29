mod common;
use soapstone_core::{FetchOptions, LoadError, fetch_all, fetch_all_with, to_url};

fn fixture(p: &str) -> url::Url {
    to_url(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn urls(docs: &[soapstone_core::Document]) -> Vec<String> {
    docs.iter().map(|d| d.url.clone()).collect()
}

#[test]
fn fetch_all_follows_wsdl_import_xsd_import_and_include() {
    let docs = fetch_all(&fixture("features/service.wsdl")).unwrap();
    let u = urls(&docs);
    assert_eq!(u.len(), 4);
    assert!(u[0].ends_with("features/service.wsdl"));
    assert!(u[1].ends_with("features/abstract.wsdl"));
    assert!(u[2].ends_with("features/xsd/common.xsd"));
    assert!(u[3].ends_with("features/xsd/parts/address.xsd"));
}

#[test]
fn fetch_all_skips_builtin_namespaces() {
    // rpc_encoded uses SOAP-ENC without any import; features imports the xml namespace without a location
    assert_eq!(fetch_all(&fixture("rpc_encoded/service.wsdl")).unwrap().len(), 1);
}

#[test]
fn fetch_all_handles_import_cycles() {
    assert_eq!(fetch_all(&fixture("cycle/root.wsdl")).unwrap().len(), 3);
}

#[test]
fn missing_import_is_a_fetch_error() {
    match fetch_all(&fixture("broken/root.wsdl")) {
        Err(LoadError::Fetch { url, .. }) => assert!(url.ends_with("broken/missing.xsd")),
        other => panic!("expected Fetch error, got {other:?}"),
    }
}

#[test]
fn http_root_without_query_retries_with_wsdl() {
    let wsdl = std::fs::read_to_string(format!("{}/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let port = common::serve(vec![("/svc", "<html><body>service page</body></html>".into()), ("/svc?wsdl", wsdl)]);
    let docs = fetch_all(&url::Url::parse(&format!("http://127.0.0.1:{port}/svc")).unwrap()).unwrap();
    assert!(docs[0].url.ends_with("/svc?wsdl"));
}

#[test]
fn http_root_returning_an_error_status_retries_with_wsdl() {
    // CXF, PHP SoapServer and Spring-WS answer the bare service URL with 404/405/500, not an HTML page.
    let wsdl = std::fs::read_to_string(format!("{}/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let port = common::serve(vec![("/svc?wsdl", wsdl)]);
    let docs = fetch_all(&url::Url::parse(&format!("http://127.0.0.1:{port}/svc")).unwrap()).unwrap();
    assert!(docs[0].url.ends_with("/svc?wsdl"));
}

#[test]
fn remote_wsdl_cannot_import_local_files() {
    let local = fixture("cycle/b.xsd");
    let wsdl = format!(
        r#"<wsdl:definitions xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/" xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:r">
             <wsdl:types><xs:schema targetNamespace="urn:r"><xs:import namespace="urn:b" schemaLocation="{local}"/></xs:schema></wsdl:types>
           </wsdl:definitions>"#
    );
    let port = common::serve(vec![("/svc?wsdl", wsdl)]);
    match fetch_all(&url::Url::parse(&format!("http://127.0.0.1:{port}/svc?wsdl")).unwrap()) {
        Err(LoadError::Fetch { url, .. }) => assert!(url.starts_with("file:"), "{url}"),
        other => panic!("expected Fetch error, got {other:?}"),
    }
}

#[test]
fn imports_from_network_file_shares_are_refused() {
    match fetch_all(&fixture("remote_share/root.wsdl")) {
        Err(LoadError::Fetch { reason, .. }) => assert!(reason.contains("not on this computer"), "{reason}"),
        other => panic!("expected Fetch error, got {other:?}"),
    }
}

#[test]
fn schemas_with_a_doctype_load() {
    let docs = fetch_all(&fixture("dtd/root.wsdl")).unwrap();
    assert_eq!(docs.len(), 2);
    assert!(soapstone_core::Definition::parse(docs).is_ok());
}

#[test]
fn basic_credentials_go_only_to_the_wsdl_origin() {
    let schema = r#"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t"/>"#;
    let (b, b_heads) = common::serve_recording(vec![("/t.xsd", schema.into())]);
    let wsdl = format!(
        r#"<wsdl:definitions xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/" xmlns:xsd="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a">
             <wsdl:types><xsd:schema targetNamespace="urn:a"><xsd:import namespace="urn:t" schemaLocation="http://127.0.0.1:{b}/t.xsd"/></xsd:schema></wsdl:types>
           </wsdl:definitions>"#
    );
    let (a, a_heads) = common::serve_recording(vec![("/svc?wsdl", wsdl)]);
    let options = FetchOptions { username: "u".into(), password: "p".into(), ..Default::default() };
    let docs = fetch_all_with(&url::Url::parse(&format!("http://127.0.0.1:{a}/svc")).unwrap(), &options).unwrap();
    assert_eq!(docs.len(), 2);
    let a_heads = a_heads.lock().unwrap();
    assert_eq!(a_heads.len(), 2, "the bare /svc attempt and the ?wsdl retry");
    for head in a_heads.iter() {
        assert!(head.to_lowercase().contains("authorization: basic dtpw"), "{head}");
    }
    let b_heads = b_heads.lock().unwrap();
    assert_eq!(b_heads.len(), 1);
    assert!(!b_heads[0].to_lowercase().contains("authorization"), "{}", b_heads[0]);
}

#[test]
fn untrusted_certificate_needs_insecure() {
    let wsdl = std::fs::read_to_string(format!("{}/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let port = common::serve_tls(wsdl);
    let url = url::Url::parse(&format!("https://127.0.0.1:{port}/svc?wsdl")).unwrap();
    assert!(matches!(fetch_all(&url), Err(LoadError::Fetch { .. })));
    let docs = fetch_all_with(&url, &FetchOptions { insecure: true, ..Default::default() }).unwrap();
    assert_eq!(docs.len(), 1);
}
