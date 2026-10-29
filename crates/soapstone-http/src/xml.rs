//! Response text: decoding, pretty printing, SOAP Fault detection.

use quick_xml::Reader;
use quick_xml::events::Event;

const SOAP11_ENV: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP12_ENV: &str = "http://www.w3.org/2003/05/soap-envelope";
const LATIN1: &[&str] = &["iso-8859-1", "latin1", "windows-1252"];

/// Response bytes → text. Charset from the Content-Type header, else from the XML declaration, else UTF-8 (lossy).
/// Strips a UTF-8 BOM.
pub fn decode_body(bytes: &[u8], content_type: Option<&str>) -> String {
    let charset = content_type.and_then(charset_param).or_else(|| declared_encoding(bytes));
    if charset.is_some_and(|c| LATIN1.iter().any(|l| c.eq_ignore_ascii_case(l))) {
        // ponytail: windows-1252 differs from latin-1 only in 0x80-0x9F; add encoding_rs if that ever matters.
        return bytes.iter().map(|&b| char::from(b)).collect();
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn charset_param(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        key.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches(['"', '\'']).to_string())
    })
}

/// `encoding="..."` of a leading `<?xml ...?>` declaration.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]);
    let decl = &head[..head.find("?>")?];
    if !decl.starts_with("<?xml") {
        return None;
    }
    let after = decl[decl.find("encoding")? + "encoding".len()..].trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let value = &after[1..];
    Some(value[..value.find(quote)?].to_string())
}

/// 3-space indented copy of an XML document; None when `text` is not well-formed XML.
/// Text runs (text, entity references, CDATA) stay on their element's line; whitespace-only runs are dropped.
pub fn pretty_xml(text: &str) -> Option<String> {
    if !text.trim_start().starts_with('<') {
        return None;
    }
    let mut reader = Reader::from_str(text);
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut pending = String::new();
    let mut depth = 0usize;
    // The innermost open element holds no child markup yet, so its end tag stays on the same line.
    let mut inline = false;
    loop {
        match reader.read_event().ok()? {
            Event::Text(t) => pending.push_str(&t),
            Event::GeneralRef(r) => {
                pending.push('&');
                pending.push_str(&r);
                pending.push(';');
            }
            Event::CData(c) => {
                pending.push_str("<![CDATA[");
                pending.push_str(&c);
                pending.push_str("]]>");
            }
            event => {
                if !pending.trim().is_empty() {
                    out.push_str(&pending);
                    inline = true;
                }
                pending.clear();
                match event {
                    Event::Eof => break,
                    Event::Start(e) => {
                        new_line(&mut out, depth);
                        push_all(&mut out, &["<", &e, ">"]);
                        depth += 1;
                        inline = true;
                    }
                    Event::End(e) => {
                        depth = depth.checked_sub(1)?;
                        if !inline {
                            new_line(&mut out, depth);
                        }
                        push_all(&mut out, &["</", &e, ">"]);
                        inline = false;
                    }
                    Event::Empty(e) => markup(&mut out, depth, &mut inline, &["<", &e, "/>"]),
                    Event::Comment(c) => markup(&mut out, depth, &mut inline, &["<!--", &c, "-->"]),
                    Event::Decl(d) => markup(&mut out, depth, &mut inline, &["<?", &d, "?>"]),
                    Event::PI(p) => markup(&mut out, depth, &mut inline, &["<?", &p, "?>"]),
                    Event::DocType(d) => markup(&mut out, depth, &mut inline, &["<!DOCTYPE ", &d, ">"]),
                    Event::Text(_) | Event::GeneralRef(_) | Event::CData(_) => unreachable!("handled above"),
                }
            }
        }
    }
    (depth == 0 && !out.is_empty()).then_some(out)
}

fn new_line(out: &mut String, depth: usize) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.extend(std::iter::repeat_n(' ', depth * 3));
}

fn push_all(out: &mut String, parts: &[&str]) {
    parts.iter().for_each(|p| out.push_str(p));
}

