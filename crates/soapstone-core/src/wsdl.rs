//! WSDL 1.1: the SOAP operations a service offers and which message parts go in each request.

use std::collections::HashMap;

use roxmltree::Node;

use crate::loader::{Document, LoadError, WSDL_NS, fetch_all, parse_all, to_url};
use crate::qname::{QName, is, resolve};
use crate::xsd::SchemaSet;

const SOAP11_NS: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
const SOAP12_NS: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoapVersion {
    V11,
    V12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Document,
    Rpc,
}

#[derive(Debug, Clone)]
pub struct Operation {
    /// Binding local name, e.g. "CalculatorSoap12".
    pub binding: String,
    /// Namespace of the binding; local names alone are not unique across imported WSDLs.
    pub binding_namespace: String,
    pub name: String,
    pub soap_version: SoapVersion,
    pub style: Style,
    /// The input `soap:body` has `use="encoded"`.
    pub encoded: bool,
    pub soap_action: Option<String>,
    /// `soap:address` of the first port using this binding.
    pub endpoint: Option<String>,
    pub(crate) input: InputSpec,
}

#[derive(Debug, Clone)]
pub(crate) struct InputSpec {
    /// Message parts filtered by `soap:body@parts` (all when absent), in message order.
    pub body_parts: Vec<Part>,
    /// One per `soap:header`, in binding order.
    pub header_parts: Vec<Part>,
    /// The RPC wrapper's namespace: `soap:body@namespace`, else the binding's targetNamespace.
    pub rpc_namespace: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub name: String,
    pub kind: PartKind,
}

#[derive(Debug, Clone)]
pub(crate) enum PartKind {
    Element(QName),
    Type(QName),
}

pub struct Definition {
    documents: Vec<Document>,
    operations: Vec<Operation>,
    schemas: SchemaSet,
}

impl Definition {
    /// Fetches the WSDL at `input` (URL or path) with everything it imports, then parses it.
    pub fn load(input: &str) -> Result<Definition, LoadError> {
        Self::parse(fetch_all(&to_url(input)?)?)
    }

    /// `documents[0]` is the root WSDL; the others are what it imports or includes.
    pub fn parse(documents: Vec<Document>) -> Result<Definition, LoadError> {
        let (operations, schemas) = {
            let parsed = parse_all(&documents)?;
            if !parsed.first().is_some_and(|x| is(x.root_element(), WSDL_NS, "definitions")) {
                return Err(LoadError::NotWsdl { url: documents.first().map_or(String::new(), |d| d.url.clone()) });
            }
            let definitions: Vec<Node> = parsed.iter().map(|x| x.root_element()).filter(|r| is(*r, WSDL_NS, "definitions")).collect();
            (collect_operations(&definitions), SchemaSet::from_parsed(&documents, &parsed))
        };
        Ok(Definition { documents, operations, schemas })
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    pub fn operation(&self, binding: &str, name: &str) -> Option<&Operation> {
        if let Some((namespace, local)) = binding.strip_prefix('{').and_then(|b| b.split_once('}')) {
            return self.operation_in(namespace, local, name);
        }
        self.operations.iter().find(|o| o.binding == binding && o.name == name)
    }

    pub fn operation_in(&self, namespace: &str, binding: &str, name: &str) -> Option<&Operation> {
        self.operations.iter().find(|o| o.binding_namespace == namespace && o.binding == binding && o.name == name)
    }

    /// The request SoapUI would generate for `op`: an envelope with `?` placeholders and guidance comments.
    pub fn sample_request(&self, op: &Operation) -> String {
        crate::envelope::sample_request(&self.schemas, op)
    }
}

fn is_soap(node: Node, name: &str) -> bool {
    is(node, SOAP11_NS, name) || is(node, SOAP12_NS, name)
}

fn wsdl_children<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| is(*n, WSDL_NS, name))
}

fn part(node: Node) -> Option<Part> {
    let kind = match node.attribute("element") {
        Some(element) => PartKind::Element(resolve(node, element)),
        None => PartKind::Type(resolve(node, node.attribute("type")?)),
    };
    Some(Part { name: node.attribute("name")?.to_string(), kind })
}

