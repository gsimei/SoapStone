use soapstone_core::{Definition, Document, SoapVersion, fetch_all, to_url};
use soapstone_http::SoapCall;
use soapstone_store::*;

fn docs(fixture: &str) -> Vec<Document> {
    let path = format!("{}/../soapstone-core/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    fetch_all(&to_url(&path).unwrap()).unwrap()
}
fn sample(fixture: &str, binding: &str, op: &str) -> String {
    let d = Definition::parse(docs(fixture)).unwrap();
    d.sample_request(d.operation(binding, op).unwrap())
}
fn op<'a>(tree: &'a [ServiceNode], name: &str) -> &'a OperationNode {
    tree.iter().flat_map(|s| &s.operations).find(|o| o.name == name).unwrap()
}
fn edit(id: i64, endpoint: &str, body: &str) -> RequestEdit {
    RequestEdit {
        id,
        endpoint: endpoint.into(),
        body: body.into(),
        headers: vec![],
        auth: Auth::None,
        ws_security: Default::default(),
        options: Default::default(),
    }
}
fn none() -> FetchSettings {
    FetchSettings::default()
}

#[test]
fn import_creates_operations_each_with_a_sample_request() {
    let mut s = Store::open_in_memory().unwrap();
    let sid = s.import_service(1, docs("features/service.wsdl"), &none()).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!((tree.len(), tree[0].id, tree[0].name.as_str()), (1, sid, "Features"));
    let ops: Vec<_> = tree[0].operations.iter().map(|o| (o.binding.as_str(), o.name.as_str(), o.orphan, o.requests.len())).collect();
    assert_eq!(ops, [("FeaturesSoap", "GetPerson", false, 1), ("FeaturesSoap", "SaveEmployee", false, 1)]);
    let r = s.request(op(&tree, "GetPerson").requests[0].id).unwrap();
    assert_eq!((r.name.as_str(), r.operation.as_str(), r.binding.as_str()), ("Request 1", "GetPerson", "FeaturesSoap"));
    assert_eq!(r.body, sample("features/service.wsdl", "FeaturesSoap", "GetPerson"));
    assert_eq!(r.endpoint, "http://localhost:8080/features");
    assert_eq!((r.soap_version.as_str(), r.soap_action.as_deref()), ("1.1", Some("urn:features/GetPerson")));
    assert_eq!((r.headers.len(), r.auth, r.last_response), (0, Auth::None, None));
    assert!(s.service_source(sid).unwrap().url.ends_with("features/service.wsdl"));
}

#[test]
fn calculator_keeps_both_bindings_and_versions() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("calculator/calculator.wsdl"), &none()).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!((tree[0].name.as_str(), tree[0].operations.len()), ("Calculator", 8));
    let add12 = tree[0].operations.iter().find(|o| o.binding == "CalculatorSoap12" && o.name == "Add").unwrap();
    assert_eq!(s.request(add12.requests[0].id).unwrap().soap_version, "1.2");
}

#[test]
fn importing_something_that_is_not_a_wsdl_stores_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    let html = vec![Document { url: "file:///page.html".into(), content: "<html/>".into() }];
    assert!(matches!(s.import_service(1, html, &none()), Err(StoreError::Load(_))));
    assert!(s.tree(1).unwrap().is_empty());
}

#[test]
fn edits_and_the_last_response_are_saved() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("features/service.wsdl"), &none()).unwrap();
    let id = op(&s.tree(1).unwrap(), "GetPerson").requests[0].id;
    let e = RequestEdit {
        id,
        endpoint: "{{baseUrl}}/features".into(),
        body: "<x/>".into(),
        headers: vec![Header { name: "X-A".into(), value: "1".into() }],
        auth: Auth::Bearer { token: "{{t}}".into() },
        ws_security: Default::default(),
        options: Default::default(),
    };
    s.save_request(&e).unwrap();
    let mut resp = HttpResponse {
        status: 200,
        status_text: "OK".into(),
        time_ms: 12,
        size: 4,
        headers: vec![],
        body: "<r/>".into(),
        pretty: Some("<r/>".into()),
        fault: None,
    };
    s.save_response(id, &resp).unwrap();
    resp.pretty = None; // formatting is recreated by the active response tab, not persisted
    let r = s.request(id).unwrap();
    assert_eq!((r.endpoint, r.body, r.headers, r.auth, r.last_response), (e.endpoint, e.body, e.headers, e.auth, Some(resp)));
}

