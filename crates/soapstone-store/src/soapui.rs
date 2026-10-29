//! SoapUI projects (`*-soapui-project.xml`): parsed into plain values, then stored as a Soapstone project.

use std::collections::{HashMap, HashSet};

use roxmltree::Node;
use rusqlite::params;
use serde::Serialize;
use soapstone_core::{Definition, Document};

use crate::services::{insert_operation, insert_operation_row, insert_service, unique_operations};
use crate::{Auth, Environments, FetchSettings, Header, PasswordType, Result, Store, Variable, WsSecurity};

const CON: &str = "http://eviware.com/soapui/config";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

#[derive(Debug, Clone)]
pub struct SoapuiProject {
    pub name: String,
    pub properties: Vec<Variable>,
    pub interfaces: Vec<SoapuiInterface>,
    /// What was skipped or changed, for the import report.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SoapuiInterface {
    pub name: String,
    /// Local name of the WSDL binding.
    pub binding: String,
    /// The WSDL URL SoapUI loaded.
    pub definition: String,
    /// The cached WSDL and schemas, root first; empty when SoapUI kept no cache.
    pub documents: Vec<Document>,
    pub calls: Vec<SoapuiCall>,
}

/// One saved SoapUI request.
#[derive(Debug, Clone, PartialEq)]
pub struct SoapuiCall {
    pub operation: String,
    pub name: String,
    pub endpoint: String,
    pub body: String,
    pub headers: Vec<Header>,
    pub auth: Auth,
    pub ws_security: WsSecurity,
}

/// What an import created, and what it skipped or changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub project_id: i64,
    pub services: usize,
    pub requests: usize,
    pub notes: Vec<String>,
}

impl Store {
    /// A new project named after the SoapUI one, in one transaction: properties become base variables, each
    /// cached WSDL a service, and saved calls the requests of their operation (operations without calls get
    /// "Request 1"). Interfaces and calls that cannot be imported are reported, not fatal.
    pub fn import_soapui(&mut self, project: SoapuiProject) -> Result<ImportReport> {
        let mut notes = project.notes;
        let tx = self.conn.transaction()?;
        let environments = Environments { base: project.properties, ..Default::default() };
        tx.execute(
            "INSERT INTO projects (name, environments) VALUES (?1, ?2)",
            params![project.name, serde_json::to_string(&environments)?],
        )?;
        let project_id = tx.last_insert_rowid();
        let (mut services, mut requests) = (0, 0);
        for interface in project.interfaces {
            let skipped = |reason: &str| format!("Skipped interface \"{}\": {reason}.", interface.name);
            if interface.documents.is_empty() {
                notes.push(skipped("SoapUI kept no copy of its WSDL; import the WSDL by URL instead"));
                continue;
            }
            let def = match Definition::parse(interface.documents.clone()) {
                Ok(def) => def,
                Err(e) => {
                    notes.push(skipped(&e.to_string()));
                    continue;
                }
            };
            let operations: Vec<_> = unique_operations(&def).collect();
            // A call goes to its operation in the interface's binding, else to the first operation of that name
            // (SoapUI can store a binding name without its local part).
            let mut calls: HashMap<(&str, &str), Vec<&SoapuiCall>> = HashMap::new();
            for call in &interface.calls {
                let target = operations
                    .iter()
                    .find(|o| o.binding == interface.binding && o.name == call.operation)
                    .or_else(|| operations.iter().find(|o| o.name == call.operation));
                match target {
                    Some(op) => calls.entry((op.binding.as_str(), op.name.as_str())).or_default().push(call),
                    None => notes.push(format!(
                        "\"{} › {}\" was not imported: the WSDL has no operation \"{}\".",
                        call.operation, call.name, call.operation
                    )),
                }
            }
            let service_id = insert_service(&tx, project_id, &def, &FetchSettings::default())?;
            services += 1;
            for op in operations {
                let Some(saved) = calls.get(&(op.binding.as_str(), op.name.as_str())) else {
                    insert_operation(&tx, &def, service_id, op)?;
                    requests += 1;
                    continue;
                };
                let operation_id = insert_operation_row(&tx, service_id, op)?;
                for call in saved {
                    let endpoint = if call.endpoint.is_empty() { op.endpoint.as_deref().unwrap_or("") } else { &call.endpoint };
                    tx.execute(
                        "INSERT INTO requests (operation_id, name, endpoint, body, headers, auth, ws_security)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            operation_id,
                            call.name,
                            endpoint,
                            call.body,
                            serde_json::to_string(&call.headers)?,
                            serde_json::to_string(&call.auth)?,
                            serde_json::to_string(&call.ws_security)?
                        ],
                    )?;
                    requests += 1;
                }
            }
        }
        tx.commit()?;
        Ok(ImportReport { project_id, services, requests, notes })
    }
}

