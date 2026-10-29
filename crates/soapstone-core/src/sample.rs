//! Sample content following SoapUI's rules: `?` placeholders, guidance comments, one occurrence
//! of optional items, and an empty element where a type would recurse.

use std::ptr;

use crate::qname::QName;
use crate::wsdl::{Part, PartKind};
use crate::xsd::{AttrItem, ComplexType, Content, Derivation, Element, Max, Particle, SOAP_ENC, SchemaSet, SimpleType, Term, TypeDef, TypeRef, XS, XSI};

#[derive(Debug)]
pub(crate) struct XmlElement {
    pub name: QName,
    pub attrs: Vec<(QName, AttrValue)>,
    pub children: Vec<XmlNode>,
}

#[derive(Debug)]
pub(crate) enum AttrValue {
    Text(String),
    /// `prefix:local` followed by a suffix (array dimensions); the prefix is chosen when written.
    QName(QName, String),
}

#[derive(Debug)]
pub(crate) enum XmlNode {
    Element(XmlElement),
    Text(String),
    Comment(String),
}

impl XmlElement {
    pub fn new(name: QName) -> XmlElement {
        XmlElement { name, attrs: Vec::new(), children: Vec::new() }
    }
}

// ponytail: depth cap stops self-referencing groups in broken schemas; deeper nesting is cut off.
const MAX_DEPTH: usize = 128;

enum Resolved<'s> {
    Text { binary: bool },
    Complex(&'s ComplexType),
}

/// One per request: the `cid:` counter starts at 1 for each sample.
pub(crate) struct Generator<'s> {
    schemas: &'s SchemaSet,
    /// Complex types being expanded; meeting one again yields an empty element.
    stack: Vec<*const ComplexType>,
    cid: u32,
    depth: usize,
    /// SOAP encoding (RPC/encoded): elements without namespace, `xsi:type` on named types, SOAP arrays.
    pub encoded: bool,
}

impl<'s> Generator<'s> {
    pub fn new(schemas: &'s SchemaSet) -> Generator<'s> {
        Generator { schemas, stack: Vec::new(), cid: 0, depth: 0, encoded: false }
    }

    /// A message part: the global element it names, or an unqualified element named after the part.
    pub fn part(&mut self, part: &Part) -> XmlElement {
        match &part.kind {
            PartKind::Element(q) => match self.schemas.elements.get(q) {
                Some(el) => self.element(el),
                None => self.build(q.clone(), None, Resolved::Text { binary: false }, None),
            },
            PartKind::Type(t) => {
                let resolved = self.resolve_named(t);
                self.build(QName { ns: String::new(), local: part.name.clone() }, Some(t), resolved, None)
            }
        }
    }

    fn element(&mut self, el: &'s Element) -> XmlElement {
        let (type_name, resolved) = match &el.ty {
            TypeRef::Named(q) => (Some(q), self.resolve_named(q)),
            TypeRef::Inline(def) => (None, self.resolve_def(def)),
            TypeRef::AnyType => (None, Resolved::Text { binary: false }),
        };
        self.build(el.name.clone(), type_name, resolved, el.default.as_deref())
    }

    /// `type_name`: the element's named type, written as `xsi:type` when encoded.
    fn build(&mut self, name: QName, type_name: Option<&QName>, resolved: Resolved<'s>, default: Option<&str>) -> XmlElement {
        let mut e = XmlElement::new(if self.encoded { QName { ns: String::new(), local: name.local } } else { name });
        if let (true, Some(t)) = (self.encoded, type_name) {
            e.attrs.push((QName { ns: XSI.into(), local: "type".into() }, AttrValue::QName(t.clone(), String::new())));
        }
        match resolved {
            Resolved::Text { binary } => e.children.push(XmlNode::Text(self.text(binary, default))),
            Resolved::Complex(ct) => {
                if self.stack.iter().any(|&p| ptr::eq(p, ct)) {
                    return e;
                }
                self.stack.push(ct);
                self.fill(ct, &mut e, default);
                self.stack.pop();
            }
        }
        e
    }

    fn text(&mut self, binary: bool, default: Option<&str>) -> String {
        match default {
            Some(d) => d.to_string(),
            None if binary => {
                self.cid += 1;
                format!("cid:{}", self.cid)
            }
            None => "?".to_string(),
        }
    }

    /// Unknown types (missing or never fetched) are treated as anyType: `?`.
    fn resolve_named(&self, q: &QName) -> Resolved<'s> {
        match self.schemas.types.get(q) {
            Some(def) => self.resolve_def(def),
            None => Resolved::Text { binary: self.is_binary(q) },
        }
    }

