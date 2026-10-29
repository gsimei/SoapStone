use soapstone_core::{Definition, Document};
use soapstone_store::*;

fn document(namespace: &str, imported: bool) -> Document {
    let import = if imported { r#"<w:import namespace="urn:second" location="second.wsdl"/>"# } else { "" };
    Document {
        url: format!("file:///review/{}.wsdl", if imported { "root" } else { "second" }),
        content: format!(
            r#"<w:definitions xmlns:w="http://schemas.xmlsoap.org/wsdl/" xmlns:s="http://schemas.xmlsoap.org/wsdl/soap/" xmlns:x="http://www.w3.org/2001/XMLSchema" xmlns:t="{namespace}" targetNamespace="{namespace}">
{import}<w:message name="M"><w:part name="v" type="x:string"/></w:message>
<w:portType name="P"><w:operation name="Op"><w:input message="t:M"/></w:operation></w:portType>
<w:binding name="B" type="t:P"><s:binding style="rpc" transport="http://schemas.xmlsoap.org/soap/http"/><w:operation name="Op"><s:operation soapAction="{namespace}:Op"/><w:input><s:body use="literal" namespace="{namespace}"/></w:input></w:operation></w:binding>
<w:service name="Service"><w:port name="Port" binding="t:B"><s:address location="http://localhost/{namespace}"/></w:port></w:service></w:definitions>"#
        ),
    }
}
fn documents() -> Vec<Document> {
    vec![document("urn:first", true), document("urn:second", false)]
}
fn response(n: u16) -> HttpResponse {
    HttpResponse {
        status: n,
        status_text: "OK".into(),
        time_ms: 10,
        size: 4,
        headers: vec![],
        body: "<r/>".into(),
        pretty: Some("<r/>".into()),
        fault: None,
    }
}

#[test]
fn namespaces_survive_import_refresh_and_portable_round_trip() {
    let definition = Definition::parse(documents()).unwrap();
    assert_eq!(definition.operation("{urn:second}B", "Op").unwrap().binding_namespace, "urn:second");
    assert_eq!(definition.operation_in("urn:second", "B", "Op").unwrap().soap_action.as_deref(), Some("urn:second:Op"));
    let mut store = Store::open_in_memory().unwrap();
    let sid = store.import_service(1, documents(), &Default::default()).unwrap();
    let tree = store.tree(1).unwrap();
    assert_eq!(tree[0].operations.len(), 2);
    let ids: Vec<_> = tree[0].operations.iter().map(|op| (op.id, op.binding_namespace.clone(), op.requests[0].id)).collect();
    let id = ids[1].2;
    let request = store.request(id).unwrap();
    let edit = RequestEdit {
        id,
        endpoint: request.endpoint,
        body: "<custom/>".into(),
        headers: vec![],
        auth: Auth::None,
        ws_security: Default::default(),
        options: Default::default(),
    };
    store.save_request(&edit).unwrap();
    store.refresh_service(sid, documents(), &Default::default()).unwrap();
    let tree = store.tree(1).unwrap();
    assert_eq!(tree[0].operations.iter().map(|o| (o.id, o.binding_namespace.clone(), o.requests[0].id)).collect::<Vec<_>>(), ids);
    assert_eq!(store.request(id).unwrap().body, "<custom/>");
    let backup = store.export_project(1).unwrap();
    let restored = store.import_project(&backup).unwrap();
    let tree = store.tree(restored).unwrap();
    assert_eq!(tree[0].operations.len(), 2);
    let request = store.request(tree[0].operations[1].requests[0].id).unwrap();
    assert_eq!((request.body.as_str(), request.soap_action.as_deref()), ("<custom/>", Some("urn:second:Op")));
}

#[test]
fn history_is_bounded_and_duplicate_has_independent_edits() {
    let mut store = Store::open_in_memory().unwrap();
    store.import_service(1, documents(), &Default::default()).unwrap();
    let id = store.tree(1).unwrap()[0].operations[0].requests[0].id;
    for status in 200..225 {
        store.save_response(id, &response(status)).unwrap();
    }
    let history = store.history(id).unwrap();
    assert_eq!(history.len(), 20);
    assert_eq!((history[0].status, history[19].status), (224, 205));
    assert_eq!(store.history_response(history[0].id).unwrap().pretty, None);
    let duplicate = store.duplicate_request(id).unwrap();
    assert_ne!(duplicate, id);
    assert_eq!(store.request(duplicate).unwrap().body, store.request(id).unwrap().body);
    assert!(store.request(duplicate).unwrap().last_response.is_none());
    assert!(store.history(duplicate).unwrap().is_empty());
    store.delete(Kind::Request, id).unwrap();
    assert!(store.history(id).unwrap().is_empty());
}

#[test]
fn exports_omit_credentials_and_secrets_but_preserve_templates_and_plans() {
    let mut store = Store::open_in_memory().unwrap();
    store.import_service(1, documents(), &FetchSettings { password: "literal-wsdl-secret".into(), ..Default::default() }).unwrap();
    store
        .set_environments(
            1,
            &Environments {
                base: vec![
                    Variable { name: "pw".into(), value: "vault-secret".into(), secret: true },
                    Variable { name: "host".into(), value: "http://localhost".into(), secret: false },
                ],
                ..Default::default()
            },
        )
        .unwrap();
    let id = store.tree(1).unwrap()[0].operations[0].requests[0].id;
    let r = store.request(id).unwrap();
    store
        .save_request(&RequestEdit {
            id,
            endpoint: r.endpoint,
            body: "<r><Password>inline-secret</Password><Username>{{user}}</Username></r>".into(),
            headers: vec![Header { name: "Authorization".into(), value: "header-secret".into() }],
            auth: Auth::Basic { username: "user".into(), password: "basic-secret".into() },
            ws_security: Default::default(),
            options: Default::default(),
        })
        .unwrap();
    let steps = vec![TestStep {
        request_id: id,
        expected_status: Some(200),
        not_fault: true,
        xpath: String::new(),
        expected_value: None,
        extract_variable: String::new(),
    }];
    store.save_test_plan(1, &steps).unwrap();
    store.save_response(id, &HttpResponse { body: "response-secret".into(), ..response(200) }).unwrap();
    let json = store.export_project(1).unwrap();
    for secret in ["literal-wsdl-secret", "vault-secret", "inline-secret", "header-secret", "basic-secret", "response-secret"] {
        assert!(!json.contains(secret), "leaked {secret}");
    }
    assert!(json.contains("{{user}}"));
    let restored = store.import_project(&json).unwrap();
    assert!(store.environments(restored, true).unwrap().base[0].value.is_empty());
    let plan = store.test_plan(restored).unwrap();
    assert_ne!(plan[0].request_id, id);
    assert!(store.request(plan[0].request_id).is_ok());
}

#[test]
fn a_broken_backup_does_not_create_a_partial_project() {
    let mut store = Store::open_in_memory().unwrap();
    store.import_service(1, documents(), &Default::default()).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&store.export_project(1).unwrap()).unwrap();
    json["services"][0]["documents"][0]["content"] = "<broken>".into();
    assert!(store.import_project(&json.to_string()).is_err());
    assert_eq!(store.projects().unwrap().len(), 1);
}

