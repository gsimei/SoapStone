mod common;
use common::assert_xml_equiv;

#[test]
fn prefixes_and_declaration_placement_are_ignored() {
    assert_xml_equiv(
        r#"<a:E xmlns:a="urn:x" xmlns:b="urn:y"><b:F>?</b:F></a:E>"#,
        r#"<p:E xmlns:p="urn:x"><q:F xmlns:q="urn:y">?</q:F></p:E>"#,
    );
}

#[test]
fn qname_attribute_values_compare_by_namespace() {
    assert_xml_equiv(
        r#"<E xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:a="urn:t" xsi:type="a:T"/>"#,
        r#"<E xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:b="urn:t" xsi:type="b:T"/>"#,
    );
    assert_xml_equiv(
        r#"<E xmlns:enc="http://schemas.xmlsoap.org/soap/encoding/" xmlns:a="urn:t" enc:arrayType="a:T[]"/>"#,
        r#"<E xmlns:soapenc="http://schemas.xmlsoap.org/soap/encoding/" xmlns:b="urn:t" soapenc:arrayType="b:T[]"/>"#,
    );
}

#[test]
#[should_panic]
fn qname_attribute_in_other_namespace_differs() {
    assert_xml_equiv(
        r#"<E xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:a="urn:t" xsi:type="a:T"/>"#,
        r#"<E xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:a="urn:other" xsi:type="a:T"/>"#,
    );
}

#[test]
fn whitespace_between_elements_is_ignored() {
    assert_xml_equiv("<a>\n   <b>?</b>\n</a>", "<a><b>?</b></a>");
}

#[test]
fn cid_values_are_wildcards() {
    assert_xml_equiv("<a>cid:1234567</a>", "<a>cid:1</a>");
}

#[test]
#[should_panic]
fn comments_are_compared() {
    assert_xml_equiv("<a><!--Optional:--><b/></a>", "<a><!--Zero or more repetitions:--><b/></a>");
}
