//! XML Schema: an owned model of the elements, types and groups a WSDL's messages refer to.

use std::collections::{HashMap, HashSet};

use roxmltree::Node;
use url::Url;

use crate::loader::{Document, WSDL_NS};
use crate::qname::{QName, is, resolve};

pub(crate) const XS: &str = "http://www.w3.org/2001/XMLSchema";
pub(crate) const SOAP_ENC: &str = "http://schemas.xmlsoap.org/soap/encoding/";
pub(crate) const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

#[derive(Debug, Default)]
pub(crate) struct SchemaSet {
    pub elements: HashMap<QName, Element>,
    pub types: HashMap<QName, TypeDef>,
    pub groups: HashMap<QName, Particle>,
    pub attribute_groups: HashMap<QName, Vec<AttrItem>>,
    pub attributes: HashMap<QName, Attribute>,
}

#[derive(Debug)]
pub(crate) struct Element {
    /// `name.ns` is "" for unqualified local elements (form / elementFormDefault).
    pub name: QName,
    pub ty: TypeRef,
    pub default: Option<String>,
}

#[derive(Debug)]
pub(crate) enum TypeRef {
    Named(QName),
    Inline(Box<TypeDef>),
    AnyType,
}

#[derive(Debug)]
pub(crate) enum TypeDef {
    Simple(SimpleType),
    Complex(ComplexType),
}

#[derive(Debug)]
pub(crate) struct SimpleType {
    /// Restriction base, list item type or first union member.
    pub base: Option<QName>,
}

// ponytail: `mixed` is not modelled; samples never put text between child elements anyway.
#[derive(Debug)]
pub(crate) struct ComplexType {
    pub derivation: Derivation,
    pub content: Content,
    pub attributes: Vec<AttrItem>,
}

#[derive(Debug)]
pub(crate) enum Derivation {
    None,
    Extension(QName),
    Restriction(QName),
}

/// `Simple` = simpleContent; its text type comes from `derivation`.
#[derive(Debug)]
pub(crate) enum Content {
    Empty,
    Simple,
    Particle(Particle),
}

#[derive(Debug)]
pub(crate) struct Particle {
    pub min: u32,
    pub max: Max,
    pub term: Term,
}

#[derive(Debug)]
pub(crate) enum Max {
    Bounded(u32),
    Unbounded,
}

#[derive(Debug)]
pub(crate) enum Term {
    Element(Element),
    ElementRef(QName),
    Sequence(Vec<Particle>),
    Choice(Vec<Particle>),
    All(Vec<Particle>),
    GroupRef(QName),
    Any,
}

#[derive(Debug)]
pub(crate) enum AttrItem {
    Attribute(Attribute),
    /// `wsdl:arrayType="tns:T[]"` → `array_type: Some((T, "[]"))`.
    Ref { name: QName, array_type: Option<(QName, String)> },
    GroupRef(QName),
}

#[derive(Debug)]
pub(crate) struct Attribute {
    pub name: QName,
    pub default: Option<String>,
}

impl SchemaSet {
    #[cfg(test)]
    pub fn build(documents: &[Document]) -> Result<SchemaSet, crate::LoadError> {
        Ok(Self::from_parsed(documents, &crate::loader::parse_all(documents)?))
    }

    /// Starts at each `xs:schema` inside `wsdl:types` and follows imports/includes among `documents`
    /// (never fetches). Unknown references stay as names: the sample builder handles the miss.
    pub(crate) fn from_parsed(documents: &[Document], parsed: &[roxmltree::Document]) -> SchemaSet {
        let mut builder = Builder {
            set: SchemaSet::default(),
            roots: documents.iter().zip(parsed).map(|(d, x)| (d.url.as_str(), x.root_element())).collect(),
            visited: HashSet::new(),
        };
        for (doc, xml) in documents.iter().zip(parsed) {
            let root = xml.root_element();
            if !is(root, WSDL_NS, "definitions") {
                continue;
            }
            let types = root.children().filter(|n| is(*n, WSDL_NS, "types"));
            for schema in types.flat_map(|t| t.children()).filter(|n| is(*n, XS, "schema")) {
                builder.schema(schema, &doc.url, None);
            }
            // `wsdl:import` may bring a plain XSD instead of another WSDL.
            for import in root.children().filter(|n| is(*n, WSDL_NS, "import")) {
                builder.follow(&doc.url, import.attribute("location"), None);
            }
        }
        builder.set
    }
}

