mod common;
use soapstone_core::{Definition, SoapVersion, Style};

fn load(p: &str) -> Definition {
    Definition::load(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn ids(d: &Definition) -> Vec<(String, String)> {
    d.operations().iter().map(|o| (o.binding.clone(), o.name.clone())).collect()
}

#[test]
fn calculator_lists_both_bindings() {
    let d = load("calculator/calculator.wsdl");
    let expected: Vec<(String, String)> = ["CalculatorSoap", "CalculatorSoap12"]
        .iter()
        .flat_map(|b| ["Add", "Subtract", "Multiply", "Divide"].map(|o| (b.to_string(), o.to_string())))
        .collect();
    assert_eq!(ids(&d), expected);
    let add12 = d.operation("CalculatorSoap12", "Add").unwrap();
    assert_eq!(add12.soap_version, SoapVersion::V12);
    assert_eq!(add12.style, Style::Document);
    assert!(!add12.encoded);
    assert_eq!(add12.soap_action.as_deref(), Some("http://tempuri.org/Add"));
    assert_eq!(add12.endpoint.as_deref(), Some("http://www.dneonline.com/calculator.asmx"));
}

#[test]
fn optitravel_is_rpc_encoded() {
    let Some(path) = common::local_fixture("optitravel/optitravel.wsdl") else { return };
    let d = Definition::load(&path).unwrap();
    assert_eq!(d.operations().len(), 18);
    for op in d.operations() {
        assert_eq!(op.binding, "wbs_methodsSoap");
        assert_eq!(op.style, Style::Rpc);
        assert!(op.encoded);
        assert_eq!(op.endpoint.as_deref(), Some("https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php"));
    }
    let op = d.operation("wbs_methodsSoap", "AVITIK_GETGROUPRESERVES").unwrap();
    assert_eq!(op.soap_action.as_deref(), Some("http://www2.optitravel.net/optitravel/export/wbs/opti_res/AVITIK_GETGROUPRESERVES"));
}

#[test]
fn rpc_encoded_fixture_is_rpc_encoded() {
    let d = load("rpc_encoded/service.wsdl");
    assert_eq!(ids(&d), vec![
        ("EncodedSoap".to_string(), "CreateBooking".to_string()),
        ("EncodedSoap".to_string(), "GetBooking".to_string()),
    ]);
    for op in d.operations() {
        assert_eq!(op.soap_version, SoapVersion::V11);
        assert_eq!(op.style, Style::Rpc);
        assert!(op.encoded);
        assert_eq!(op.endpoint.as_deref(), Some("http://localhost:8080/encoded"));
    }
    assert_eq!(d.operation("EncodedSoap", "CreateBooking").unwrap().soap_action.as_deref(), Some("urn:encoded#CreateBooking"));
}

#[test]
fn features_ignores_http_binding_and_follows_wsdl_import() {
    let d = load("features/service.wsdl");
    assert_eq!(ids(&d), vec![
        ("FeaturesSoap".to_string(), "GetPerson".to_string()),
        ("FeaturesSoap".to_string(), "SaveEmployee".to_string()),
    ]);
    assert_eq!(d.operation("FeaturesSoap", "GetPerson").unwrap().endpoint.as_deref(), Some("http://localhost:8080/features"));
    assert_eq!(d.documents().len(), 4);
}

#[test]
fn parse_works_offline_from_documents() {
    let docs = load("features/service.wsdl").documents().to_vec();
    assert_eq!(Definition::parse(docs).unwrap().operations().len(), 2);
}
