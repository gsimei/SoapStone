//! Qualified names as written in WSDL/XSD attribute values (`tns:Foo`).

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct QName {
    pub ns: String,
    pub local: String,
}

/// Is `node` the element `{ns}name`?
pub(crate) fn is(node: roxmltree::Node, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == name
}

/// "p:local" or "local" resolved against `node` (inherits ancestors' xmlns). Unknown prefix → ns "".
pub(crate) fn resolve(node: roxmltree::Node, value: &str) -> QName {
    let value = value.trim();
    let (prefix, local) = value.split_once(':').map_or((None, value), |(p, l)| (Some(p), l));
    QName { ns: node.lookup_namespace_uri(prefix).unwrap_or("").to_string(), local: local.to_string() }
}