struct Builder<'a, 'i> {
    set: SchemaSet,
    roots: HashMap<&'a str, Node<'a, 'i>>,
    visited: HashSet<(String, String)>,
}

impl<'a, 'i> Builder<'a, 'i> {
    /// `chameleon`: the includer's namespace, used when this schema has no targetNamespace.
    fn schema(&mut self, schema: Node<'a, 'i>, url: &str, chameleon: Option<&str>) {
        let own_tns = schema.attribute("targetNamespace");
        let ctx = Ctx {
            tns: own_tns.or(chameleon).unwrap_or("").to_string(),
            chameleon: own_tns.is_none() && chameleon.is_some(),
            elements_qualified: schema.attribute("elementFormDefault") == Some("qualified"),
            attributes_qualified: schema.attribute("attributeFormDefault") == Some("qualified"),
        };
        // Duplicate global definitions: first one wins (SoapUI is lenient too).
        for child in xs_children(schema) {
            let name = ctx.global(child);
            match child.tag_name().name() {
                "import" => self.follow(url, child.attribute("schemaLocation"), None),
                "include" => self.follow(url, child.attribute("schemaLocation"), Some(&ctx.tns)),
                "element" => {
                    self.set.elements.entry(name).or_insert_with(|| ctx.element(child, true));
                }
                "complexType" => {
                    self.set.types.entry(name).or_insert_with(|| TypeDef::Complex(ctx.complex(child)));
                }
                "simpleType" => {
                    self.set.types.entry(name).or_insert_with(|| TypeDef::Simple(ctx.simple(child)));
                }
                "group" => {
                    if let Some(p) = xs_children(child).find_map(|c| ctx.particle(c)) {
                        self.set.groups.entry(name).or_insert(p);
                    }
                }
                "attributeGroup" => {
                    self.set.attribute_groups.entry(name).or_insert_with(|| ctx.attr_items(child));
                }
                "attribute" => {
                    self.set.attributes.entry(name).or_insert_with(|| ctx.attribute(child, true));
                }
                _ => {}
            }
        }
    }

    fn follow(&mut self, base: &str, location: Option<&str>, chameleon: Option<&str>) {
        let Some(url) = location.and_then(|l| Url::parse(base).and_then(|b| b.join(l)).ok()).map(|u| u.to_string()) else { return };
        let Some(&root) = self.roots.get(url.as_str()) else { return };
        if is(root, XS, "schema") && self.visited.insert((url.clone(), chameleon.unwrap_or("").to_string())) {
            self.schema(root, &url, chameleon);
        }
    }
}

/// What a schema's declarations need to name things: its namespace and form defaults.
struct Ctx {
    tns: String,
    chameleon: bool,
    elements_qualified: bool,
    attributes_qualified: bool,
}

fn xs_children<'a, 'i>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(|n| n.is_element() && n.tag_name().namespace() == Some(XS))
}

fn occurs(value: Option<&str>) -> u32 {
    value.and_then(|v| v.trim().parse().ok()).unwrap_or(1)
}

fn default_value(node: Node) -> Option<String> {
    node.attribute("default").or(node.attribute("fixed")).map(str::to_string)
}

impl Ctx {
    /// A QName from an attribute value; in a chameleon schema, no-namespace names move to the includer's namespace.
    fn qname(&self, node: Node, value: &str) -> QName {
        let mut q = resolve(node, value);
        if self.chameleon && q.ns.is_empty() {
            q.ns = self.tns.clone();
        }
        q
    }