#[test]
fn new_requests_are_numbered_and_start_from_the_sample() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("features/service.wsdl"), &none()).unwrap();
    let get = op(&s.tree(1).unwrap(), "GetPerson").id;
    let r = s.request(s.create_request(get).unwrap()).unwrap();
    assert_eq!(r.name, "Request 2");
    assert_eq!(r.body, sample("features/service.wsdl", "FeaturesSoap", "GetPerson"));
    assert_eq!(op(&s.tree(1).unwrap(), "GetPerson").requests.len(), 2);
}

#[test]
fn refresh_marks_missing_operations_orphan_and_keeps_requests() {
    let mut s = Store::open_in_memory().unwrap();
    let sid = s.import_service(1, docs("features/service.wsdl"), &none()).unwrap();
    let get = op(&s.tree(1).unwrap(), "GetPerson").requests[0].id;
    s.save_request(&edit(get, "http://edited", "<edited/>")).unwrap();

    s.refresh_service(sid, docs("calculator/calculator.wsdl"), &none()).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!(tree[0].operations.len(), 10);
    assert!(op(&tree, "GetPerson").orphan && op(&tree, "SaveEmployee").orphan);
    assert!(!op(&tree, "Add").orphan && op(&tree, "Add").requests.len() == 1);
    assert_eq!(s.request(get).unwrap().body, "<edited/>");

    s.refresh_service(sid, docs("features/service.wsdl"), &none()).unwrap();
    let tree = s.tree(1).unwrap();
    assert!(!op(&tree, "GetPerson").orphan && op(&tree, "Add").orphan);
    assert_eq!(op(&tree, "GetPerson").requests.len(), 1);
    assert_eq!(s.request(get).unwrap().endpoint, "http://edited");
}

#[test]
fn prepare_send_combines_request_operation_and_variables() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("calculator/calculator.wsdl"), &none()).unwrap();
    s.set_environments(
        1,
        &Environments { base: vec![Variable { name: "baseUrl".into(), value: "http://x".into(), secret: false }], ..Default::default() },
    )
    .unwrap();
    let tree = s.tree(1).unwrap();
    let add12 = tree[0].operations.iter().find(|o| o.binding == "CalculatorSoap12" && o.name == "Add").unwrap();
    let id = add12.requests[0].id;
    s.save_request(&edit(id, "{{baseUrl}}/calc", "<b/>")).unwrap();
    let (call, vars) = s.prepare_send(id).unwrap();
    assert_eq!(
        call,
        SoapCall {
            endpoint: "{{baseUrl}}/calc".into(),
            soap_version: SoapVersion::V12,
            soap_action: Some("http://tempuri.org/Add".into()),
            headers: vec![],
            auth: Auth::None,
            body: "<b/>".into(),
            ws_security: Default::default(),
            options: Default::default(),
        }
    );
    assert_eq!(vars["baseUrl"], "http://x");
}

#[test]
fn renaming_and_deleting_work_for_every_kind() {
    let mut s = Store::open_in_memory().unwrap();
    let p = s.create_project("P").unwrap();
    let sid = s.import_service(p, docs("features/service.wsdl"), &none()).unwrap();
    let tree = s.tree(p).unwrap();
    let (get, save) = (op(&tree, "GetPerson").requests[0].id, op(&tree, "SaveEmployee").requests[0].id);
    s.rename(Kind::Service, sid, "Svc").unwrap();
    s.rename(Kind::Request, get, "Mine").unwrap();
    let tree = s.tree(p).unwrap();
    assert_eq!((tree[0].name.as_str(), op(&tree, "GetPerson").requests[0].name.as_str()), ("Svc", "Mine"));
    s.delete(Kind::Request, get).unwrap();
    assert!(op(&s.tree(p).unwrap(), "GetPerson").requests.is_empty());
    s.delete(Kind::Project, p).unwrap();
    assert!(s.tree(p).unwrap().is_empty());
    assert!(matches!(s.request(save), Err(StoreError::NotFound(_))));
}

