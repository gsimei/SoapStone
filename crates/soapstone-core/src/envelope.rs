//! The SOAP envelope around a sample: Header and Body, namespace prefixes, and the text layout.

use crate::qname::QName;
use crate::sample::{AttrValue, Generator, XmlElement, XmlNode};
use crate::wsdl::{Operation, SoapVersion, Style};
use crate::xsd::{SOAP_ENC, SchemaSet, XS, XSI};

const SOAP11_ENV: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP12_ENV: &str = "http://www.w3.org/2003/05/soap-envelope";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Envelope → Header (always present; one sample per header part) → Body (one sample per body part;
/// RPC style wraps them in an element named after the operation).
pub(crate) fn sample_request(schemas: &SchemaSet, op: &Operation) -> String {
    let env = match op.soap_version {
        SoapVersion::V11 => SOAP11_ENV,
        SoapVersion::V12 => SOAP12_ENV,
    };
    let q = |ns: &str, local: &str| QName { ns: ns.to_string(), local: local.to_string() };
    let mut generator = Generator::new(schemas);
    let mut header = XmlElement::new(q(env, "Header"));
    header.children = op.input.header_parts.iter().map(|p| XmlNode::Element(generator.part(p))).collect();

    generator.encoded = op.encoded;
    let parts = op.input.body_parts.iter().map(|p| XmlNode::Element(generator.part(p))).collect();
    let mut body = XmlElement::new(q(env, "Body"));
    body.children = match op.style {
        Style::Document => parts,
        Style::Rpc => {
            let mut wrapper = XmlElement::new(q(&op.input.rpc_namespace, &op.name));
            if op.encoded {
                wrapper.attrs.push((q(env, "encodingStyle"), AttrValue::Text(SOAP_ENC.into())));
            }
            wrapper.children = parts;
            vec![XmlNode::Element(wrapper)]
        }
    };

    let mut envelope = XmlElement::new(q(env, "Envelope"));
    envelope.children = vec![XmlNode::Element(header), XmlNode::Element(body)];
    render(&envelope, if op.encoded { &[XSI, XS] } else { &[] })
}

/// SoapUI's prefix for a namespace: a well-known one, else a short word from the URI's last segment.
pub(crate) fn suggest_prefix(ns: &str) -> String {
    match ns {
        SOAP11_ENV => return "soapenv".into(),
        SOAP12_ENV => return "soap".into(),
        XS => return "xsd".into(),
        XSI => return "xsi".into(),
        SOAP_ENC => return "soapenc".into(),
        _ => {}
    }
    let segment = ns.strip_suffix('/').unwrap_or(ns).rsplit('/').next().unwrap_or("");
    let segment = segment.strip_prefix("www.").unwrap_or(segment);
    let word: String = segment
        .trim_start_matches(|c: char| !c.is_ascii_alphabetic())
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    match word.len() {
        0 => "ns".into(),
        1..=4 => word,
        _ => word[..3].to_string(),
    }
}

/// Prefixes in order of first use; a prefix already taken by another namespace gets 1, 2, ... appended.
#[derive(Default)]
struct Prefixes {
    declared: Vec<(String, String)>,
}

impl Prefixes {
    /// `prefix:local`, declaring the namespace on first use; no namespace → `local`.
    fn name(&mut self, q: &QName) -> String {
        match self.prefix(&q.ns) {
            Some(p) => format!("{p}:{}", q.local),
            None => q.local.clone(),
        }
    }

    fn prefix(&mut self, ns: &str) -> Option<String> {
        if ns.is_empty() {
            return None;
        }
        if ns == XML_NS {
            return Some("xml".into());
        }
        if let Some((_, p)) = self.declared.iter().find(|(n, _)| n == ns) {
            return Some(p.clone());
        }
        let base = suggest_prefix(ns);
        let mut prefix = base.clone();
        for n in 1.. {
            if !self.declared.iter().any(|(_, p)| *p == prefix) {
                break;
            }
            prefix = format!("{base}{n}");
        }
        self.declared.push((ns.to_string(), prefix.clone()));
        Some(prefix)
    }
}

/// 3-space indentation, leaf text inline, `<a/>` when empty, every namespace declared on the root
/// (`predeclared` first), no XML declaration and no trailing newline.
fn render(root: &XmlElement, predeclared: &[&str]) -> String {
    let mut prefixes = Prefixes::default();
    for ns in predeclared {
        prefixes.prefix(ns);
    }
    let root_name = prefixes.name(&root.name);
    let mut inner = String::new();
    for child in &root.children {
        write_node(child, 1, &mut prefixes, &mut inner);
    }
    let mut out = format!("<{root_name}");
    for (ns, prefix) in &prefixes.declared {
        out.push_str(&format!(" xmlns:{prefix}=\"{}\"", escape(ns)));
    }
    out.push_str(">\n");
    out.push_str(&inner);
    out.push_str(&format!("</{root_name}>"));
    out
}

fn write_node(node: &XmlNode, depth: usize, prefixes: &mut Prefixes, out: &mut String) {
    let indent = "   ".repeat(depth);
    match node {
        XmlNode::Comment(c) => out.push_str(&format!("{indent}<!--{c}-->\n")),
        XmlNode::Text(t) => out.push_str(&format!("{indent}{}\n", escape(t))),
        XmlNode::Element(e) => {
            let name = prefixes.name(&e.name);
            out.push_str(&format!("{indent}<{name}"));
            for (attr, value) in &e.attrs {
                let attr = prefixes.name(attr);
                let value = match value {
                    AttrValue::Text(t) => t.clone(),
                    AttrValue::QName(q, suffix) => format!("{}{suffix}", prefixes.name(q)),
                };
                out.push_str(&format!(" {attr}=\"{}\"", escape(&value)));
            }
            match e.children.as_slice() {
                [] => out.push_str("/>\n"),
                [XmlNode::Text(t)] => out.push_str(&format!(">{}</{name}>\n", escape(t))),
                children => {
                    out.push_str(">\n");
                    for child in children {
                        write_node(child, depth + 1, prefixes, out);
                    }
                    out.push_str(&format!("{indent}</{name}>\n"));
                }
            }
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn suggest_prefix_matches_soapui() {
        for (ns, p) in [
            ("http://tempuri.org/", "tem"),
            ("http://www2.optitravel.net/optitravel/export/wbs/opti_res/", "opti"),
            ("http://www.oorsprong.org/websamples.countryinfo", "web"),
            ("urn:common", "urn"),
            ("http://schemas.xmlsoap.org/soap/envelope/", "soapenv"),
            ("http://www.w3.org/2003/05/soap-envelope", "soap"),
            ("http://www.w3.org/2001/XMLSchema", "xsd"),
            ("http://www.w3.org/2001/XMLSchema-instance", "xsi"),
            ("http://schemas.xmlsoap.org/soap/encoding/", "soapenc"),
        ] {
            assert_eq!(super::suggest_prefix(ns), p, "{ns}");
        }
    }
}