    fn global(&self, node: Node) -> QName {
        QName { ns: self.tns.clone(), local: node.attribute("name").unwrap_or("").to_string() }
    }

    fn local(&self, node: Node, qualified_by_default: bool) -> QName {
        let qualified = node.attribute("form").map_or(qualified_by_default, |f| f == "qualified");
        QName { ns: if qualified { self.tns.clone() } else { String::new() }, local: node.attribute("name").unwrap_or("").to_string() }
    }

    fn element(&self, node: Node, global: bool) -> Element {
        Element {
            name: if global { self.global(node) } else { self.local(node, self.elements_qualified) },
            ty: self.type_ref(node),
            default: default_value(node),
        }
    }

    fn attribute(&self, node: Node, global: bool) -> Attribute {
        Attribute {
            name: if global { self.global(node) } else { self.local(node, self.attributes_qualified) },
            default: default_value(node),
        }
    }

    fn type_ref(&self, node: Node) -> TypeRef {
        if let Some(ty) = node.attribute("type") {
            return TypeRef::Named(self.qname(node, ty));
        }
        for child in xs_children(node) {
            match child.tag_name().name() {
                "complexType" => return TypeRef::Inline(Box::new(TypeDef::Complex(self.complex(child)))),
                "simpleType" => return TypeRef::Inline(Box::new(TypeDef::Simple(self.simple(child)))),
                _ => {}
            }
        }
        TypeRef::AnyType
    }

    fn simple(&self, node: Node) -> SimpleType {
        let base = xs_children(node).find_map(|c| {
            let value = match c.tag_name().name() {
                "restriction" => c.attribute("base"),
                "list" => c.attribute("itemType"),
                "union" => c.attribute("memberTypes").and_then(|m| m.split_whitespace().next()),
                _ => None,
            };
            value.map(|v| self.qname(c, v))
        });
        SimpleType { base }
    }

    fn complex(&self, node: Node) -> ComplexType {
        let mut t = ComplexType { derivation: Derivation::None, content: Content::Empty, attributes: Vec::new() };
        // Particle and attributes live on the complexType itself, or on its extension/restriction.
        let mut body = node;
        for content in xs_children(node).filter(|c| matches!(c.tag_name().name(), "complexContent" | "simpleContent")) {
            let Some(d) = xs_children(content).find(|c| matches!(c.tag_name().name(), "extension" | "restriction")) else { continue };
            let base = self.qname(d, d.attribute("base").unwrap_or(""));
            t.derivation = if d.tag_name().name() == "extension" { Derivation::Extension(base) } else { Derivation::Restriction(base) };
            if content.tag_name().name() == "simpleContent" {
                t.content = Content::Simple;
            }
            body = d;
        }
        if !matches!(t.content, Content::Simple)
            && let Some(p) = xs_children(body).find_map(|c| self.particle(c))
        {
            t.content = Content::Particle(p);
        }
        t.attributes = self.attr_items(body);
        t
    }

    fn particle(&self, node: Node) -> Option<Particle> {
        let particles = |n: Node| xs_children(n).filter_map(|c| self.particle(c)).collect();
        let term = match node.tag_name().name() {
            "element" => match node.attribute("ref") {
                Some(r) => Term::ElementRef(self.qname(node, r)),
                None => Term::Element(self.element(node, false)),
            },
            "sequence" => Term::Sequence(particles(node)),
            "choice" => Term::Choice(particles(node)),
            "all" => Term::All(particles(node)),
            "group" => Term::GroupRef(self.qname(node, node.attribute("ref")?)),
            "any" => Term::Any,
            _ => return None,
        };
        let max = match node.attribute("maxOccurs") {
            Some("unbounded") => Max::Unbounded,
            m => Max::Bounded(occurs(m)),
        };
        Some(Particle { min: occurs(node.attribute("minOccurs")), max, term })
    }