/// Reads a SoapUI project. `Err` only when the text is not XML or not a SoapUI project.
pub fn parse_soapui(xml: &str) -> std::result::Result<SoapuiProject, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(xml, options).map_err(|e| format!("Not an XML file ({e})."))?;
    let root = doc.root_element();
    if !root.has_tag_name((CON, "soapui-project")) {
        return Err("Not a SoapUI project: the file's root element is not soapui-project.".into());
    }
    let mut notes = Vec::new();

    let mut properties = Vec::new();
    for p in children(root, "properties").flat_map(|ps| children(ps, "property")) {
        let (name, value) = (text(child(p, "name")), text(child(p, "value")));
        if is_variable(&name) {
            properties.push(Variable { name, value, secret: false });
        } else {
            notes.push(format!("Property \"{name}\" was not imported: variable names use letters, digits, _, . and -."));
        }
    }
    let names: HashSet<String> = properties.iter().map(|v| v.name.clone()).collect();

    let mut interfaces = Vec::new();
    for i in children(root, "interface") {
        let name = i.attribute("name").unwrap_or("Interface").to_string();
        let kind = i.attribute((XSI, "type")).unwrap_or("");
        if !(kind.ends_with("WsdlInterface") || (kind.is_empty() && i.attribute("bindingName").is_some())) {
            notes.push(format!("Skipped interface \"{name}\": only WSDL (SOAP) interfaces are imported."));
            continue;
        }
        let binding = i.attribute("bindingName").unwrap_or("");
        let binding = binding.rsplit_once('}').map_or(binding, |(_, local)| local).to_string();
        let mut calls = Vec::new();
        for op in children(i, "operation") {
            let operation = op.attribute("name").unwrap_or("");
            for c in children(op, "call") {
                calls.push(call(operation, c, &names, &mut notes));
            }
        }
        let definition = i.attribute("definition").unwrap_or("").to_string();
        interfaces.push(SoapuiInterface { name, binding, definition, documents: cache(xml, i), calls });
    }

    for (tag, what) in [("testSuite", "test suites"), ("mockService", "mock services")] {
        let n = children(root, tag).count();
        if n > 0 {
            notes.push(format!("{n} {what} ignored; they arrive in Stage 2."));
        }
    }
    if children(root, "wssContainer").any(|w| w.children().any(|c| c.is_element())) {
        notes.push("The project's WS-Security configurations and keystores were not imported; set WS-Security on each request.".into());
    }
    if root.descendants().any(|n| n.has_tag_name((CON, "call")) && n.attribute("sslKeystore").is_some_and(|k| !k.is_empty())) {
        notes.push("Client certificates (SSL keystores) were not imported; set them in each request's Auth tab.".into());
    }
    let name = root.attribute("name").unwrap_or("SoapUI project").to_string();
    Ok(SoapuiProject { name, properties, interfaces, notes })
}

