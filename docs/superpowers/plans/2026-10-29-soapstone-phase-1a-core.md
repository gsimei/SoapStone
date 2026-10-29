# Soapstone Phase 1a — Core (WSDL → operations → sample XML) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust library, `soapstone-core`, that loads a WSDL from a URL or file (following every import/include), lists its SOAP operations, and generates for each one the same sample request XML SoapUI 5.9.1 generates.

**Architecture:** Three stages with owned data between them: `fetch_all` downloads every document reachable from the root WSDL; `Definition::parse` builds the operation list and an owned XSD model (`SchemaSet`) from those documents without touching the network (so the app can later reopen from its cache); `Definition::sample_request` walks the model and writes the envelope. Correctness is pinned by golden files produced by running the real SoapUI 5.9.1 installed on the developer's Mac.

**Tech Stack:** Rust (edition 2024), `roxmltree` (read XML), `url` (resolve relative locations), `reqwest` blocking (HTTP), `thiserror` (errors). No XSD crate: the resolver is ours (decision and reasons in the spec, section 6).

**Spec:** `docs/superpowers/specs/2026-10-29-cliente-soap-design.md` (sections 7.1–7.3 and 10 are the ones this plan implements).

## Global Constraints

- Crate `soapstone-core` at `crates/soapstone-core`, in a Cargo workspace at the repo root. `edition = "2024"`, `rust-version = "1.88"`, `license = "MIT"`.
- No code copied from SoapUI (EUPL 1.1). SoapUI is only run as a test oracle; it is never distributed.
- Code, identifiers, comments and commit messages in English.
- All network access lives in this crate (never in a WebView). HTTP timeout: 60 s.
- Sample output format (from SoapUI): no XML declaration; 3-space indentation; empty element as `<x/>`; all namespace declarations on the Envelope element; placeholder value `?`.
- Supported: WSDL 1.1 with SOAP 1.1 and SOAP 1.2 bindings; document and RPC style; literal and encoded use. Not supported in this phase: WSDL 2.0, HTTP/MIME bindings, attachments, `xs:redefine`, substitution groups.
- Performance targets (spec §5), release build, Apple Silicon: parse a ~5 MB / ~500-operation WSDL already downloaded in < 2 s; generate one sample in < 20 ms.

## Review Focus

Failure modes the spec implies that a person will hit; each has a test in the owning task:

