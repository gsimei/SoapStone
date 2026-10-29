//! Repeatable offline release benchmark: a synthetic 5 MiB WSDL, 500 operations, 40 fields per operation.
use soapstone_core::{Definition, Document};
use soapstone_store::Store;
use std::time::{Duration, Instant};

fn documents() -> Vec<Document> {
    let mut schema = String::new();
    let mut messages = String::new();
    let mut port_type = String::new();
    let mut binding = String::new();
    for i in 0..500 {
        schema.push_str(&format!("<x:element name=\"Op{i}\"><x:complexType><x:sequence>"));
        for field in 0..40 {
            schema.push_str(&format!("<x:element name=\"field{field}\" type=\"x:string\" minOccurs=\"0\"/>"));
        }
        schema.push_str("</x:sequence></x:complexType></x:element>");
        messages.push_str(&format!("<w:message name=\"M{i}\"><w:part name=\"parameters\" element=\"t:Op{i}\"/></w:message>"));
        port_type.push_str(&format!("<w:operation name=\"Op{i}\"><w:input message=\"t:M{i}\"/></w:operation>"));
        binding.push_str(&format!("<w:operation name=\"Op{i}\"><s:operation soapAction=\"urn:bench:Op{i}\"/><w:input><s:body use=\"literal\"/></w:input></w:operation>"));
    }
    let mut xml = format!(
        "<w:definitions xmlns:w=\"http://schemas.xmlsoap.org/wsdl/\" xmlns:s=\"http://schemas.xmlsoap.org/wsdl/soap/\" xmlns:x=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"urn:bench\" targetNamespace=\"urn:bench\"><w:types><x:schema targetNamespace=\"urn:bench\" elementFormDefault=\"qualified\">{schema}</x:schema></w:types>{messages}<w:portType name=\"P\">{port_type}</w:portType><w:binding name=\"B\" type=\"t:P\"><s:binding style=\"document\" transport=\"http://schemas.xmlsoap.org/soap/http\"/>{binding}</w:binding>"
    );
    xml.push_str("<w:documentation>");
    xml.extend(std::iter::repeat_n('x', (5 * 1024 * 1024usize).saturating_sub(xml.len() + "</w:documentation></w:definitions>".len())));
    xml.push_str("</w:documentation></w:definitions>");
    vec![Document { url: "file:///benchmark.wsdl".into(), content: xml }]
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let docs = documents();
    let mut parse = Vec::new();
    let mut sample = Duration::ZERO;
    for _ in 0..5 {
        let input = docs.clone();
        let start = Instant::now();
        let def = Definition::parse(input).unwrap();
        parse.push(start.elapsed());
        assert_eq!(def.operations().len(), 500);
        for op in def.operations() {
            let start = Instant::now();
            std::hint::black_box(def.sample_request(op));
            sample = sample.max(start.elapsed());
        }
    }
    parse.sort();
    let mut store = Store::open_in_memory().unwrap();
    let start = Instant::now();
    store.import_service(1, docs.clone(), &Default::default()).unwrap();
    let import = start.elapsed();
    let operation = store.tree(1).unwrap()[0].operations[0].id;
    let mut create = Vec::new();
    let mut read = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let id = store.create_request(operation).unwrap();
        create.push(start.elapsed());
        let start = Instant::now();
        std::hint::black_box(store.request(id).unwrap());
        read.push(start.elapsed());
    }
    create.sort();
    read.sort();
    println!("WSDL: {} bytes, 500 operations, 40 fields per operation", docs[0].content.len());
    println!("Parse median / max (5 runs): {:.2} / {:.2} ms", ms(parse[2]), ms(parse[4]));
    println!("Sample max (2500 samples): {:.2} ms", ms(sample));
    println!("Import including stored samples: {:.2} ms", ms(import));
    println!("Create cached request median / max: {:.3} / {:.3} ms", ms(create[50]), ms(create[99]));
    println!("Read request median / max: {:.3} / {:.3} ms", ms(read[50]), ms(read[99]));
    println!("App startup, WebView rendering, network, and desktop memory are measured separately.");
    if check {
        assert!(parse[4] < Duration::from_secs(2), "WSDL parse exceeded 2 s");
        assert!(sample < Duration::from_millis(20), "Sample generation exceeded 20 ms");
        assert!(create[99] < Duration::from_millis(50), "Creating a cached request exceeded 50 ms");
        assert!(read[99] < Duration::from_millis(50), "Reading a request exceeded 50 ms");
    }
}