    fn attr_items(&self, node: Node) -> Vec<AttrItem> {
        xs_children(node)
            .filter_map(|c| match c.tag_name().name() {
                "attribute" => Some(match c.attribute("ref") {
                    Some(r) => AttrItem::Ref {
                        name: self.qname(c, r),
                        array_type: c.attribute((WSDL_NS, "arrayType")).map(|v| {
                            let (ty, dims) = v.split_at(v.find('[').unwrap_or(v.len()));
                            (self.qname(c, ty), dims.to_string())
                        }),
                    },
                    None => AttrItem::Attribute(self.attribute(c, false)),
                }),
                "attributeGroup" => Some(AttrItem::GroupRef(self.qname(c, c.attribute("ref")?))),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Definition, loader::Document};

    fn q(ns: &str, local: &str) -> QName {
        QName { ns: ns.into(), local: local.into() }
    }
    fn docs(p: &str) -> Vec<Document> {
        Definition::load(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap().documents().to_vec()
    }
    fn complex(s: &SchemaSet, n: QName) -> &ComplexType {
        match s.types.get(&n) {
            Some(TypeDef::Complex(c)) => c,
            _ => panic!("no complex type {n:?}"),
        }
    }
    fn children(c: &ComplexType) -> Vec<&Particle> {
        match &c.content {
            Content::Particle(Particle { term: Term::Sequence(v), .. }) => v.iter().collect(),
            _ => vec![],
        }
    }
    fn elem(p: &Particle) -> &Element {
        match &p.term {
            Term::Element(e) => e,
            _ => panic!("not an element"),
        }
    }

    #[test]
    fn chameleon_include_takes_including_namespace() {
        let s = SchemaSet::build(&docs("features/service.wsdl")).unwrap();
        let addr = complex(&s, q("urn:common", "Address"));
        let country = elem(children(addr)[2]);
        assert!(matches!(&country.ty, TypeRef::Named(n) if *n == q("urn:common", "Country")));
        assert_eq!(elem(children(addr)[0]).name, q("", "street")); // chameleon file has no elementFormDefault
        assert!(s.types.contains_key(&q("urn:common", "Country")));
    }

    #[test]
    fn inline_schema_uses_prefixes_declared_on_wsdl_root() {
        let s = SchemaSet::build(&docs("features/service.wsdl")).unwrap();
        let get = &s.elements[&q("urn:features", "GetPerson")];
        let TypeRef::Inline(t) = &get.ty else { panic!("inline type expected") };
        let TypeDef::Complex(c) = t.as_ref() else { panic!() };
        let filter = elem(children(c)[1]);
        assert_eq!(filter.name, q("urn:features", "filter")); // elementFormDefault="qualified"
        assert!(matches!(&filter.ty, TypeRef::Named(n) if *n == q("urn:common", "Contact")));
        assert_eq!(children(c)[1].min, 0);
    }

    #[test]
    fn extension_records_its_base() {
        let s = SchemaSet::build(&docs("features/service.wsdl")).unwrap();
        assert!(matches!(&complex(&s, q("urn:common", "Employee")).derivation,
            Derivation::Extension(b) if *b == q("urn:common", "Person")));
    }

    #[test]
    fn soapenc_array_type_is_read() {
        let s = SchemaSet::build(&docs("rpc_encoded/service.wsdl")).unwrap();
        let tns = "urn:encoded";
        let arr = complex(&s, q(tns, "TravellerArray"));
        assert!(matches!(&arr.derivation, Derivation::Restriction(b) if *b == q(SOAP_ENC, "Array")));
        assert!(matches!(&arr.attributes[0],
            AttrItem::Ref { name, array_type: Some((t, dims)) }
            if *name == q(SOAP_ENC, "arrayType") && *t == q(tns, "Traveller") && dims == "[]"));
    }
}