1. **Service URL pasted without `?wsdl`** (the user's own URL is `.../wbs_server.php`): loading must retry with `?wsdl` instead of failing on the HTML page. → Task 3, `http_root_without_query_retries_with_wsdl`.
2. **WSDL served as ISO-8859-1 or with a UTF-8 BOM** (common on PHP/NuSOAP servers): must load, accented text intact. → Task 3, `decode_handles_bom_and_latin1`.
3. **Schemas that import each other in a cycle**: must terminate. → Task 3, `fetch_all_handles_import_cycles`.
4. **Dirty schema: reference to an undefined type or undeclared prefix**: must still generate the request, with `?` for that element, and never panic. → Task 6, `unresolved_type_still_generates`.
5. **A non-SOAP binding (HTTP GET/POST) next to the SOAP one**: must be ignored, not listed or crash. → Task 4, `features_ignores_http_binding_and_follows_wsdl_import`.

---

## File Structure

```
Cargo.toml                                  workspace
LICENSE                                     MIT
.gitignore
crates/soapstone-core/
  Cargo.toml
  src/lib.rs                                module wiring + public re-exports
  src/qname.rs                              QName + prefix resolution against a roxmltree node
  src/loader.rs                             Document, LoadError, to_url, fetch_all, decode
  src/wsdl.rs                               Definition, Operation, SoapVersion, Style (WSDL 1.1 parsing)
  src/xsd.rs                                SchemaSet: owned XSD model built from documents
  src/sample.rs                             XmlWriter + content generator (SoapUI rules)
  src/envelope.rs                           envelope assembly (document/RPC, literal/encoded), prefixes
  examples/soapstone-sample.rs              CLI: list operations, print a sample, time it
  tests/common/mod.rs                       assert_xml_equiv + tiny HTTP server for tests
  tests/xml_equiv.rs                        tests for the helper itself
  tests/loader.rs
  tests/operations.rs
  tests/golden.rs                           compares every expected/*.xml with our output
  tests/fixtures/calculator/calculator.wsdl         real (dneonline), doc/literal, SOAP 1.1 + 1.2
  tests/fixtures/optitravel/optitravel.wsdl         real (user's service), RPC/encoded
  tests/fixtures/features/...                        handwritten: imports, includes, XSD constructs
  tests/fixtures/rpc_literal/service.wsdl           handwritten: RPC/literal
  tests/fixtures/cycle/...  tests/fixtures/broken/...   loader edge cases
  tests/fixtures/*/expected/<Binding>__<Operation>.xml  generated by the SoapUI oracle
tools/soapui-oracle/oracle.groovy           runs SoapUI to produce expected/*.xml
tools/soapui-oracle/run.sh
```

---

### Task 1: Workspace, fixtures and the SoapUI oracle

**Files:**
- Create: `Cargo.toml`, `LICENSE`, `.gitignore`, `crates/soapstone-core/Cargo.toml`, `crates/soapstone-core/src/lib.rs`
- Create: `crates/soapstone-core/tests/fixtures/{calculator,optitravel,features,rpc_literal}/...`
- Create: `tools/soapui-oracle/oracle.groovy`, `tools/soapui-oracle/run.sh`
- Create (generated): `crates/soapstone-core/tests/fixtures/{calculator,optitravel,features,rpc_literal}/expected/*.xml`

**Interfaces:**
- Produces: fixture paths and expected-file naming `<BindingLocalName>__<OperationName>.xml`, used by Tasks 3–7.

- [ ] **Step 1: Create the repo and workspace**

`git init`. Root `Cargo.toml`: `[workspace] members = ["crates/soapstone-core"] resolver = "3"`. Crate deps: `roxmltree = "0.21"`, `url = "2"`, `reqwest = { version = "0.13", features = ["blocking"] }`, `thiserror = "2"`. `LICENSE`: MIT, "Copyright (c) 2026 Soapstone contributors". `.gitignore`: `/target`. `src/lib.rs`: a one-line crate doc comment.

Run: `cargo build`
Expected: `Finished` with no errors.

- [ ] **Step 2: Add the two real WSDLs**

```bash
curl -sSL "http://www.dneonline.com/calculator.asmx?WSDL" -o crates/soapstone-core/tests/fixtures/calculator/calculator.wsdl
curl -sSL "https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php?wsdl" -o crates/soapstone-core/tests/fixtures/optitravel/optitravel.wsdl
```

Expected: calculator ≈ 7.9 KB with bindings `CalculatorSoap` and `CalculatorSoap12`; optitravel ≈ 77 KB with binding `wbs_methodsSoap`.

- [ ] **Step 3: Write the handwritten `features` fixture**

Four files. Every construct listed is required; names are fixed because later tests use them.

`features/service.wsdl` — `targetNamespace="urn:features"`, `xmlns:tns="urn:features"`:
- `<wsdl:import namespace="urn:features" location="abstract.wsdl"/>`
- binding `FeaturesSoap` (SOAP 1.1, `style="document"`) for portType `tns:FeaturesPort`:
  - `GetPerson`: `soapAction="urn:features/GetPerson"`, input `soap:body use="literal"` plus `soap:header message="tns:AuthHeader" part="auth" use="literal"`.
  - `SaveEmployee`: `soapAction="urn:features/SaveEmployee"`, input `soap:body use="literal" parts="parameters"` plus `soap:header message="tns:SaveEmployeeIn" part="auth" use="literal"` (header part in the same message as the body part).
- binding `FeaturesHttp` (`http:binding verb="GET"`) for the same portType — must be ignored.
- service `Features`: port `FeaturesSoap` → `soap:address location="http://localhost:8080/features"`; port `FeaturesHttp` → `http:address`.

`features/abstract.wsdl` — `targetNamespace="urn:features"`; **`xmlns:tns="urn:features"` and `xmlns:c="urn:common"` declared only on `wsdl:definitions`**:
- `wsdl:types` → one `xs:schema targetNamespace="urn:features" elementFormDefault="qualified"` with **no** xmlns of its own, containing `<xs:import namespace="urn:common" schemaLocation="xsd/common.xsd"/>` and global elements:
  - `GetPerson`: sequence `id` (xs:int), `filter` (c:Contact, minOccurs 0)
  - `GetPersonResponse`: sequence `person` (c:Person)
  - `SaveEmployee`: sequence `employee` (c:Employee)
  - `Auth`: sequence `token` (xs:string)
- messages: `GetPersonIn` (part `parameters` element tns:GetPerson), `GetPersonOut` (part `parameters` element tns:GetPersonResponse), `AuthHeader` (part `auth` element tns:Auth), `SaveEmployeeIn` (parts `parameters` element tns:SaveEmployee and `auth` element tns:Auth), `SaveEmployeeOut` (part `parameters` element tns:GetPersonResponse).
- portType `FeaturesPort` with operations `GetPerson` and `SaveEmployee`.

`features/xsd/common.xsd` — `targetNamespace="urn:common"`, `xmlns:c="urn:common"`, `elementFormDefault="qualified"`:
- `<xs:include schemaLocation="parts/address.xsd"/>`
- `<xs:import namespace="http://www.w3.org/XML/1998/namespace"/>` (no schemaLocation)
- global element `note` (xs:string)
- group `Meta`: sequence `createdBy` (xs:string)
- attributeGroup `Audit`: attribute `version` (xs:int), `<xs:attribute ref="xml:lang"/>`
- simpleType `Code`: restriction of xs:string, enumerations `A`, `B`
- complexType `Person`: sequence `name` (xs:string); `age` (xs:int, minOccurs 0, default "30"); `nickname` (xs:string, minOccurs 0, maxOccurs unbounded); `address` (c:Address); `photo` (xs:base64Binary, minOccurs 0); `<xs:element ref="c:note" minOccurs="0"/>`; `<xs:group ref="c:Meta"/>`. Attributes: `id` (xs:string, use required), `<xs:attributeGroup ref="c:Audit"/>`.
- complexType `Contact`: choice `email` (xs:string), `phone` (xs:string)
- complexType `Flags`: all `a` (xs:boolean), `b` (xs:boolean)
- complexType `Extra`: sequence `<xs:any processContents="lax"/>`
- complexType `Employee`: complexContent extension base `c:Person`, sequence `employeeId` (xs:string); `code` (c:Code); `flags` (c:Flags, minOccurs 0); `extra` (c:Extra, minOccurs 0); `manager` (c:Employee, minOccurs 0) — recursive.

`features/xsd/parts/address.xsd` — **no targetNamespace, no elementFormDefault, no default xmlns** (chameleon):
- complexType `Address`: sequence `street` (xs:string), `city` (xs:string), `country` with **unprefixed** `type="Country"`
- simpleType `Country`: restriction of xs:string

- [ ] **Step 4: Write the `rpc_literal` fixture**

`rpc_literal/service.wsdl` — `targetNamespace="urn:rpc"`, inline schema (tns `urn:rpc`) with complexType `Item`: sequence `name` (xs:string). Message `EchoIn` with parts `text` (type xs:string), `count` (type xs:int), `item` (type tns:Item). Binding `RpcSoap` (SOAP 1.1, `style="rpc"`), operation `Echo`, `soapAction="urn:rpc/Echo"`, input `soap:body use="literal" namespace="urn:rpc"`. Port address `http://localhost:8080/rpc`.

- [ ] **Step 5: Write the oracle**

`tools/soapui-oracle/oracle.groovy` (exact):

```groovy
// Writes SoapUI's sample request for every operation of a WSDL to <wsdl dir>/expected/<Binding>__<Operation>.xml
import com.eviware.soapui.SoapUI
import com.eviware.soapui.StandaloneSoapUICore
import com.eviware.soapui.impl.wsdl.WsdlProject
import com.eviware.soapui.impl.wsdl.support.wsdl.WsdlImporter

def wsdl = new File(args[0]).canonicalFile
def out = new File(wsdl.parentFile, "expected")
out.mkdirs()
SoapUI.setSoapUICore(new StandaloneSoapUICore(true))
def project = new WsdlProject()
WsdlImporter.importWsdl(project, wsdl.toURI().toString()).each { iface ->
    iface.operationList.each { op ->
        new File(out, "${iface.name}__${op.name}.xml").setText(op.createRequest(true), "UTF-8")
    }
}
System.exit(0)
```

`tools/soapui-oracle/run.sh` (exact; `chmod +x`). A throwaway `user.home` keeps the developer's SoapUI settings out of the expected files:

```bash
#!/usr/bin/env bash
set -euo pipefail
APP="${SOAPUI_APP:-/Applications/SoapUI-5.9.1.app/Contents}"
TMP_HOME="$(mktemp -d)"
"$APP/PlugIns/jre.bundle/Contents/Home/bin/java" -Djava.awt.headless=true -Duser.home="$TMP_HOME" \
  -cp "$APP/java/app/bin/soapui-5.9.1.jar:$APP/java/app/lib/*" \
  groovy.ui.GroovyMain "$(dirname "$0")/oracle.groovy" "$1"
```

- [ ] **Step 6: Generate the expected files**

```bash
for f in calculator/calculator.wsdl optitravel/optitravel.wsdl features/service.wsdl rpc_literal/service.wsdl; do
  tools/soapui-oracle/run.sh crates/soapstone-core/tests/fixtures/$f
done
ls crates/soapstone-core/tests/fixtures/*/expected | grep -c '\.xml$'
```

Expected: `29` (calculator 8, optitravel 18, features 2, rpc_literal 1). Open `optitravel/expected/wbs_methodsSoap__AVITIK_GETGROUPRESERVES.xml` and check it has the shape of the user's real request (spec §7.2). If SoapUI rejects a handwritten fixture, fix the fixture, not the oracle.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: workspace, test fixtures and SoapUI oracle with expected samples"
```

---

### Task 2: XML equivalence helper for tests

**Files:**
- Create: `crates/soapstone-core/tests/common/mod.rs`
- Test: `crates/soapstone-core/tests/xml_equiv.rs`

**Interfaces:**
- Produces: `pub fn assert_xml_equiv(expected: &str, actual: &str)` (panics with both canonical forms on mismatch), used by Tasks 6–7.

- [ ] **Step 1: Write the failing tests**

```rust
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-core --test xml_equiv`
Expected: compile error, `assert_xml_equiv` not found.

- [ ] **Step 3: Implement `assert_xml_equiv` in `tests/common/mod.rs`**

Render each document to a canonical string with `roxmltree` (one line per node, indented by depth) and `assert_eq!` the two strings. Element: `{ns}local` followed by attributes sorted by expanded name, skipping `xmlns` declarations. Values of `{http://www.w3.org/2001/XMLSchema-instance}type` and `{http://schemas.xmlsoap.org/soap/encoding/}arrayType` are rewritten from `prefix:Local[...]` to `{ns}Local[...]` using the attribute's element for prefix lookup. Text: trimmed, skipped when empty, `cid:` followed by digits becomes `cid:*`. Comments: `<!--trimmed text-->`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-core --test xml_equiv`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/soapstone-core/tests
git commit -m "test: namespace-aware XML equivalence helper"
```

---

### Task 3: Loader — fetch every document reachable from the root

**Files:**
- Create: `crates/soapstone-core/src/qname.rs`, `crates/soapstone-core/src/loader.rs`
- Modify: `crates/soapstone-core/src/lib.rs`, `crates/soapstone-core/tests/common/mod.rs` (add test server)
- Create: `tests/fixtures/cycle/root.wsdl`, `tests/fixtures/cycle/a.xsd`, `tests/fixtures/cycle/b.xsd`, `tests/fixtures/broken/root.wsdl`
- Test: `crates/soapstone-core/tests/loader.rs`

**Interfaces:**
- Produces (`src/qname.rs`, crate-private):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub(crate) struct QName { pub ns: String, pub local: String }
  // "p:local" or "local" resolved against `node` (inherits ancestors' xmlns). Unknown prefix → ns "".
  pub(crate) fn resolve(node: roxmltree::Node, value: &str) -> QName
  ```
- Produces (`src/loader.rs`, re-exported from `lib.rs`):
  ```rust
  #[derive(Debug, Clone)]
  pub struct Document { pub url: String, pub content: String }

  #[derive(Debug, thiserror::Error)]
  pub enum LoadError {
      #[error("could not fetch {url}: {reason}")] Fetch { url: String, reason: String },
      #[error("invalid XML in {url}: {reason}")] Xml { url: String, reason: String },
      #[error("{url} is not a WSDL 1.1 document")] NotWsdl { url: String },
  }

  pub fn to_url(input: &str) -> Result<url::Url, LoadError>        // http(s)/file URL as is; anything else is a path → canonicalized file URL
  pub fn fetch_all(root: &url::Url) -> Result<Vec<Document>, LoadError> // root first, then discovery order, each URL once
  pub(crate) fn decode(bytes: &[u8], url: &str) -> Result<String, LoadError>
  pub(crate) const BUILTIN_NAMESPACES: &[&str]                      // imports of these are never fetched
  ```

- [ ] **Step 1: Write the edge-case fixtures**

- `cycle/root.wsdl`: minimal WSDL whose inline schema has `<xs:import namespace="urn:a" schemaLocation="a.xsd"/>`; no bindings needed.
- `cycle/a.xsd` (tns `urn:a`) imports `urn:b` from `b.xsd`; `cycle/b.xsd` (tns `urn:b`) imports `urn:a` from `a.xsd`.
- `broken/root.wsdl`: minimal WSDL whose inline schema imports `schemaLocation="missing.xsd"` (file does not exist).

- [ ] **Step 2: Write the failing tests**

```rust
mod common;
use soapstone_core::{fetch_all, to_url, LoadError};

fn fixture(p: &str) -> url::Url {
    to_url(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn urls(docs: &[soapstone_core::Document]) -> Vec<String> {
    docs.iter().map(|d| d.url.clone()).collect()
}

#[test]
fn fetch_all_follows_wsdl_import_xsd_import_and_include() {
    let docs = fetch_all(&fixture("features/service.wsdl")).unwrap();
    let u = urls(&docs);
    assert_eq!(u.len(), 4);
    assert!(u[0].ends_with("features/service.wsdl"));
    assert!(u[1].ends_with("features/abstract.wsdl"));
    assert!(u[2].ends_with("features/xsd/common.xsd"));
    assert!(u[3].ends_with("features/xsd/parts/address.xsd"));
}

#[test]
fn fetch_all_skips_builtin_namespaces() {
    // optitravel uses soapenc without any import; features imports the xml namespace without a location
    assert_eq!(fetch_all(&fixture("optitravel/optitravel.wsdl")).unwrap().len(), 1);
}

#[test]
fn fetch_all_handles_import_cycles() {
    assert_eq!(fetch_all(&fixture("cycle/root.wsdl")).unwrap().len(), 3);
}

#[test]
fn missing_import_is_a_fetch_error() {
    match fetch_all(&fixture("broken/root.wsdl")) {
        Err(LoadError::Fetch { url, .. }) => assert!(url.ends_with("broken/missing.xsd")),
        other => panic!("expected Fetch error, got {other:?}"),
    }
}

#[test]
fn http_root_without_query_retries_with_wsdl() {
    let wsdl = std::fs::read_to_string(format!("{}/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let port = common::serve(vec![("/svc", "<html><body>service page</body></html>".into()), ("/svc?wsdl", wsdl)]);
    let docs = fetch_all(&url::Url::parse(&format!("http://127.0.0.1:{port}/svc")).unwrap()).unwrap();
    assert!(docs[0].url.ends_with("/svc?wsdl"));
}
```

`decode` is crate-private, so its test is a unit test in `src/loader.rs`:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn decode_handles_bom_and_latin1() {
        assert_eq!(super::decode(b"\xEF\xBB\xBF<a/>", "x").unwrap(), "<a/>");
        let latin1 = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>Servi\xE7o</a>";
        assert!(super::decode(latin1, "x").unwrap().contains("Serviço"));
    }
}
```

Add `pub fn serve(routes: Vec<(&'static str, String)>) -> u16` to `tests/common/mod.rs`: bind `127.0.0.1:0`, spawn a thread that answers each connection by matching the request target (path + query) against `routes`, replies `200` with the body (or `404`), closes the connection; return the port. If binding fails with "Operation not permitted", the sandbox blocks local ports: enable `sandbox.network.allowLocalBinding`.

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p soapstone-core --test loader`
Expected: compile error, `fetch_all` / `to_url` not found.

- [ ] **Step 4: Implement `qname::resolve`, `to_url`, `decode`, `fetch_all`**

- `decode`: strip a UTF-8 BOM; if the XML declaration names `ISO-8859-1`/`latin1` (case-insensitive), map each byte to the `char` with that code point; otherwise UTF-8 (`Xml` error if invalid). Fetch bodies as bytes (`reqwest` `.bytes()`), never `.text()`.
- `fetch_all`: breadth-first queue starting at root; skip URLs already seen. For the root only: if its root element is not `{http://schemas.xmlsoap.org/wsdl/}definitions` and the URL has no query, retry once with query `wsdl`; if still not, `NotWsdl`. Discovery per document: `wsdl:import/@location`; for every `xs:schema` (top-level XSD or inside `wsdl:types`): `xs:import/@schemaLocation` (skipped when `@namespace` is in `BUILTIN_NAMESPACES` or no location) and `xs:include/@schemaLocation`. Relative locations: `base.join(location)`. HTTP: `reqwest::blocking::Client` with 60 s timeout; non-2xx → `Fetch`.
- `BUILTIN_NAMESPACES`: SOAP 1.1/1.2 envelope and encoding, `http://www.w3.org/XML/1998/namespace`, `http://www.w3.org/2001/XMLSchema`, xmime `2004/11` and `2005/05`, xop `http://www.w3.org/2004/08/xop/include`, swaref `http://ws-i.org/profiles/basic/1.1/xsd`.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p soapstone-core`
Expected: loader tests (5) + the `decode` unit test + xml_equiv tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): fetch WSDL and every imported/included document"
```

---

### Task 4: WSDL model — list SOAP operations

**Files:**
- Create: `crates/soapstone-core/src/wsdl.rs`
- Modify: `crates/soapstone-core/src/lib.rs`
- Test: `crates/soapstone-core/tests/operations.rs`

**Interfaces:**
- Consumes: `Document`, `LoadError`, `to_url`, `fetch_all`, `qname::{QName, resolve}` (Task 3).
- Produces (re-exported from `lib.rs`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SoapVersion { V11, V12 }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Style { Document, Rpc }

  #[derive(Debug, Clone)]
  pub struct Operation {
      pub binding: String,              // binding local name, e.g. "CalculatorSoap12"
      pub name: String,
      pub soap_version: SoapVersion,
      pub style: Style,                 // soap:operation@style, else soap:binding@style, else Document
      pub encoded: bool,                // input soap:body@use == "encoded"
      pub soap_action: Option<String>,  // soap:operation@soapAction, None when absent or empty
      pub endpoint: Option<String>,     // soap:address@location of the first port using this binding
      pub(crate) input: InputSpec,
  }
  #[derive(Debug, Clone)]
  pub(crate) struct InputSpec {
      pub body_parts: Vec<Part>,        // message parts filtered by soap:body@parts (all when absent), in message order
      pub header_parts: Vec<Part>,      // one per soap:header (message + part), in binding order
      pub rpc_namespace: Option<String>,// soap:body@namespace
  }
  #[derive(Debug, Clone)] pub(crate) struct Part { pub name: String, pub kind: PartKind }
  #[derive(Debug, Clone)] pub(crate) enum PartKind { Element(QName), Type(QName) }

  pub struct Definition { /* documents, operations; `schemas` added in Task 5 */ }
  impl Definition {
      pub fn parse(documents: Vec<Document>) -> Result<Definition, LoadError>  // documents[0] is the root WSDL
      pub fn load(input: &str) -> Result<Definition, LoadError>               // to_url + fetch_all + parse
      pub fn documents(&self) -> &[Document]
      pub fn operations(&self) -> &[Operation]
      pub fn operation(&self, binding: &str, name: &str) -> Option<&Operation>
  }
  ```

- [ ] **Step 1: Write the failing tests**

```rust
use soapstone_core::{Definition, SoapVersion, Style};

fn load(p: &str) -> Definition {
    Definition::load(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn ids(d: &Definition) -> Vec<(String, String)> {
    d.operations().iter().map(|o| (o.binding.clone(), o.name.clone())).collect()
}

#[test]
fn calculator_lists_both_bindings() {
    let d = load("calculator/calculator.wsdl");
    let expected: Vec<(String, String)> = ["CalculatorSoap", "CalculatorSoap12"]
        .iter()
        .flat_map(|b| ["Add", "Subtract", "Multiply", "Divide"].map(|o| (b.to_string(), o.to_string())))
        .collect();
    assert_eq!(ids(&d), expected);
    let add12 = d.operation("CalculatorSoap12", "Add").unwrap();
    assert_eq!(add12.soap_version, SoapVersion::V12);
    assert_eq!(add12.style, Style::Document);
    assert!(!add12.encoded);
    assert_eq!(add12.soap_action.as_deref(), Some("http://tempuri.org/Add"));
    assert_eq!(add12.endpoint.as_deref(), Some("http://www.dneonline.com/calculator.asmx"));
}

#[test]
fn optitravel_is_rpc_encoded() {
    let d = load("optitravel/optitravel.wsdl");
    assert_eq!(d.operations().len(), 18);
    for op in d.operations() {
        assert_eq!(op.binding, "wbs_methodsSoap");
        assert_eq!(op.style, Style::Rpc);
        assert!(op.encoded);
        assert_eq!(op.endpoint.as_deref(), Some("https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php"));
    }
    let op = d.operation("wbs_methodsSoap", "AVITIK_GETGROUPRESERVES").unwrap();
    assert_eq!(op.soap_action.as_deref(), Some("http://www2.optitravel.net/optitravel/export/wbs/opti_res/AVITIK_GETGROUPRESERVES"));
}

#[test]
fn features_ignores_http_binding_and_follows_wsdl_import() {
    let d = load("features/service.wsdl");
    assert_eq!(ids(&d), vec![
        ("FeaturesSoap".to_string(), "GetPerson".to_string()),
        ("FeaturesSoap".to_string(), "SaveEmployee".to_string()),
    ]);
    assert_eq!(d.operation("FeaturesSoap", "GetPerson").unwrap().endpoint.as_deref(), Some("http://localhost:8080/features"));
    assert_eq!(d.documents().len(), 4);
}

#[test]
fn parse_works_offline_from_documents() {
    let docs = load("features/service.wsdl").documents().to_vec();
    assert_eq!(Definition::parse(docs).unwrap().operations().len(), 2);
}
```

The `body_parts` / `header_parts` filtering is crate-private; add a unit test in `src/wsdl.rs`:

```rust
#[test]
fn soap_body_parts_attribute_filters_body() {
    let d = Definition::load(&format!("{}/tests/fixtures/features/service.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let op = d.operation("FeaturesSoap", "SaveEmployee").unwrap();
    assert_eq!(op.input.body_parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["parameters"]);
    assert_eq!(op.input.header_parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["auth"]);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-core --test operations`
Expected: compile error, `Definition` not found.

- [ ] **Step 3: Implement `wsdl.rs`**

Collect messages, portTypes, bindings and services from every document whose root is `wsdl:definitions`, keyed by QName in that document's `targetNamespace`. Bindings in document order (root first), operations in binding order. A binding counts only if it has a child `binding` in `http://schemas.xmlsoap.org/wsdl/soap/` (V11) or `http://schemas.xmlsoap.org/wsdl/soap12/` (V12). Skip binding operations with no `input`. Unknown message/part references are skipped, not errors (dirty WSDLs).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-core`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): parse WSDL 1.1 SOAP bindings into operations"
```

---

### Task 5: XSD model — owned `SchemaSet`

**Files:**
- Create: `crates/soapstone-core/src/xsd.rs`
- Modify: `crates/soapstone-core/src/wsdl.rs` (`Definition` gains `schemas: SchemaSet`, built in `parse`), `src/lib.rs`

**Interfaces:**
- Consumes: `Document`, `LoadError`, `QName`, `resolve`, `BUILTIN_NAMESPACES` (Task 3).
- Produces (crate-private; used by Tasks 6–7):
  ```rust
  pub(crate) const XS: &str = "http://www.w3.org/2001/XMLSchema";
  pub(crate) const SOAP_ENC: &str = "http://schemas.xmlsoap.org/soap/encoding/";

  #[derive(Default)]
  pub(crate) struct SchemaSet {
      pub elements: HashMap<QName, Element>,           // global elements
      pub types: HashMap<QName, TypeDef>,              // global named types
      pub groups: HashMap<QName, Particle>,            // xs:group
      pub attribute_groups: HashMap<QName, Vec<AttrItem>>,
      pub attributes: HashMap<QName, Attribute>,       // global attributes
  }
  pub(crate) struct Element { pub name: QName, pub ty: TypeRef, pub default: Option<String> }
      // name.ns is "" for unqualified local elements (form / elementFormDefault)
  pub(crate) enum TypeRef { Named(QName), Inline(Box<TypeDef>), AnyType }
  pub(crate) enum TypeDef { Simple(SimpleType), Complex(ComplexType) }
  pub(crate) struct SimpleType { pub name: Option<QName>, pub base: Option<QName> }   // restriction base or list itemType
  pub(crate) struct ComplexType {
      pub name: Option<QName>, pub mixed: bool,
      pub derivation: Derivation, pub content: Content, pub attributes: Vec<AttrItem>,
  }
  pub(crate) enum Derivation { None, Extension(QName), Restriction(QName) }
  pub(crate) enum Content { Empty, Simple, Particle(Particle) }   // Simple = simpleContent; text type comes from `derivation`
  pub(crate) struct Particle { pub min: u32, pub max: Max, pub term: Term }
  pub(crate) enum Max { Bounded(u32), Unbounded }
  pub(crate) enum Term { Element(Element), ElementRef(QName), Sequence(Vec<Particle>), Choice(Vec<Particle>), All(Vec<Particle>), GroupRef(QName), Any }
  pub(crate) enum AttrItem {
      Attribute(Attribute),
      Ref { name: QName, array_type: Option<(QName, String)> },   // wsdl:arrayType="tns:T[]" → (T, "[]")
      GroupRef(QName),
  }
  pub(crate) struct Attribute { pub name: QName, pub ty: TypeRef, pub default: Option<String> }

  impl SchemaSet {
      pub fn build(documents: &[Document]) -> Result<SchemaSet, LoadError>
  }
  ```

- [ ] **Step 1: Write the failing unit tests in `src/xsd.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{loader::Document, Definition};

    fn q(ns: &str, local: &str) -> QName { QName { ns: ns.into(), local: local.into() } }
    fn docs(p: &str) -> Vec<Document> {
        Definition::load(&format!("{}/tests/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap().documents().to_vec()
    }
    fn complex<'a>(s: &'a SchemaSet, n: QName) -> &'a ComplexType {
        match s.types.get(&n) { Some(TypeDef::Complex(c)) => c, _ => panic!("no complex type {n:?}") }
    }
    fn children(c: &ComplexType) -> Vec<&Particle> {
        match &c.content { Content::Particle(Particle { term: Term::Sequence(v), .. }) => v.iter().collect(), _ => vec![] }
    }
    fn elem(p: &Particle) -> &Element { match &p.term { Term::Element(e) => e, _ => panic!("not an element") } }

    #[test]
    fn chameleon_include_takes_including_namespace() {
        let s = SchemaSet::build(&docs("features/service.wsdl")).unwrap();
        let addr = complex(&s, q("urn:common", "Address"));
        let country = elem(children(addr)[2]);
        assert!(matches!(&country.ty, TypeRef::Named(n) if *n == q("urn:common", "Country")));
        assert_eq!(elem(children(addr)[0]).name, q("", "street"));        // chameleon file has no elementFormDefault
        assert!(s.types.contains_key(&q("urn:common", "Country")));
    }

    #[test]
    fn inline_schema_uses_prefixes_declared_on_wsdl_root() {
        let s = SchemaSet::build(&docs("features/service.wsdl")).unwrap();
        let get = &s.elements[&q("urn:features", "GetPerson")];
        let TypeRef::Inline(t) = &get.ty else { panic!("inline type expected") };
        let TypeDef::Complex(c) = t.as_ref() else { panic!() };
        let filter = elem(children(c)[1]);
        assert_eq!(filter.name, q("urn:features", "filter"));             // elementFormDefault="qualified"
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
        let s = SchemaSet::build(&docs("optitravel/optitravel.wsdl")).unwrap();
        let tns = "http://www2.optitravel.net/optitravel/export/wbs/opti_res/";
        let arr = complex(&s, q(tns, "TravellerArray"));
        assert!(matches!(&arr.derivation, Derivation::Restriction(b) if *b == q(SOAP_ENC, "Array")));
        assert!(matches!(&arr.attributes[0],
            AttrItem::Ref { name, array_type: Some((t, dims)) }
            if *name == q(SOAP_ENC, "arrayType") && *t == q(tns, "Traveller") && dims == "[]"));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-core --lib xsd`
Expected: compile error, `SchemaSet` not found.

- [ ] **Step 3: Implement `SchemaSet::build`**

Walk from every WSDL document: each `xs:schema` under `wsdl:types` (parsed in place, so prefixes declared on `wsdl:definitions` resolve through `qname::resolve`), then its `xs:import` / `xs:include` targets looked up by absolute URL among `documents` (never fetched here). Track visited `(url, chameleon_tns)`. A schema with no `targetNamespace` reached through `xs:include` uses the includer's namespace, and any QName in it that resolves to namespace `""` is moved to that namespace. Local element names get the target namespace only when `form="qualified"` or `elementFormDefault="qualified"`; attributes likewise with `attributeFormDefault`. `minOccurs`/`maxOccurs` default to 1. Duplicate global definitions: first one wins (SoapUI is lenient too). Nothing in this module returns an error for unknown references: they stay as names and the generator handles the miss.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-core`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): owned XSD model with imports, chameleon includes and SOAP arrays"
```

---

### Task 6: Sample request — document/literal

**Files:**
- Create: `crates/soapstone-core/src/sample.rs`, `crates/soapstone-core/src/envelope.rs`
- Modify: `crates/soapstone-core/src/wsdl.rs` (add `sample_request`), `src/lib.rs`
- Test: `crates/soapstone-core/tests/golden.rs`

**Interfaces:**
- Consumes: `Definition`, `Operation`, `InputSpec`, `Part`, `PartKind` (Task 4); `SchemaSet` and model types (Task 5); `assert_xml_equiv` (Task 2).
- Produces:
  ```rust
  impl Definition { pub fn sample_request(&self, op: &Operation) -> String }
  pub(crate) fn suggest_prefix(ns: &str) -> String                        // envelope.rs
  ```

- [ ] **Step 1: Write the failing tests**

`tests/golden.rs`:

```rust
mod common;
use soapstone_core::Definition;

fn check_fixture(dir: &str, root: &str) {
    let base = format!("{}/tests/fixtures/{dir}", env!("CARGO_MANIFEST_DIR"));
    let def = Definition::load(&format!("{base}/{root}")).unwrap();
    let mut n = 0;
    for entry in std::fs::read_dir(format!("{base}/expected")).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let (binding, op) = stem.split_once("__").unwrap();
        let op = def.operation(binding, op).unwrap_or_else(|| panic!("missing operation {stem}"));
        let expected = std::fs::read_to_string(&path).unwrap();
        println!("checking {stem}");
        common::assert_xml_equiv(&expected, &def.sample_request(op));
        n += 1;
    }
    assert!(n > 0);
}

#[test] fn calculator_matches_soapui() { check_fixture("calculator", "calculator.wsdl"); }
#[test] fn features_matches_soapui() { check_fixture("features", "service.wsdl"); }

#[test]
fn calculator_add_exact_text() {
    let def = Definition::load(&format!("{}/tests/fixtures/calculator/calculator.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let op = def.operation("CalculatorSoap", "Add").unwrap();
    assert_eq!(def.sample_request(op), concat!(
        "<soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:tem=\"http://tempuri.org/\">\n",
        "   <soapenv:Header/>\n",
        "   <soapenv:Body>\n",
        "      <tem:Add>\n",
        "         <tem:intA>?</tem:intA>\n",
        "         <tem:intB>?</tem:intB>\n",
        "      </tem:Add>\n",
        "   </soapenv:Body>\n",
        "</soapenv:Envelope>"));
}

#[test]
fn unresolved_type_still_generates() {
    let wsdl = r#"<wsdl:definitions xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/" xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/"
        xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:tns="urn:dirty" targetNamespace="urn:dirty">
      <wsdl:types><xs:schema targetNamespace="urn:dirty" elementFormDefault="qualified">
        <xs:element name="Op"><xs:complexType><xs:sequence>
          <xs:element name="a" type="tns:Missing"/>
          <xs:element name="b" type="nope:Undeclared"/>
          <xs:element ref="tns:alsoMissing"/>
        </xs:sequence></xs:complexType></xs:element>
      </xs:schema></wsdl:types>
      <wsdl:message name="In"><wsdl:part name="p" element="tns:Op"/></wsdl:message>
      <wsdl:portType name="P"><wsdl:operation name="Op"><wsdl:input message="tns:In"/></wsdl:operation></wsdl:portType>
      <wsdl:binding name="B" type="tns:P"><soap:binding style="document" transport="http://schemas.xmlsoap.org/soap/http"/>
        <wsdl:operation name="Op"><soap:operation soapAction="Op"/><wsdl:input><soap:body use="literal"/></wsdl:input></wsdl:operation>
      </wsdl:binding>
    </wsdl:definitions>"#;
    let def = Definition::parse(vec![soapstone_core::Document { url: "file:///dirty.wsdl".into(), content: wsdl.into() }]).unwrap();
    let xml = def.sample_request(def.operation("B", "Op").unwrap());
    assert!(xml.contains(":a>?</"), "{xml}");
    assert!(xml.contains(":b>?</"), "{xml}");
}
```

Unit test in `src/envelope.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-core --test golden`
Expected: compile error, `sample_request` not found.

- [ ] **Step 3: Implement `suggest_prefix` in `envelope.rs`**

Well-known namespaces map as in the test. Otherwise, take the last `/`-separated segment (ignoring one trailing `/`), skip a leading `www.`, skip to the first letter, then take characters while they are ASCII letters or digits. Longer than 4 → keep the first 3. Empty → `ns`. Lowercase. When building an envelope, a prefix already used for another namespace gets `1`, `2`, ... appended.

- [ ] **Step 4: Implement the generator in `sample.rs` and document-style assembly in `envelope.rs`**

`envelope.rs` builds: `Envelope` (SOAP 1.1 ns `http://schemas.xmlsoap.org/soap/envelope/`, SOAP 1.2 ns `http://www.w3.org/2003/05/soap-envelope`) → `Header` (always present; one sample per `header_parts` entry) → `Body` (one sample per `body_parts` entry). Part with `PartKind::Element(q)`: the global element `q`. Part with `PartKind::Type(t)`: an unqualified element named after the part, content from type `t`. Namespace declarations are collected while writing and emitted on the Envelope: `soapenv`/`soap` first, then the rest in order of first use.

`sample.rs` content rules (spec §7.2; the golden files are the authority when in doubt):
- Simple type or `AnyType`: text `?`. If the simple type's base chain reaches `xs:base64Binary` or `xs:hexBinary`: `cid:` + a counter starting at 1 per request. Unknown named type: treated as `AnyType`.
- Element with `default`: that text instead of `?`.
- Complex type: attributes first (all of them, with default or `?`; `AttrItem::GroupRef` expanded; `AttrItem::Ref` to an unknown attribute such as `xml:lang` still emitted with `?`), then content.
- `Derivation::Extension(b)`: base attributes and content first, then own. `Derivation::Restriction`: own content only. `Content::Simple`: text from the base chain.
- Particle count: `min` if `min == max`, else `max(min, 1)`. Comment once before an element particle with `min != max`: `Optional:` (max 1), `Zero or more repetitions:` (min 0, unbounded), `{min} or more repetitions:` (unbounded), `{min} to {max} repetitions:` (bounded > 1). No comment before group particles.
- `Choice` of n: comment `You have a CHOICE of the next {n} items at this level`, then every option. `All` of n: `You may enter the following {n} items in any order`, then every child. `Any`: comment `You may enter ANY elements at this point`, nothing else. `GroupRef`/`ElementRef`: expand; unknown → nothing.
- Recursion: keep a stack of the complex types being expanded; a type already on the stack produces an empty element.
- `XmlWriter`: 3-space indentation, comments on their own line, leaf text inline (`<a>?</a>`), empty element `<a/>`, escape `& < > "` in text and attributes, no trailing newline, no XML declaration.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p soapstone-core`
Expected: all tests pass, including `calculator_matches_soapui` (8 operations) and `features_matches_soapui` (2 operations). If a golden mismatch is a SoapUI behavior not described above, match SoapUI and add one line to the rules list in this plan.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): generate document/literal sample requests matching SoapUI"
```

---

### Task 7: Sample request — RPC literal and RPC/encoded

**Files:**
- Modify: `crates/soapstone-core/src/envelope.rs`, `crates/soapstone-core/src/sample.rs`
- Test: `crates/soapstone-core/tests/golden.rs`

**Interfaces:**
- Consumes: everything from Task 6.
- Produces: no new public API; `sample_request` now handles `Style::Rpc` and `encoded`.

- [ ] **Step 1: Write the failing tests (append to `tests/golden.rs`)**

```rust
#[test] fn rpc_literal_matches_soapui() { check_fixture("rpc_literal", "service.wsdl"); }
#[test] fn optitravel_matches_soapui() { check_fixture("optitravel", "optitravel.wsdl"); }

#[test]
fn optitravel_has_the_shape_of_the_users_request() {
    let def = Definition::load(&format!("{}/tests/fixtures/optitravel/optitravel.wsdl", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let xml = def.sample_request(def.operation("wbs_methodsSoap", "AVITIK_GETGROUPRESERVES").unwrap());
    for needle in [
        r#"xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance""#,
        r#"xmlns:xsd="http://www.w3.org/2001/XMLSchema""#,
        r#"<opti:AVITIK_GETGROUPRESERVES soapenv:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">"#,
        r#"<Credential xsi:type="opti:Credentials">"#,
        r#"<SystemID xsi:type="xsd:int">?</SystemID>"#,
        r#"<Username xsi:type="xsd:string">?</Username>"#,
        r#"<GroupReservesSearchParameters xsi:type="opti:GroupReservesSearchParameters">"#,
    ] {
        assert!(xml.contains(needle), "missing {needle} in\n{xml}");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-core --test golden`
Expected: the three new tests FAIL (RPC/encoded output not implemented yet).

- [ ] **Step 3: Implement RPC and encoded output**

- `Style::Rpc`: Body holds one wrapper element `{rpc_namespace or the binding's WSDL targetNamespace}{operation name}`; each body part becomes an **unqualified** element named after the part (element parts: the global element's qualified name), content from its type.
- `encoded`: declare `xsi` and `xsd` (in that order) before `soapenv` on the Envelope; wrapper gets `soapenv:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"`; every element whose type is a named type gets `xsi:type="{prefix}:{local}"` as its first attribute (built-ins with prefix `xsd`); every element inside the wrapper is written without namespace.
- SOAP arrays: `AttrItem::Ref` named `{soapenc}arrayType` with `array_type: Some((t, dims))` is written as `soapenc:arrayType="{prefix of t}:{t.local}{dims}"` (declaring `soapenc`); without `array_type` it is omitted. Attributes `href`, `id`, `offset` are skipped in encoded mode. A restriction of `soapenc:Array` with no own content is an empty element.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-core`
Expected: all tests pass; `optitravel_matches_soapui` covers all 18 operations.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): RPC literal and RPC/encoded sample requests (xsi:type, SOAP arrays)"
```

---

### Task 8: CLI example and performance check

**Files:**
- Create: `crates/soapstone-core/examples/soapstone-sample.rs`

**Interfaces:**
- Consumes: `to_url`, `fetch_all`, `Definition::{parse, operations, operation, sample_request}`.

- [ ] **Step 1: Implement the example**

Usage: `soapstone-sample <wsdl-url-or-path> [--op <Binding>/<Operation>] [--time]`. Plain `std::env::args` parsing.
- No `--op`: print one line per operation: `{binding}/{name}  SOAP {1.1|1.2}  {document|rpc}/{literal|encoded}  action={soap_action or -}  endpoint={endpoint or -}`.
- `--op`: print that operation's sample request to stdout (exit code 1 with a message if not found).
- `--time`: print to stderr `fetch: {ms} ms ({n} documents)`, `parse: {ms} ms`, `slowest sample: {ms} ms ({binding}/{name})`, generating every operation's sample to find the slowest.

- [ ] **Step 2: Run against the user's service**

Run: `cargo run -q -p soapstone-core --example soapstone-sample -- "https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php" --op wbs_methodsSoap/AVITIK_GETGROUPRESERVES`
Expected: a request with the same shape as the user's SoapUI request (spec §7.2), loaded from the URL **without** `?wsdl`.

- [ ] **Step 3: Check the performance targets**

```bash
curl -sSL https://developer.ebay.com/webservices/latest/ebaySvc.wsdl -o "$TMPDIR/big.wsdl"   # ≈ 5+ MB; any public WSDL ≥ 5 MB works
cargo run -q --release -p soapstone-core --example soapstone-sample -- "$TMPDIR/big.wsdl" --time > /dev/null
```

Expected: `parse` < 2000 ms and `slowest sample` < 20 ms. If not, profile before changing anything, and record the numbers in the commit message.

- [ ] **Step 4: Run the whole suite and lints**

Run: `cargo test -p soapstone-core && cargo clippy -p soapstone-core --all-targets -- -D warnings`
Expected: all tests pass, no clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): soapstone-sample CLI with timing"
```