#[test]
fn overloaded_operation_names_keep_the_first() {
    // Real WSDLs (Java overloads) repeat an operation name inside one binding.
    let wsdl = r#"<definitions xmlns="http://schemas.xmlsoap.org/wsdl/" xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:tns="urn:o" targetNamespace="urn:o" name="Over">
  <message name="A"/>
  <message name="B"><part name="x" type="xsd:string"/></message>
  <portType name="P">
    <operation name="Get"><input message="tns:A"/><output message="tns:A"/></operation>
    <operation name="Get"><input message="tns:B"/><output message="tns:A"/></operation>
  </portType>
  <binding name="Bind" type="tns:P">
    <soap:binding style="rpc" transport="http://schemas.xmlsoap.org/soap/http"/>
    <operation name="Get"><soap:operation soapAction="urn:o#Get1"/><input><soap:body use="literal" namespace="urn:o"/></input><output><soap:body use="literal" namespace="urn:o"/></output></operation>
    <operation name="Get"><soap:operation soapAction="urn:o#Get2"/><input><soap:body use="literal" namespace="urn:o"/></input><output><soap:body use="literal" namespace="urn:o"/></output></operation>
  </binding>
  <service name="Over"><port name="Port" binding="tns:Bind"><soap:address location="http://localhost/o"/></port></service>
</definitions>"#;
    let doc = || vec![Document { url: "file:///over.wsdl".into(), content: wsdl.into() }];
    let mut s = Store::open_in_memory().unwrap();
    let sid = s.import_service(1, doc(), &none()).unwrap();
    s.refresh_service(sid, doc(), &none()).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!(tree[0].operations.len(), 1);
    let get = op(&tree, "Get");
    assert_eq!(get.requests.len(), 1);
    assert_eq!(s.request(get.requests[0].id).unwrap().soap_action.as_deref(), Some("urn:o#Get1"));
}

#[test]
fn ws_security_and_options_round_trip_to_the_call() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("features/service.wsdl"), &none()).unwrap();
    let id = op(&s.tree(1).unwrap(), "GetPerson").requests[0].id;
    let wss = WsSecurity { username_token: true, username: "u".into(), password: "{{pw}}".into(), ..Default::default() };
    let options = RequestOptions { timeout_seconds: 5, insecure: true, cert_path: "/c.p12".into(), ..Default::default() };
    s.save_request(&RequestEdit { ws_security: wss.clone(), options: options.clone(), ..edit(id, "http://x", "<b/>") }).unwrap();
    let r = s.request(id).unwrap();
    assert_eq!((&r.ws_security, &r.options), (&wss, &options));
    let call = s.prepare_send(id).unwrap().0;
    assert_eq!((call.ws_security, call.options), (wss, options));
}

#[test]
fn service_keeps_its_fetch_settings_and_renders_them() {
    let mut s = Store::open_in_memory().unwrap();
    let fetch = FetchSettings { username: "{{wu}}".into(), password: "pw".into(), insecure: true };
    let sid = s.import_service(1, docs("features/service.wsdl"), &fetch).unwrap();
    let source = s.service_source(sid).unwrap();
    assert_eq!((source.project_id, &source.fetch), (1, &fetch));
    assert!(source.url.ends_with("features/service.wsdl"));

    match s.fetch_options(1, &fetch) {
        Err(StoreError::Variables(names)) => assert_eq!(names, ["wu"]),
        other => panic!("expected undefined variables, got {other:?}"),
    }
    s.set_environments(
        1,
        &Environments { base: vec![Variable { name: "wu".into(), value: "bob".into(), secret: false }], ..Default::default() },
    )
    .unwrap();
    let options = s.fetch_options(1, &fetch).unwrap();
    assert_eq!(options, soapstone_core::FetchOptions { username: "bob".into(), password: "pw".into(), insecure: true });

    let changed = FetchSettings { username: "alice".into(), ..Default::default() };
    s.refresh_service(sid, docs("features/service.wsdl"), &changed).unwrap();
    assert_eq!(s.service_source(sid).unwrap().fetch, changed);
}