    fn resolve_def(&self, def: &'s TypeDef) -> Resolved<'s> {
        match def {
            TypeDef::Complex(c) => Resolved::Complex(c),
            TypeDef::Simple(s) => Resolved::Text { binary: s.base.as_ref().is_some_and(|b| self.is_binary(b)) },
        }
    }

    /// Does the type's base chain reach xs:base64Binary or xs:hexBinary?
    fn is_binary(&self, q: &QName) -> bool {
        let mut q = q;
        for _ in 0..MAX_DEPTH {
            if q.ns == XS {
                return matches!(q.local.as_str(), "base64Binary" | "hexBinary");
            }
            match self.schemas.types.get(q) {
                Some(TypeDef::Simple(SimpleType { base: Some(b), .. })) => q = b,
                _ => return false,
            }
        }
        false
    }

    /// Attributes, then content; an extension's base comes first.
    fn fill(&mut self, ct: &'s ComplexType, e: &mut XmlElement, default: Option<&str>) {
        let chain = self.chain(ct);
        for t in &chain {
            self.attributes(&t.attributes, e, 0);
        }
        if let Content::Simple = ct.content {
            let binary = match &chain[0].derivation {
                Derivation::Extension(b) | Derivation::Restriction(b) => self.is_binary(b),
                Derivation::None => false,
            };
            let text = self.text(binary, default);
            e.children.push(XmlNode::Text(text));
            return;
        }
        for t in chain {
            if let Content::Particle(p) = &t.content {
                self.particle(p, &mut e.children);
            }
        }
    }

    /// `ct` preceded by the types it extends, base-most first. A restriction keeps only its own model.
    fn chain(&self, ct: &'s ComplexType) -> Vec<&'s ComplexType> {
        let mut chain = vec![ct];
        while let Derivation::Extension(b) = &chain[chain.len() - 1].derivation {
            match self.schemas.types.get(b) {
                Some(TypeDef::Complex(base)) if !chain.iter().any(|c| ptr::eq(*c, base)) => chain.push(base),
                _ => break,
            }
        }
        chain.reverse();
        chain
    }

    /// Every attribute, with its default or `?`; references to unknown attributes (`xml:lang`) still appear.
    /// `soapenc:arrayType` is written only when the schema gives `wsdl:arrayType` (`urn:T[]`).
    fn attributes(&self, items: &'s [AttrItem], e: &mut XmlElement, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let text = |default: Option<&String>| AttrValue::Text(default.cloned().unwrap_or_else(|| "?".into()));
        for item in items {
            match item {
                AttrItem::Attribute(a) if !self.skipped(&a.name) => e.attrs.push((a.name.clone(), text(a.default.as_ref()))),
                AttrItem::Attribute(_) => {}
                AttrItem::Ref { name, array_type } if name.ns == SOAP_ENC && name.local == "arrayType" => {
                    if let Some((t, dims)) = array_type {
                        e.attrs.push((name.clone(), AttrValue::QName(t.clone(), dims.clone())));
                    }
                }
                AttrItem::Ref { name, .. } if !self.skipped(name) => {
                    let default = self.schemas.attributes.get(name).and_then(|a| a.default.as_ref());
                    e.attrs.push((name.clone(), text(default)));
                }
                AttrItem::Ref { .. } => {}
                AttrItem::GroupRef(g) => {
                    if let Some(group) = self.schemas.attribute_groups.get(g) {
                        self.attributes(group, e, depth + 1);
                    }
                }
            }
        }
    }

    /// SOAP encoding's own bookkeeping attributes never go in a sample.
    fn skipped(&self, name: &QName) -> bool {
        self.encoded && (name.ns.is_empty() || name.ns == SOAP_ENC) && matches!(name.local.as_str(), "href" | "id" | "offset")
    }

    fn particle(&mut self, p: &'s Particle, out: &mut Vec<XmlNode>) {
        if self.depth > MAX_DEPTH {
            return;
        }
        self.depth += 1;
        let count = match p.max {
            Max::Bounded(max) if max == p.min => p.min,
            _ => p.min.max(1),
        };
        match &p.term {
            Term::Element(el) => self.repeat_element(p, count, el, out),
            Term::ElementRef(q) => {
                if let Some(el) = self.schemas.elements.get(q) {
                    self.repeat_element(p, count, el, out);
                }
            }
            Term::Sequence(items) => {
                for _ in 0..count {
                    items.iter().for_each(|c| self.particle(c, out));
                }
            }
            Term::Choice(items) => {
                for _ in 0..count {
                    out.push(XmlNode::Comment(format!("You have a CHOICE of the next {} items at this level", items.len())));
                    items.iter().for_each(|c| self.particle(c, out));
                }
            }
            Term::All(items) => {
                out.push(XmlNode::Comment(format!("You may enter the following {} items in any order", items.len())));
                items.iter().for_each(|c| self.particle(c, out));
            }
            Term::GroupRef(q) => {
                if let Some(group) = self.schemas.groups.get(q) {
                    for _ in 0..count {
                        self.particle(group, out);
                    }
                }
            }
            Term::Any => out.push(XmlNode::Comment("You may enter ANY elements at this point".into())),
        }
        self.depth -= 1;
    }

    fn repeat_element(&mut self, p: &Particle, count: u32, el: &'s Element, out: &mut Vec<XmlNode>) {
        if let Some(comment) = occurrence_comment(p) {
            out.push(XmlNode::Comment(comment));
        }
        for _ in 0..count {
            out.push(XmlNode::Element(self.element(el)));
        }
    }
}

fn occurrence_comment(p: &Particle) -> Option<String> {
    match p.max {
        Max::Bounded(max) if max == p.min => None,
        Max::Bounded(1) => Some("Optional:".into()),
        Max::Unbounded if p.min == 0 => Some("Zero or more repetitions:".into()),
        Max::Unbounded => Some(format!("{} or more repetitions:", p.min)),
        Max::Bounded(max) => Some(format!("{} to {max} repetitions:", p.min)),
    }
}
