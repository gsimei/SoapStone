use soapstone_core::Definition;
use soapstone_store::*;

fn fixture() -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/soapui/sample-soapui-project.xml", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn calculator_wsdl() -> String {
    let path = format!("{}/../soapstone-core/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"));
    let wsdl = std::fs::read_to_string(path).unwrap();
    match wsdl.strip_prefix("<?xml") {
        Some(rest) => rest[rest.find("?>").unwrap() + 2..].trim_start().to_string(),
        None => wsdl,
    }
}

fn call<'a>(p: &'a SoapuiProject, name: &str) -> &'a SoapuiCall {
    p.interfaces.iter().flat_map(|i| &i.calls).find(|c| c.name == name).unwrap()
}

#[test]
fn wsdl_interfaces_calls_and_properties_are_read() {
    let p = parse_soapui(&fixture()).unwrap();
    assert_eq!(p.name, "Sample");
    let props: Vec<_> = p.properties.iter().map(|v| (v.name.as_str(), v.value.as_str(), v.secret)).collect();
    assert_eq!(props, [("base", "http://localhost:8080", false), ("user", "bob", false)]);
    assert_eq!(p.interfaces.len(), 1);
    let i = &p.interfaces[0];
    assert_eq!((i.name.as_str(), i.binding.as_str()), ("CalculatorSoap", "CalculatorSoap"));
    assert_eq!(i.definition, "http://www.dneonline.com/calculator.asmx?wsdl");
    assert_eq!(i.documents[0].url, i.definition);
    assert!(Definition::parse(i.documents.clone()).is_ok());
    let calls: Vec<_> = i.calls.iter().map(|c| (c.operation.as_str(), c.name.as_str())).collect();
    assert_eq!(calls, [("Add", "Basic one"), ("Add", "NTLM one"), ("Subtract", "OAuth one")]);
}

#[test]
fn credentials_headers_and_ws_security_map() {
    let p = parse_soapui(&fixture()).unwrap();
    let basic = call(&p, "Basic one");
    assert_eq!(basic.auth, Auth::Basic { username: "{{user}}".into(), password: "pw".into() });
    let headers: Vec<_> = basic.headers.iter().map(|h| (h.name.as_str(), h.value.as_str())).collect();
    assert_eq!(headers, [("X-Trace", "{{user}}"), ("X-Amp", "a&b")]);
    assert!(!basic.ws_security.enabled());

    let ntlm = call(&p, "NTLM one");
    assert_eq!(ntlm.auth, Auth::Ntlm { username: "u".into(), password: "p".into(), domain: "CORP".into() });
    let wss = &ntlm.ws_security;
    assert!(wss.username_token && !wss.timestamp);
    assert_eq!((wss.username.as_str(), wss.password.as_str(), wss.password_type), ("u", "p", PasswordType::Digest));
    assert!(ntlm.headers.is_empty());

    assert_eq!(call(&p, "OAuth one").auth, Auth::None);
}

#[test]
fn expansions_convert_or_are_reported() {
    let p = parse_soapui(&fixture()).unwrap();
    let basic = call(&p, "Basic one");
    assert_eq!(basic.endpoint, "{{base}}/calc");
    assert!(basic.body.contains("<tem:intA>${#TestCase#id}</tem:intA>"), "{}", basic.body);
    let has = |s: &str| p.notes.iter().any(|n| n.contains(s));
    assert!(has("\"Add › Basic one\"") && has("${#TestCase#id}"), "{:?}", p.notes);
    assert!(has("OAuth 2.0 authentication is not supported"), "{:?}", p.notes);
    assert!(has("\"my prop\""), "{:?}", p.notes);
    assert!(has("Skipped interface \"Some REST API\""), "{:?}", p.notes);
    assert!(has("1 test suites ignored; they arrive in Stage 2."), "{:?}", p.notes);
    assert!(has("1 mock services ignored; they arrive in Stage 2."), "{:?}", p.notes);
}

#[test]
fn inline_definition_cache_content_is_read() {
    // Older SoapUI versions store the cached WSDL as XML elements instead of CDATA.
    let xml = format!(
        r#"<con:soapui-project name="Inline" xmlns:con="http://eviware.com/soapui/config" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <con:interface xsi:type="con:WsdlInterface" name="CalculatorSoap" bindingName="{{http://tempuri.org/}}CalculatorSoap" soapVersion="1_1" definition="file:/c.wsdl">
    <con:definitionCache type="TEXT" rootPart="file:/c.wsdl">
      <con:part><con:url>file:/c.wsdl</con:url><con:content>{}</con:content><con:type>http://schemas.xmlsoap.org/wsdl/</con:type></con:part>
    </con:definitionCache>
  </con:interface>
</con:soapui-project>"#,
        calculator_wsdl()
    );
    let p = parse_soapui(&xml).unwrap();
    let def = Definition::parse(p.interfaces[0].documents.clone()).unwrap();
    assert!(def.operation("CalculatorSoap", "Add").is_some());
}

#[test]
fn not_a_soapui_project_is_an_error() {
    assert!(parse_soapui("<a/>").is_err());
    assert!(parse_soapui("not xml").is_err());
}