/// A self-contained piece of markup on its own line.
fn markup(out: &mut String, depth: usize, inline: &mut bool, parts: &[&str]) {
    new_line(out, depth);
    push_all(out, parts);
    *inline = false;
}

/// Text of a SOAP 1.1 `faultstring` or SOAP 1.2 `Reason/Text`; "SOAP Fault" when a Fault has neither;
/// None when `text` is not a SOAP Fault.
pub fn fault_string(text: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let fault = doc
        .descendants()
        .find(|n| n.tag_name().name() == "Fault" && matches!(n.tag_name().namespace(), Some(SOAP11_ENV | SOAP12_ENV)))?;
    let message = child(fault, "faultstring")
        .or_else(|| child(fault, "Reason").and_then(|r| child(r, "Text")))
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    Some(message.unwrap_or("SOAP Fault").to_string())
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, local: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|n| n.tag_name().name() == local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_body_uses_charset_header_then_xml_declaration() {
        assert_eq!(decode_body(b"Servi\xE7o", Some("text/xml; charset=ISO-8859-1")), "Serviço");
        assert_eq!(decode_body(b"Servi\xE7o", Some("text/xml;charset=\"latin1\"")), "Serviço");
        let declared = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xE7</a>";
        assert!(decode_body(declared, Some("text/xml")).ends_with("<a>ç</a>"));
        assert_eq!(decode_body(b"\xEF\xBB\xBF<a>\xC3\xA7</a>", None), "<a>ç</a>");
        assert_eq!(decode_body(b"<a>\xE7</a>", None), "<a>\u{FFFD}</a>");
    }

    #[test]
    fn pretty_xml_indents_three_spaces_and_keeps_text_inline() {
        assert_eq!(pretty_xml("<a><b>x</b><c/></a>").as_deref(), Some("<a>\n   <b>x</b>\n   <c/>\n</a>"));
    }

    #[test]
    fn pretty_xml_keeps_declaration_comments_and_cdata() {
        let p = pretty_xml("<?xml version=\"1.0\"?><a><!--note--><b><![CDATA[x<y]]></b></a>").unwrap();
        assert!(p.starts_with("<?xml version=\"1.0\"?>\n<a>"), "{p}");
        assert!(p.contains("<!--note-->") && p.contains("<![CDATA[x<y]]>"), "{p}");
    }

    #[test]
    fn pretty_xml_keeps_entities_and_empty_elements_on_one_line() {
        assert_eq!(pretty_xml("<a><b>Tom &amp; Jerry</b><c></c></a>").as_deref(), Some("<a>\n   <b>Tom &amp; Jerry</b>\n   <c></c>\n</a>"));
    }

    #[test]
    fn pretty_xml_returns_none_for_non_xml() {
        for t in ["", "   ", "Service Unavailable", "<html><body><p>oops</body></html>", "<a><b></a>", "<a>"] {
            assert_eq!(pretty_xml(t), None, "{t:?}");
        }
    }

    #[test]
    fn fault_string_reads_soap11_and_soap12() {
        let f11 = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring> Bad input </faultstring></s:Fault></s:Body></s:Envelope>"#;
        let f12 = r#"<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"><e:Body><e:Fault><e:Code><e:Value>e:Sender</e:Value></e:Code><e:Reason><e:Text xml:lang="en">Bad 1.2</e:Text></e:Reason></e:Fault></e:Body></e:Envelope>"#;
        let bare = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault/></s:Body></s:Envelope>"#;
        let ok = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><r/></s:Body></s:Envelope>"#;
        assert_eq!(fault_string(f11).as_deref(), Some("Bad input"));
        assert_eq!(fault_string(f12).as_deref(), Some("Bad 1.2"));
        assert_eq!(fault_string(bare).as_deref(), Some("SOAP Fault"));
        assert_eq!(fault_string(ok), None);
        assert_eq!(fault_string("<html>"), None);
    }
}