/// Bindings in document order (root first), operations in binding order. Dangling references
/// (unknown message, portType or part) leave the parts out instead of failing: real WSDLs are dirty.
fn collect_operations(definitions: &[Node]) -> Vec<Operation> {
    let named = |def: Node, node: Node| QName {
        ns: def.attribute("targetNamespace").unwrap_or("").to_string(),
        local: node.attribute("name").unwrap_or("").to_string(),
    };

    let mut messages: HashMap<QName, Vec<Part>> = HashMap::new();
    let mut port_types: HashMap<QName, HashMap<String, QName>> = HashMap::new();
    let mut endpoints: HashMap<QName, String> = HashMap::new();
    for &def in definitions {
        for message in wsdl_children(def, "message") {
            messages.entry(named(def, message)).or_insert_with(|| wsdl_children(message, "part").filter_map(part).collect());
        }
        for port_type in wsdl_children(def, "portType") {
            let inputs = port_types.entry(named(def, port_type)).or_default();
            for op in wsdl_children(port_type, "operation") {
                let input = wsdl_children(op, "input").next().and_then(|i| Some(resolve(i, i.attribute("message")?)));
                if let (Some(name), Some(input)) = (op.attribute("name"), input) {
                    inputs.entry(name.to_string()).or_insert(input);
                }
            }
        }
        for port in wsdl_children(def, "service").flat_map(|s| wsdl_children(s, "port")) {
            let address = port.children().find(|n| is_soap(*n, "address")).and_then(|a| a.attribute("location"));
            if let (Some(binding), Some(address)) = (port.attribute("binding"), address) {
                endpoints.entry(resolve(port, binding)).or_insert_with(|| address.to_string());
            }
        }
    }

    let mut operations = Vec::new();
    for &def in definitions {
        for binding in wsdl_children(def, "binding") {
            let Some(soap_binding) = binding.children().find(|n| is_soap(*n, "binding")) else { continue };
            let soap_version = if is(soap_binding, SOAP12_NS, "binding") { SoapVersion::V12 } else { SoapVersion::V11 };
            let port_type = binding.attribute("type").and_then(|t| port_types.get(&resolve(binding, t)));
            let binding_name = named(def, binding);
            let endpoint = endpoints.get(&binding_name).cloned();
            for op in wsdl_children(binding, "operation") {
                let Some(input) = wsdl_children(op, "input").next() else { continue };
                let name = op.attribute("name").unwrap_or("");
                let soap_op = op.children().find(|n| is_soap(*n, "operation"));
                let style = match soap_op.and_then(|o| o.attribute("style")).or(soap_binding.attribute("style")) {
                    Some("rpc") => Style::Rpc,
                    _ => Style::Document,
                };
                let body = input.children().find(|n| is_soap(*n, "body"));
                let all_parts = port_type.and_then(|p| p.get(name)).and_then(|m| messages.get(m)).map_or(&[][..], Vec::as_slice);
                let body_parts = match body.and_then(|b| b.attribute("parts")) {
                    Some(list) => {
                        let wanted: Vec<&str> = list.split_whitespace().collect();
                        all_parts.iter().filter(|p| wanted.contains(&p.name.as_str())).cloned().collect()
                    }
                    None => all_parts.to_vec(),
                };
                let header_parts = input
                    .children()
                    .filter(|n| is_soap(*n, "header"))
                    .filter_map(|h| {
                        let part = h.attribute("part")?;
                        messages.get(&resolve(h, h.attribute("message")?))?.iter().find(|p| p.name == part).cloned()
                    })
                    .collect();
                operations.push(Operation {
                    binding: binding.attribute("name").unwrap_or("").to_string(),
                    binding_namespace: binding_name.ns.clone(),
                    name: name.to_string(),
                    soap_version,
                    style,
                    encoded: body.and_then(|b| b.attribute("use")) == Some("encoded"),
                    soap_action: soap_op.and_then(|o| o.attribute("soapAction")).filter(|a| !a.is_empty()).map(str::to_string),
                    endpoint: endpoint.clone(),
                    input: InputSpec {
                        body_parts,
                        header_parts,
                        rpc_namespace: body.and_then(|b| b.attribute("namespace")).map_or_else(|| binding_name.ns.clone(), str::to_string),
                    },
                });
            }
        }
    }
    operations
}

#[cfg(test)]
mod tests {
    use super::Definition;

    fn load(p: &str) -> Definition {
        Definition::load(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    fn names(parts: &[super::Part]) -> Vec<&str> {
        parts.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn soap_body_parts_attribute_filters_body() {
        let d = load("features/service.wsdl");
        let op = d.operation("FeaturesSoap", "SaveEmployee").unwrap();
        assert_eq!(names(&op.input.body_parts), ["parameters"]);
        assert_eq!(names(&op.input.header_parts), ["auth"]);
    }

    #[test]
    fn rpc_body_keeps_message_order_and_namespace() {
        let d = load("rpc_encoded/service.wsdl");
        let op = d.operation("EncodedSoap", "CreateBooking").unwrap();
        assert_eq!(names(&op.input.body_parts), ["Credential", "Booking"]);
        assert_eq!(op.input.rpc_namespace, "urn:encoded");
    }
}