#[test]
fn invalid_variables_and_cross_project_steps_are_rejected() {
    let store = Store::open_in_memory().unwrap();
    let env = Environments { base: vec![Variable { name: "bad name".into(), value: "x".into(), secret: false }], ..Default::default() };
    assert!(store.set_environments(1, &env).is_err());
    let step = TestStep {
        request_id: 999,
        expected_status: Some(200),
        not_fault: true,
        xpath: String::new(),
        expected_value: None,
        extract_variable: String::new(),
    };
    assert!(store.save_test_plan(1, &[step]).is_err());
}

#[test]
fn soapui_calls_keep_their_binding_namespace() {
    let mut store = Store::open_in_memory().unwrap();
    let interfaces = ["urn:first", "urn:second"]
        .into_iter()
        .map(|ns| SoapuiInterface {
            name: ns.into(),
            binding: "B".into(),
            binding_namespace: ns.into(),
            definition: "file:///review/root.wsdl".into(),
            documents: documents(),
            calls: vec![SoapuiCall {
                operation: "Op".into(),
                name: ns.into(),
                endpoint: "http://localhost/service".into(),
                body: format!("<{} />", if ns == "urn:first" { "first" } else { "second" }),
                headers: vec![],
                auth: Auth::None,
                ws_security: Default::default(),
            }],
        })
        .collect();
    let report = store.import_soapui(SoapuiProject { name: "Imported".into(), properties: vec![], interfaces, notes: vec![] }).unwrap();
    assert!(report.notes.is_empty());
    let tree = store.tree(report.project_id).unwrap();
    assert_eq!(tree[0].operations.len(), 2);
    for op in &tree[0].operations {
        assert_eq!(op.requests[0].name, op.binding_namespace);
    }
}
