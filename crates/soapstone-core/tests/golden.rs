mod common;
use soapstone_core::Definition;

fn check_fixture(dir: &str, root: &str) {
    let base = format!("{}/tests/fixtures/{dir}", env!("CARGO_MANIFEST_DIR"));
    let def = Definition::load(&format!("{base}/{root}")).unwrap();
    let mut n = 0;
    for entry in std::fs::read_dir(format!("{base}/expected")).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let (binding, op) = stem.split_once("__").unwrap();
        let op = def.operation(binding, op).unwrap_or_else(|| panic!("missing operation {stem}"));
        let expected = std::fs::read_to_string(&path).unwrap();
        println!("checking {stem}");
        common::assert_xml_equiv(&expected, &def.sample_request(op));
        n += 1;
    }
    assert!(n > 0);
}

#[test]
fn calculator_matches_soapui() {
    check_fixture("calculator", "calculator.wsdl");
}

#[test]
fn features_matches_soapui() {
    check_fixture("features", "service.wsdl");
}

#[test]
fn calculator_add_exact_text() {
    let def = Definition::load(&format!("{}/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let op = def.operation("CalculatorSoap", "Add").unwrap();
    assert_eq!(def.sample_request(op), concat!(
        "<soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:tem=\"http://tempuri.org/\">\n",
        "   <soapenv:Header/>\n",
        "   <soapenv:Body>\n",
        "      <tem:Add>\n",
        "         <tem:intA>?</tem:intA>\n",
        "         <tem:intB>?</tem:intB>\n",
        "      </tem:Add>\n",
        "   </soapenv:Body>\n",
        "</soapenv:Envelope>"));
}

#[test]
fn unresolved_type_still_generates() {
    let wsdl = r#"<wsdl:definitions xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/" xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/"
        xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:tns="urn:dirty" targetNamespace="urn:dirty">
      <wsdl:types><xs:schema targetNamespace="urn:dirty" elementFormDefault="qualified">
        <xs:element name="Op"><xs:complexType><xs:sequence>
          <xs:element name="a" type="tns:Missing"/>
          <xs:element name="b" type="nope:Undeclared"/>
          <xs:element ref="tns:alsoMissing"/>
        </xs:sequence></xs:complexType></xs:element>
      </xs:schema></wsdl:types>
      <wsdl:message name="In"><wsdl:part name="p" element="tns:Op"/></wsdl:message>
      <wsdl:portType name="P"><wsdl:operation name="Op"><wsdl:input message="tns:In"/></wsdl:operation></wsdl:portType>
      <wsdl:binding name="B" type="tns:P"><soap:binding style="document" transport="http://schemas.xmlsoap.org/soap/http"/>
        <wsdl:operation name="Op"><soap:operation soapAction="Op"/><wsdl:input><soap:body use="literal"/></wsdl:input></wsdl:operation>
      </wsdl:binding>
    </wsdl:definitions>"#;
    let def = Definition::parse(vec![soapstone_core::Document { url: "file:///dirty.wsdl".into(), content: wsdl.into() }]).unwrap();
    let xml = def.sample_request(def.operation("B", "Op").unwrap());
    assert!(xml.contains(":a>?</"), "{xml}");
    assert!(xml.contains(":b>?</"), "{xml}");
}

#[test]
fn rpc_literal_matches_soapui() {
    check_fixture("rpc_literal", "service.wsdl");
}

#[test]
fn rpc_encoded_matches_soapui() {
    check_fixture("rpc_encoded", "service.wsdl");
}

#[test]
fn rpc_encoded_has_the_shape_of_the_users_request() {
    let def = Definition::load(&format!("{}/tests/fixtures/rpc_encoded/service.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let xml = def.sample_request(def.operation("EncodedSoap", "CreateBooking").unwrap());
    for needle in [
        r#"<soapenv:Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:soapenv="#,
        r#"<urn:CreateBooking soapenv:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">"#,
        r#"<Credential xsi:type="urn:Credentials">"#,
        r#"<SystemID xsi:type="xsd:int">?</SystemID>"#,
        r#"<Travellers xsi:type="urn:TravellerArray" soapenc:arrayType="urn:Traveller[]"/>"#,
    ] {
        assert!(xml.contains(needle), "missing {needle} in\n{xml}");
    }
}

#[test]
fn optitravel_matches_soapui() {
    if common::local_fixture("optitravel/optitravel.wsdl").is_some() {
        check_fixture("optitravel", "optitravel.wsdl");
    }
}

#[test]
fn optitravel_has_the_shape_of_the_users_request() {
    let Some(path) = common::local_fixture("optitravel/optitravel.wsdl") else { return };
    let def = Definition::load(&path).unwrap();
    let xml = def.sample_request(def.operation("wbs_methodsSoap", "AVITIK_GETGROUPRESERVES").unwrap());
    for needle in [
        r#"xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance""#,
        r#"xmlns:xsd="http://www.w3.org/2001/XMLSchema""#,
        r#"<opti:AVITIK_GETGROUPRESERVES soapenv:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">"#,
        r#"<Credential xsi:type="opti:Credentials">"#,
        r#"<SystemID xsi:type="xsd:int">?</SystemID>"#,
        r#"<Username xsi:type="xsd:string">?</Username>"#,
        r#"<GroupReservesSearchParameters xsi:type="opti:GroupReservesSearchParameters">"#,
    ] {
        assert!(xml.contains(needle), "missing {needle} in\n{xml}");
    }
}