fn call(operation: &str, c: Node, properties: &HashSet<String>, notes: &mut Vec<String>) -> SoapuiCall {
    let name = c.attribute("name").unwrap_or("Request").to_string();
    let label = format!("\"{operation} › {name}\"");
    let mut unresolved = Vec::new();
    let mut x = |s: String| expand(&s, properties, &mut unresolved);

    let endpoint = x(text(child(c, "endpoint")).trim().to_string());
    let body = x(text(child(c, "request")));
    let headers = request_headers(c).into_iter().map(|(n, v)| Header { name: x(n), value: x(v) }).collect();
    let credentials = child(c, "credentials");
    let field = |n: &'static str| text(credentials.and_then(|cr| child(cr, n)));
    let (username, password, domain) = (x(field("username")), x(field("password")), x(field("domain")));

    let auth_type = field("authType");
    let auth = match auth_type.as_str() {
        "NTLM" if !username.is_empty() => Auth::Ntlm { username: username.clone(), password: password.clone(), domain },
        "Basic" | "Preemptive" | "Global HTTP Settings" | "" if !username.is_empty() => {
            Auth::Basic { username: username.clone(), password: password.clone() }
        }
        "Basic" | "Preemptive" | "Global HTTP Settings" | "" | "NTLM" | "No Authorization" => Auth::None,
        other => {
            notes.push(format!("{label}: {other} authentication is not supported; imported without it."));
            Auth::None
        }
    };

    let password_type = match c.attribute("wssPasswordType").unwrap_or("") {
        "" => None,
        "PasswordText" => Some(PasswordType::Text),
        "PasswordDigest" => Some(PasswordType::Digest),
        other => {
            notes.push(format!("{label}: WS-Security password type {other} is not supported; imported without it."));
            None
        }
    };
    let ws_security = match password_type {
        Some(password_type) => WsSecurity { username_token: true, username, password, password_type, ..Default::default() },
        None => WsSecurity::default(),
    };

    if !unresolved.is_empty() {
        notes.push(format!("{label} keeps SoapUI expressions Soapstone cannot resolve: {}", unresolved.join(", ")));
    }
    SoapuiCall { operation: operation.to_string(), name, endpoint, body, headers, auth, ws_security }
}

/// `${#Project#name}` of a known property becomes `{{name}}`; any other `${…}` stays and is collected.
// ponytail: an expression ends at its first `}`; nested Groovy (`${= ... {…} }`) is reported cut short but left intact.
fn expand(s: &str, properties: &HashSet<String>, unresolved: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        let Some(len) = rest[start..].find('}') else { break };
        let expression = &rest[start..start + len + 1];
        out.push_str(&rest[..start]);
        match expression.strip_prefix("${#Project#").and_then(|e| e.strip_suffix('}')) {
            Some(name) if properties.contains(name) => out.push_str(&format!("{{{{{name}}}}}")),
            _ => {
                out.push_str(expression);
                if !unresolved.iter().any(|u| u == expression) {
                    unresolved.push(expression.to_string());
                }
            }
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// The `…@request-headers` setting: an escaped `<xml-fragment>` of `<entry key=".." value=".."/>`.
fn request_headers(c: Node) -> Vec<(String, String)> {
    let setting = children(c, "settings")
        .flat_map(|s| children(s, "setting"))
        .find(|s| s.attribute("id").is_some_and(|id| id.ends_with("@request-headers")));
    let fragment = text(setting);
    let Ok(doc) = roxmltree::Document::parse(fragment.trim()) else { return vec![] };
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .map(|n| (n.attribute("key").unwrap_or("").to_string(), n.attribute("value").unwrap_or("").to_string()))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

/// The cached documents, root part first. Content is CDATA text, or (older files) the document's own elements.
fn cache(source: &str, interface: Node) -> Vec<Document> {
    let Some(cache) = child(interface, "definitionCache") else { return vec![] };
    let root = cache.attribute("rootPart").unwrap_or("");
    let mut documents: Vec<Document> = children(cache, "part")
        .map(|part| {
            let content = child(part, "content");
            let content = match content.and_then(|c| c.first_element_child()) {
                Some(element) => source[element.range()].to_string(),
                None => text(content).trim_start().to_string(),
            };
            Document { url: text(child(part, "url")).trim().to_string(), content }
        })
        .collect();
    if let Some(i) = documents.iter().position(|d| d.url == root) {
        let root = documents.remove(i);
        documents.insert(0, root);
    }
    documents
}

fn children<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| n.has_tag_name((CON, name)))
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> Option<Node<'a, 'i>> {
    children(node, name).next()
}

/// All text directly inside the element (CDATA included).
fn text(node: Option<Node>) -> String {
    node.map(|n| n.children().filter(|c| c.is_text()).filter_map(|c| c.text()).collect()).unwrap_or_default()
}

/// Soapstone's `{{name}}` rule.
fn is_variable(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}