fn requests_of(s: &Store, project: i64, binding: &str, name: &str) -> Vec<(i64, String)> {
    let tree = s.tree(project).unwrap();
    let op = tree.iter().flat_map(|s| &s.operations).find(|o| o.binding == binding && o.name == name).unwrap();
    op.requests.iter().map(|r| (r.id, r.name.clone())).collect()
}

#[test]
fn soapui_project_becomes_a_project() {
    let mut s = Store::open_in_memory().unwrap();
    let report = s.import_soapui(parse_soapui(&fixture()).unwrap()).unwrap();
    let p = report.project_id;
    assert_eq!(s.projects().unwrap().iter().find(|x| x.id == p).unwrap().name, "Sample");
    let vars = s.variables(p).unwrap();
    assert_eq!((vars["base"].as_str(), vars["user"].as_str(), vars.len()), ("http://localhost:8080", "bob", 2));
    let tree = s.tree(p).unwrap();
    assert_eq!((tree.len(), tree[0].name.as_str()), (1, "Calculator"));
    let names = |b, o| requests_of(&s, p, b, o).into_iter().map(|(_, n)| n).collect::<Vec<_>>();
    assert_eq!(names("CalculatorSoap", "Add"), ["Basic one", "NTLM one"]);
    assert_eq!(names("CalculatorSoap", "Subtract"), ["OAuth one"]);
    assert_eq!(names("CalculatorSoap", "Multiply"), ["Request 1"]);
    assert_eq!(names("CalculatorSoap12", "Add"), ["Request 1"]);
    // 3 calls + 2 more CalculatorSoap operations + 4 CalculatorSoap12 operations
    assert_eq!((report.services, report.requests), (1, 9));
    assert!(report.notes.iter().any(|n| n.contains("1 test suites ignored")), "{:?}", report.notes);

    let add = requests_of(&s, p, "CalculatorSoap", "Add");
    let (call, vars) = s.prepare_send(add[0].0).unwrap();
    assert_eq!(soapstone_http::render(&call.endpoint, &vars).unwrap(), "http://localhost:8080/calc");
    assert_eq!(call.auth, Auth::Basic { username: "{{user}}".into(), password: "pw".into() });
    assert_eq!(call.headers.len(), 2);
    assert_eq!(call.soap_action.as_deref(), Some("http://tempuri.org/Add"));
    let ntlm = s.request(add[1].0).unwrap();
    assert!(ntlm.ws_security.username_token && matches!(ntlm.auth, Auth::Ntlm { .. }));
    assert_eq!(s.service_source(tree[0].id).unwrap().url, "http://www.dneonline.com/calculator.asmx?wsdl");
}

#[test]
fn bad_interfaces_are_skipped_and_reported() {
    let mut project = parse_soapui(&fixture()).unwrap();
    let good = project.interfaces[0].clone();
    project.interfaces.push(SoapuiInterface { name: "NoCache".into(), documents: vec![], ..good.clone() });
    let html = soapstone_core::Document { url: "file:///x.html".into(), content: "<html/>".into() };
    project.interfaces.push(SoapuiInterface { name: "Broken".into(), documents: vec![html], ..good });
    let mut s = Store::open_in_memory().unwrap();
    let report = s.import_soapui(project).unwrap();
    assert_eq!(report.services, 1);
    let has = |x: &str| report.notes.iter().any(|n| n.contains(x));
    assert!(has("Skipped interface \"NoCache\"") && has("Skipped interface \"Broken\""), "{:?}", report.notes);
}

#[test]
fn calls_for_unknown_operations_are_reported() {
    let mut project = parse_soapui(&fixture()).unwrap();
    project.interfaces[0].calls[0].operation = "Gone".into();
    let mut s = Store::open_in_memory().unwrap();
    let report = s.import_soapui(project).unwrap();
    let note = "\"Gone › Basic one\" was not imported: the WSDL has no operation \"Gone\".";
    assert!(report.notes.iter().any(|n| n == note), "{:?}", report.notes);
    assert_eq!(report.requests, 8);
}

#[test]
fn empty_binding_name_still_matches_operations_by_name() {
    // A real SoapUI file had bindingName="{namespace}" with no local name.
    let mut project = parse_soapui(&fixture()).unwrap();
    project.interfaces[0].binding = String::new();
    let mut s = Store::open_in_memory().unwrap();
    let p = s.import_soapui(project).unwrap().project_id;
    assert_eq!(requests_of(&s, p, "CalculatorSoap", "Add").len(), 2);
}

#[test]
#[ignore = "reads the user's own SoapUI projects from the home folder; prints counts only"]
fn users_real_projects_import() {
    let home = std::env::var("HOME").unwrap();
    for name in ["Optgest-soapui-project.xml", "OPTGEST-WWW2-soapui-project.xml"] {
        let Ok(xml) = std::fs::read_to_string(format!("{home}/{name}")) else {
            eprintln!("skipped: ~/{name} is absent");
            continue;
        };
        let start = std::time::Instant::now();
        let report = std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || Store::open_in_memory().unwrap().import_soapui(parse_soapui(&xml).unwrap()).unwrap())
            .unwrap()
            .join()
            .unwrap();
        let calls = report.requests;
        eprintln!("{name}: {} services, {calls} requests, {} notes, {:.1} ms", report.services, report.notes.len(), start.elapsed().as_secs_f64() * 1000.0);
        for note in &report.notes {
            eprintln!("  note: {note}");
        }
        assert_eq!(report.services, 1);
    }
}
