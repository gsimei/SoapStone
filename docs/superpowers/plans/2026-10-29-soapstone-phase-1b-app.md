# Soapstone Phase 1b — Minimal App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri v2 desktop app where a user imports a WSDL (URL or file), browses its operations in a sidebar, edits and sends each operation's sample request with `{{variables}}` and Basic/Bearer/custom-header auth, and sees the response (status, time, size, headers, formatted XML, raw text, SOAP Fault), with projects and environments, everything saved in SQLite in the app's data directory.

**Architecture:** Three Rust libraries and a thin app. `soapstone-core` (Phase 1a, unchanged) turns WSDLs into operations and sample requests. New `soapstone-http` renders `{{variables}}` and sends one SOAP call with async `reqwest`, returning decoded body, pretty XML and fault text. New `soapstone-store` keeps projects, environments, imported services (with every downloaded document, so reopening needs no network), operations and saved requests in SQLite. The Tauri crate `app/src-tauri` only wires these into async commands; the React frontend in `app/src` calls them through `invoke` and never touches the network.

**Tech Stack:** Rust 2024; Tauri 2.12 + tauri-plugin-dialog 2.8; rusqlite 0.41 (`bundled`); reqwest 0.13 (async); quick-xml 0.42 (pretty print); roxmltree 0.21 (fault detection); serde / serde_json; tokio 1. React 19.3, TypeScript 7.0, Vite 8.3, CodeMirror 6 (`codemirror`, `@codemirror/lang-xml`, `@codemirror/theme-one-dark`), Vitest 5. npm.

**Spec:** `docs/superpowers/specs/2026-10-29-cliente-soap-design.md`. §9 row 1b is the scope; this plan implements §5 (performance targets), §6 (stack), §7.3 (HTTP), §8.1–8.4 (model, variables, send sequence, auth) and §8.6 (UI). Phase 1a plan: `docs/superpowers/plans/2026-10-29-soapstone-phase-1a-core.md`.

## Global Constraints

- Tauri **v2**: crate `tauri = "2.12"`, CLI `@tauri-apps/cli ^2.12.4` as an npm dev dependency (no global install). Tauri 3 is alpha and out of scope.
- Workspace: root `Cargo.toml` members `["crates/*", "app/src-tauri"]`. New crates: `edition = "2024"`, `rust-version = "1.88"`, `license = "MIT"`.
- **Do not edit `crates/soapstone-core`.** Its Phase 1a review runs on `feat/phase-1a-core`; when the user commits fixes there, run `git merge feat/phase-1a-core` on this branch. Core API used: `soapstone_core::{to_url, fetch_all, Document, LoadError, Definition, SoapVersion}` and `Definition::{parse, operations, operation, sample_request}`.
- All network access happens in Rust, never in the WebView (spec §6). `fetch_all` uses blocking `reqwest`: call it only inside `tauri::async_runtime::spawn_blocking` (or `tokio::task::spawn_blocking` in tests). Every Tauri command is an `async fn`: Tauri runs sync commands on the main thread, which would freeze the window while the store lock is held.
- Data: one SQLite file `soapstone.db` in Tauri's `app_data_dir()`. App identifier `io.github.gsimei.soapstone` (user, 2026-10-29), so macOS data lives in `~/Library/Application Support/io.github.gsimei.soapstone/`. Nothing is written to project folders.
- HTTP (spec §7.3, §8.4): method POST. SOAP 1.1: `Content-Type: text/xml;charset=UTF-8` and `SOAPAction: "<soapAction>"`. SOAP 1.2: `Content-Type: application/soap+xml;charset=UTF-8;action="<soapAction>"`. Timeout 60 s. Proxies: `reqwest` defaults (system proxy, `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`). Basic is preemptive. Bearer is `Authorization: Bearer <token>`.
- Variables (spec §8.2): `{{name}}`, spaces allowed inside the braces, name `[A-Za-z_][A-Za-z0-9_.-]*`, in endpoint, header names and values, body and auth fields. Lookup: active environment first, then the project's base variables. Any undefined name → error `Undefined variables: a, b`, and nothing is sent.
- Only the last response of each request is stored (spec §8.1).
- UI text in English only. Code, comments and commit messages in English. Theme follows the system (light/dark).
- Response content is never rendered as HTML: it goes only into CodeMirror or React text nodes (no `dangerouslySetInnerHTML`).
- Every text `<input>` has `spellCheck={false} autoCorrect="off" autoCapitalize="off" autoComplete="off"`, so macOS WebKit does not "fix" URLs and header values.
- npm is the package manager; `app/package-lock.json` is committed. Run `npm --prefix app run build` before any cargo command that compiles the app crate: `tauri::generate_context!` needs `app/dist`.
- Commits are SSH-signed by the user and hang without a TTY. "Stage" steps run `git add -A`; the user commits. Never bypass signing.
- `crates/soapstone-core/tests/fixtures/optitravel/` is local-only and must never be committed. Tests that need the network are `#[ignore]`.
- Performance targets (spec §5, release build, Apple Silicon): app open to usable screen < 1 s; switching request in the sidebar < 50 ms; processing a big already-downloaded WSDL (~5 MB, ~500 operations) < 2 s; one sample < 20 ms.

## Scope Decisions

User (2026-10-29), from the UI questions:
- Request editor and response **side by side**, with a draggable divider (also between sidebar and main area).
- A **project switcher** at the top of the sidebar; the tree below shows the current project's Services → Operations → Requests.
- An operation with exactly **one request is a single row** that opens it. Child rows appear only from two requests on. Clicking an operation with no request creates one from the sample.

Mine, reviewable:
- In 1b: everything in §9 row 1b, plus "Refresh WSDL" (§8.1), `Cmd/Ctrl+Enter` to send and `Cmd/Ctrl+P` quick open (§8.6), a Cancel button for a running send (§8.4).
- Moved to 1c with the TLS work: the per-request timeout field and "ignore TLS certificate errors" (for sends and WSDL downloads), plus WSDL URLs that need authentication. 1b uses a fixed 60 s timeout and normal certificate checks. Secret variables are already 1c.
- Not in 1b: request tabs (one request open at a time), duplicate request, response history, nested variables (`{{a}}` inside a variable value is sent as is).
- Variable values are substituted raw, without XML escaping, like SoapUI: a `&` in a password is written `&amp;` in the variable. This keeps imported SoapUI projects (1d) working unchanged.
- Edits autosave 500 ms after the last change; there is no Save button.
- A failed send (network error, cancel, undefined variable) keeps the previously saved response.
- SOAP 1.1 without a soapAction sends `SOAPAction: ""`. SOAP 1.2 without one omits `action=`.
- A user header with the same name as an automatic one (`Content-Type`, `SOAPAction`, `Authorization`) replaces it (case-insensitive).
- Auth password fields are plain text inputs (values are usually `{{variables}}`); masking arrives with secret variables in 1c.
- New services are named after the WSDL's first `wsdl:service/@name`, else the root URL's last path segment without extension; the user can rename them. A new store starts with one project, "My Project".
- Frontend tests: Vitest for the pure functions in `logic.ts` only; UI behavior is checked by running the app against the checklist in each UI task.

## Review Focus

Failure modes the spec implies that a person will hit; each has a test or check in the owning task:

1. **Server unreachable (DNS, refused connection, TLS failure, timeout)**: a readable message in the response pane, the app keeps working, and the request's previous saved response is kept. → Task 3 `connection_refused_is_a_readable_error`; Task 9 checklist item 4.
2. **Response in ISO-8859-1** (PHP/NuSOAP servers like the user's): accents intact in Body, Raw and the fault text. → Task 2 `decode_body_uses_charset_header_then_xml_declaration`; Task 3 `soap_fault_with_latin1_body_is_decoded_and_detected`.
3. **Response that is not XML** (HTML error page from a proxy, empty 500): shown raw with its status, never a blank pane or a crash. → Task 2 `pretty_xml_returns_none_for_non_xml`; Task 3 `html_error_page_is_kept_raw`.
4. **Undefined variable outside the body** (endpoint, header, auth field): nothing is sent and the error names every missing variable. → Task 3 `missing_variables_block_the_request`.
5. **Refresh WSDL after an operation disappeared or came back**: saved requests are untouched, vanished operations are marked, nothing is duplicated. → Task 5 `refresh_marks_missing_operations_orphan_and_keeps_requests`.

---

## File Structure

```
Cargo.toml                          workspace: members ["crates/*", "app/src-tauri"]
.gitignore                          + /app/node_modules, /app/dist, /app/src-tauri/gen/schemas
README.md                           what it is, status, development commands
crates/soapstone-core/              Phase 1a, untouched
crates/soapstone-http/
  Cargo.toml
  src/lib.rs                        Header, Auth, SoapCall, HttpResponse, SendError; re-exports
  src/vars.rs                       render(): {{variable}} substitution
  src/xml.rs                        decode_body, pretty_xml, fault_string
  src/send.rs                       client(), send()
  tests/send.rs                     against a one-shot local HTTP server
crates/soapstone-store/
  Cargo.toml
  src/lib.rs                        Store, schema, errors, projects, environments, rename/delete
  src/services.rs                   import/refresh, tree, requests, prepare_send, save_response
  tests/store.rs                    projects and environments
  tests/services.rs                 services, operations, requests (Phase 1a fixtures)
  tests/live.rs                     #[ignore] end-to-end against the user's service
app/
  package.json  package-lock.json  index.html  vite.config.ts  tsconfig.json
  src/main.tsx                      React root
  src/App.tsx                       layout, current project/request, global shortcuts
  src/api.ts                        TS types mirroring the Rust serde output + invoke wrappers
  src/logic.ts                      pure helpers: variables, search, quick open, sizes
  src/logic.test.ts                 Vitest
  src/Split.tsx                     two panes with a draggable divider
  src/Sidebar.tsx                   project switcher, search, tree, row menus, inline rename
  src/ImportDialog.tsx              WSDL URL or file
  src/XmlEditor.tsx                 CodeMirror 6 wrapper (XML, theme, {{var}} marks, read-only)
  src/KeyValueTable.tsx             name/value rows (headers, variables)
  src/RequestView.tsx               request editing, autosave, send/cancel, request pane UI
  src/ResponsePane.tsx              status line, Body / Headers / Raw
  src/EnvironmentDialog.tsx         base variables + environments
  src/QuickOpen.tsx                 Cmd/Ctrl+P
  src/styles.css                    light/dark tokens, layout
  src-tauri/
    Cargo.toml  build.rs  tauri.conf.json  app-icon.svg
    capabilities/default.json
    icons/                          generated by `tauri icon`
    src/main.rs                     state, setup, run
    src/commands.rs                 #[tauri::command]s
```

---

### Task 1: `soapstone-http` — shared types and `{{variable}}` rendering

**Files:**
- Modify: `Cargo.toml` (members `["crates/*"]`; `app/src-tauri` is added in Task 6 once it exists)
- Create: `crates/soapstone-http/Cargo.toml`, `crates/soapstone-http/src/lib.rs`, `crates/soapstone-http/src/vars.rs`

**Interfaces:**
- Consumes: `soapstone_core::SoapVersion` (`#[derive(Debug, Clone, Copy, PartialEq, Eq)] enum { V11, V12 }`).
- Produces (`lib.rs`), used by Tasks 2–6:
  ```rust
  pub use soapstone_core::SoapVersion;
  pub use vars::render;

  #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct Header { pub name: String, pub value: String }

  // JSON: {"type":"none"} | {"type":"basic","username":..,"password":..} | {"type":"bearer","token":..}
  #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
  #[serde(tag = "type", rename_all = "camelCase")]
  pub enum Auth { #[default] None, Basic { username: String, password: String }, Bearer { token: String } }

  /// Everything needed to send one request, before variables are rendered.
  #[derive(Debug, Clone, PartialEq)]
  pub struct SoapCall {
      pub endpoint: String,
      pub soap_version: SoapVersion,
      pub soap_action: Option<String>,
      pub headers: Vec<Header>,
      pub auth: Auth,
      pub body: String,
  }

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct HttpResponse {
      pub status: u16,
      pub status_text: String,      // canonical reason phrase, "" when unknown
      pub time_ms: u64,             // send start → whole body read
      pub size: u64,                // body bytes
      pub headers: Vec<Header>,     // in received order, one entry per value
      pub body: String,             // decoded text
      pub pretty: Option<String>,   // formatted XML, None when the body is not XML
      pub fault: Option<String>,    // SOAP Fault text when the body is a fault
  }

  #[derive(Debug, Clone, PartialEq, thiserror::Error)]
  pub enum SendError {
      #[error("Undefined variables: {}", .0.join(", "))] MissingVariables(Vec<String>),
      #[error("Invalid endpoint URL: \"{0}\"")] InvalidUrl(String),
      #[error("{0}")] Http(String),
      #[error("Request cancelled")] Cancelled,
  }
  ```
- Produces (`vars.rs`):
  ```rust
  /// Replaces every `{{name}}` with its value. Err lists each undefined name once, in order of first appearance.
  pub fn render(template: &str, vars: &HashMap<String, String>) -> Result<String, Vec<String>>
  ```

- [ ] **Step 1: Create the crate**

`crates/soapstone-http/Cargo.toml`: package `soapstone-http`, description "Sends SOAP requests for the Soapstone client". Dependencies: `soapstone-core = { path = "../soapstone-core" }`, `reqwest = "0.13"`, `serde = { version = "1", features = ["derive"] }`, `thiserror = "2"`. Dev-dependencies: `serde_json = "1"`. Root `Cargo.toml`: `members = ["crates/*"]`. `lib.rs` gets a one-line crate doc comment, `mod vars;`, and the types above.

- [ ] **Step 2: Write the failing tests**

Unit tests at the bottom of `src/vars.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::render;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn replaces_variables_with_optional_spaces() {
        let v = vars(&[("a", "1"), ("b", "2"), ("c.d-e_f", "3")]);
        assert_eq!(render("{{a}}-{{ b }}-{{c.d-e_f}}", &v), Ok("1-2-3".to_string()));
    }

    #[test]
    fn reports_each_missing_name_once_in_order() {
        assert_eq!(render("{{x}} {{y}} {{x}}", &vars(&[])), Err(vec!["x".to_string(), "y".to_string()]));
    }

    #[test]
    fn leaves_text_that_is_not_a_variable() {
        let t = "{{}} {{a b}} {{1a}} {{ a {{ }} {";
        assert_eq!(render(t, &vars(&[])), Ok(t.to_string()));
    }

    #[test]
    fn values_are_not_expanded_again() {
        assert_eq!(render("{{a}}", &vars(&[("a", "{{b}}")])), Ok("{{b}}".to_string()));
    }

    #[test]
    fn handles_multibyte_text_and_adjacent_variables() {
        assert_eq!(render("é{{a}}{{a}}ç", &vars(&[("a", "ü")])), Ok("éüüç".to_string()));
    }
}
```

Unit test at the bottom of `src/lib.rs` (the TypeScript side depends on this JSON):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shapes_match_the_frontend() {
        assert_eq!(serde_json::to_string(&Auth::None).unwrap(), r#"{"type":"none"}"#);
        let basic = Auth::Basic { username: "u".into(), password: "p".into() };
        assert_eq!(serde_json::to_string(&basic).unwrap(), r#"{"type":"basic","username":"u","password":"p"}"#);
        let r = HttpResponse { status: 200, status_text: "OK".into(), time_ms: 1, size: 2, headers: vec![], body: "<a/>".into(), pretty: None, fault: None };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!((json["statusText"].as_str(), json["timeMs"].as_u64()), (Some("OK"), Some(1)));
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p soapstone-http`
Expected: compile error, `render` not found.

- [ ] **Step 4: Implement `render` in `src/vars.rs`**

A hand-written scanner, no regex crate: find `{{`, skip spaces, read a name matching `[A-Za-z_][A-Za-z0-9_.-]*`, skip spaces, require `}}`. On a match, emit the value or record the missing name. Otherwise emit `{{` literally and continue scanning right after it. Values are inserted as they are, never scanned again. Slice only on char boundaries.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p soapstone-http`
Expected: 6 passed.

- [ ] **Step 6: Stage**

```bash
git add -A
```

---

### Task 2: `soapstone-http` — response text: decoding, pretty XML, SOAP Fault

**Files:**
- Create: `crates/soapstone-http/src/xml.rs`
- Modify: `crates/soapstone-http/Cargo.toml` (add `quick-xml = "0.42"`, `roxmltree = "0.21"`), `crates/soapstone-http/src/lib.rs` (`mod xml;` and re-exports)

**Interfaces:**
- Produces (re-exported from `lib.rs`), used by Task 3:
  ```rust
  /// Response bytes → text. Charset from the Content-Type header, else from the XML declaration, else UTF-8 (lossy). Strips a UTF-8 BOM.
  pub fn decode_body(bytes: &[u8], content_type: Option<&str>) -> String
  /// 3-space indented copy of an XML document; None when `text` is not well-formed XML.
  pub fn pretty_xml(text: &str) -> Option<String>
  /// Text of a SOAP 1.1 `faultstring` or SOAP 1.2 `Reason/Text`; "SOAP Fault" when a Fault has neither; None when not a fault.
  pub fn fault_string(text: &str) -> Option<String>
  ```

- [ ] **Step 1: Write the failing tests (bottom of `src/xml.rs`)**

```rust
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-http xml`
Expected: compile error, `decode_body` not found.

- [ ] **Step 3: Implement the three functions**

- `decode_body`: charset = the `charset=` parameter of `content_type` (case-insensitive, quotes trimmed); if absent and the text starts with `<?xml`, the declaration's `encoding`. `iso-8859-1`, `latin1` and `windows-1252` (case-insensitive) map each byte to the `char` with that code point (`// ponytail:` comment: windows-1252 differs from latin-1 only in 0x80–0x9F, add `encoding_rs` if that matters). Anything else: strip a UTF-8 BOM, then `String::from_utf8_lossy`.
- `pretty_xml`: `None` unless the trimmed text starts with `<`. `quick_xml::Reader::from_str` with `trim_text(true)`, copying every event to `quick_xml::Writer::new_with_indent(Vec::new(), b' ', 3)`. Any reader error, or element depth ≠ 0 at EOF, → `None`.
- `fault_string`: `roxmltree::Document::parse` (error → `None`); the first element named `Fault` in `http://schemas.xmlsoap.org/soap/envelope/` or `http://www.w3.org/2003/05/soap-envelope`; text of its `faultstring` child (any namespace), else of the first `Text` under `Reason`; trimmed.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-http`
Expected: all tests pass (11).

- [ ] **Step 5: Stage**

```bash
git add -A
```

---

### Task 3: `soapstone-http` — send a SOAP call

**Files:**
- Create: `crates/soapstone-http/src/send.rs`, `crates/soapstone-http/tests/send.rs`
- Modify: `crates/soapstone-http/src/lib.rs` (`mod send;`, re-exports), `crates/soapstone-http/Cargo.toml` (dev-dependency `tokio = { version = "1", features = ["macros", "rt"] }`)

**Interfaces:**
- Consumes: `render` (Task 1), `decode_body`, `pretty_xml`, `fault_string` (Task 2), types (Task 1).
- Produces (re-exported from `lib.rs`), used by Tasks 5, 6, 11:
  ```rust
  pub use reqwest::Client;
  /// Shared client: 60 s timeout, User-Agent "Soapstone/<crate version>".
  pub fn client() -> Client
  pub async fn send(client: &Client, call: &SoapCall, vars: &HashMap<String, String>) -> Result<HttpResponse, SendError>
  ```

- [ ] **Step 1: Write the failing tests in `tests/send.rs`**

Helpers at the top of the file (the implementer writes their bodies):

```rust
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;
use soapstone_http::{client, send, Auth, Header, SendError, SoapCall, SoapVersion};

/// Raw HTTP/1.1 response with Content-Length and `Connection: close`.
fn http(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8>
/// Binds 127.0.0.1:0, answers one connection with `response` and sends the raw request (headers + body,
/// read up to Content-Length) through the channel. Returns ("http://127.0.0.1:<port>", receiver).
fn serve_once(response: Vec<u8>) -> (String, mpsc::Receiver<String>)
/// Value of header `name` (case-insensitive) in a raw request.
fn header(raw: &str, name: &str) -> Option<String>
/// Body "<x/>", no headers, Auth::None.
fn call(endpoint: &str, version: SoapVersion, action: Option<&str>) -> SoapCall
fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String>
fn received(rx: mpsc::Receiver<String>) -> String { rx.recv_timeout(Duration::from_secs(5)).unwrap() }
```

Tests:

```rust
#[tokio::test]
async fn soap11_request_has_soapui_headers_and_rendered_body() {
    let (url, rx) = serve_once(http("200 OK", &[("Content-Type", "text/xml")], b"<r><v>1</v></r>"));
    let mut c = call(&format!("{url}/svc"), SoapVersion::V11, Some("urn:a/Op"));
    c.body = "<x>{{v}}</x>".into();
    let resp = send(&client(), &c, &vars(&[("v", "1")])).await.unwrap();
    let raw = received(rx);
    assert!(raw.starts_with("POST /svc HTTP/1.1\r\n"), "{raw}");
    assert_eq!(header(&raw, "content-type").as_deref(), Some("text/xml;charset=UTF-8"));
    assert_eq!(header(&raw, "soapaction").as_deref(), Some("\"urn:a/Op\""));
    assert!(raw.ends_with("\r\n\r\n<x>1</x>"), "{raw}");
    assert_eq!((resp.status, resp.status_text.as_str(), resp.size), (200, "OK", 15));
    assert_eq!(resp.body, "<r><v>1</v></r>");
    assert_eq!(resp.pretty.as_deref(), Some("<r>\n   <v>1</v>\n</r>"));
    assert_eq!(resp.fault, None);
    assert!(resp.headers.iter().any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value == "text/xml"));
}

#[tokio::test]
async fn soap12_puts_the_action_in_the_content_type() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    send(&client(), &call(&url, SoapVersion::V12, Some("urn:a/Op")), &vars(&[])).await.unwrap();
    let raw = received(rx);
    assert_eq!(header(&raw, "content-type").as_deref(), Some("application/soap+xml;charset=UTF-8;action=\"urn:a/Op\""));
    assert_eq!(header(&raw, "soapaction"), None);
}

#[tokio::test]
async fn missing_soap_action_sends_empty_soapaction_or_no_action_parameter() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    send(&client(), &call(&url, SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!(header(&received(rx), "soapaction").as_deref(), Some("\"\""));
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    send(&client(), &call(&url, SoapVersion::V12, None), &vars(&[])).await.unwrap();
    assert_eq!(header(&received(rx), "content-type").as_deref(), Some("application/soap+xml;charset=UTF-8"));
}

#[tokio::test]
async fn basic_auth_is_preemptive_and_bearer_uses_the_token() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    let mut c = call(&url, SoapVersion::V11, None);
    c.auth = Auth::Basic { username: "{{user}}".into(), password: "p".into() };
    send(&client(), &c, &vars(&[("user", "u")])).await.unwrap();
    assert_eq!(header(&received(rx), "authorization").as_deref(), Some("Basic dTpw"));
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    let mut c = call(&url, SoapVersion::V11, None);
    c.auth = Auth::Bearer { token: "{{t}}".into() };
    send(&client(), &c, &vars(&[("t", "abc")])).await.unwrap();
    assert_eq!(header(&received(rx), "authorization").as_deref(), Some("Bearer abc"));
}

#[tokio::test]
async fn user_headers_replace_automatic_ones_and_are_rendered() {
    let (url, rx) = serve_once(http("200 OK", &[], b""));
    let mut c = call(&url, SoapVersion::V11, Some("a"));
    c.headers = vec![
        Header { name: "Content-Type".into(), value: "text/xml".into() },
        Header { name: "X-Env".into(), value: "{{env}}".into() },
        Header { name: "".into(), value: "ignored".into() },
    ];
    send(&client(), &c, &vars(&[("env", "dev")])).await.unwrap();
    let raw = received(rx);
    assert_eq!(raw.to_lowercase().matches("content-type:").count(), 1, "{raw}");
    assert_eq!(header(&raw, "content-type").as_deref(), Some("text/xml"));
    assert_eq!(header(&raw, "x-env").as_deref(), Some("dev"));
}

#[tokio::test]
async fn missing_variables_block_the_request() {
    let mut c = call("{{baseUrl}}/svc", SoapVersion::V11, None);
    c.body = "<x>{{a}}</x>".into();
    c.headers = vec![Header { name: "X-T".into(), value: "{{token}}".into() }];
    c.auth = Auth::Basic { username: "u".into(), password: "{{pass}}".into() };
    let expected = ["baseUrl", "a", "token", "pass"].map(String::from).to_vec();
    assert_eq!(send(&client(), &c, &vars(&[])).await, Err(SendError::MissingVariables(expected)));
}

#[tokio::test]
async fn invalid_endpoint_is_a_readable_error() {
    for endpoint in ["", "localhost:8080/svc", "ftp://example.com/x"] {
        let r = send(&client(), &call(endpoint, SoapVersion::V11, None), &vars(&[])).await;
        assert!(matches!(r, Err(SendError::InvalidUrl(_))), "{endpoint}: {r:?}");
    }
}

#[tokio::test]
async fn connection_refused_is_a_readable_error() {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    match send(&client(), &call(&format!("http://127.0.0.1:{port}/"), SoapVersion::V11, None), &vars(&[])).await {
        Err(SendError::Http(msg)) => assert!(msg.to_lowercase().contains("connection refused"), "{msg}"),
        other => panic!("expected Http error, got {other:?}"),
    }
}

#[tokio::test]
async fn soap_fault_with_latin1_body_is_decoded_and_detected() {
    let body = b"<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><faultcode>s:Server</faultcode><faultstring>Servi\xE7o indispon\xEDvel</faultstring></s:Fault></s:Body></s:Envelope>";
    let (url, _rx) = serve_once(http("500 Internal Server Error", &[("Content-Type", "text/xml; charset=ISO-8859-1")], body));
    let resp = send(&client(), &call(&url, SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!(resp.status, 500);
    assert_eq!(resp.fault.as_deref(), Some("Serviço indisponível"));
    assert!(resp.body.contains("Serviço"));
}

#[tokio::test]
async fn html_error_page_is_kept_raw() {
    let page = "<html><body><p>Bad gateway</body></html>";
    let (url, _rx) = serve_once(http("502 Bad Gateway", &[("Content-Type", "text/html")], page.as_bytes()));
    let resp = send(&client(), &call(&url, SoapVersion::V11, None), &vars(&[])).await.unwrap();
    assert_eq!((resp.status, resp.body.as_str(), resp.pretty, resp.fault), (502, page, None, None));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-http --test send`
Expected: compile error, `send` / `client` not found. If binding fails with "Operation not permitted", the sandbox blocks local ports: enable `sandbox.network.allowLocalBinding`.

- [ ] **Step 3: Implement `client` and `send` in `src/send.rs`**

1. Render, in this order, collecting missing names across all fields (deduplicated, first appearance first): endpoint, body, each header's name then value, auth fields (username, password, or token). Any missing → `MissingVariables`, before any network activity.
2. `reqwest::Url::parse(endpoint)` with scheme `http` or `https`, else `InvalidUrl(endpoint as typed)`.
3. Build `client.post(url).body(body)` with the automatic headers (Global Constraints), then `.basic_auth(user, Some(pass))` or `.bearer_auth(token)`; `.build()` it, then insert each user header with a non-empty name into `request.headers_mut()` (`insert` replaces same-name headers). An invalid header name or value → `Http("Invalid header \"<name>\"")`.
4. Start the clock, `client.execute(request)`, read `bytes()`, stop the clock. Fill `HttpResponse`: `status_text` from `canonical_reason()` (`""` if none), header values with `String::from_utf8_lossy`, `body = decode_body(&bytes, content_type)`, `pretty = pretty_xml(&body)`, `fault = fault_string(&body)`.
5. Any `reqwest::Error` → `Http(message)` where `message` is the error's `Display` followed by each `source()` joined with `": "`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-http && cargo clippy -p soapstone-http --all-targets -- -D warnings`
Expected: all tests pass (21), no warnings.

- [ ] **Step 5: Stage**

```bash
git add -A
```

---

### Task 4: `soapstone-store` — schema, projects, environments

**Files:**
- Create: `crates/soapstone-store/Cargo.toml`, `crates/soapstone-store/src/lib.rs`, `crates/soapstone-store/tests/store.rs`

**Interfaces:**
- Consumes: `soapstone_http::{Auth, Header, HttpResponse}` (Task 1), `soapstone_core::LoadError`.
- Produces (`lib.rs`), used by Tasks 5, 6:
  ```rust
  pub use soapstone_http::{Auth, Header, HttpResponse};

  #[derive(Debug, thiserror::Error)]
  pub enum StoreError {
      #[error(transparent)] Sql(#[from] rusqlite::Error),
      #[error(transparent)] Json(#[from] serde_json::Error),
      #[error(transparent)] Load(#[from] soapstone_core::LoadError),
      #[error("{0} not found")] NotFound(String),      // e.g. "request 42"
  }
  pub type Result<T> = std::result::Result<T, StoreError>;

  #[derive(Debug, Clone, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
  pub struct Project { pub id: i64, pub name: String }

  // All three: #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
  pub struct Variable { pub name: String, pub value: String }
  pub struct Environment { pub name: String, pub variables: Vec<Variable> }
  pub struct Environments { pub base: Vec<Variable>, pub environments: Vec<Environment>, pub active: Option<String> }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)] #[serde(rename_all = "lowercase")]
  pub enum Kind { Project, Service, Request }

  pub struct Store { conn: rusqlite::Connection }
  impl Store {
      pub fn open(path: &Path) -> Result<Store>            // creates the file and schema if needed
      pub fn open_in_memory() -> Result<Store>
      pub fn projects(&self) -> Result<Vec<Project>>       // ordered by id
      pub fn create_project(&self, name: &str) -> Result<i64>
      pub fn rename(&self, kind: Kind, id: i64, name: &str) -> Result<()>   // NotFound when no such row
      pub fn delete(&self, kind: Kind, id: i64) -> Result<()>               // cascades; deleting a missing row is Ok
      pub fn environments(&self, project_id: i64) -> Result<Environments>
      pub fn set_environments(&self, project_id: i64, environments: &Environments) -> Result<()>
      /// Base variables overlaid by the active environment's; empty names skipped; an unknown active name means base only.
      pub fn variables(&self, project_id: i64) -> Result<HashMap<String, String>>
  }
  ```

- [ ] **Step 1: Create the crate**

Package `soapstone-store`, description "Local SQLite storage for the Soapstone client". Dependencies: `soapstone-core` and `soapstone-http` (path), `rusqlite = { version = "0.41", features = ["bundled"] }`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `thiserror = "2"`, `roxmltree = "0.21"` (service names, Task 5).

- [ ] **Step 2: Write the failing tests in `tests/store.rs`**

```rust
use std::collections::HashMap;
use soapstone_store::{Environment, Environments, Kind, Project, Store, Variable};

fn var(name: &str, value: &str) -> Variable { Variable { name: name.into(), value: value.into() } }
fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn a_new_store_has_one_default_project() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.projects().unwrap(), vec![Project { id: 1, name: "My Project".into() }]);
}

#[test]
fn projects_can_be_created_renamed_and_deleted() {
    let s = Store::open_in_memory().unwrap();
    let id = s.create_project("Optitravel").unwrap();
    s.rename(Kind::Project, id, "Opti").unwrap();
    assert_eq!(s.projects().unwrap()[1].name, "Opti");
    s.delete(Kind::Project, id).unwrap();
    assert_eq!(s.projects().unwrap().len(), 1);
    assert!(s.rename(Kind::Project, 999, "x").is_err());
}

#[test]
fn environments_round_trip() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.environments(1).unwrap(), Environments::default());
    let envs = Environments {
        base: vec![var("a", "1")],
        environments: vec![Environment { name: "dev".into(), variables: vec![var("b", "2")] }],
        active: Some("dev".into()),
    };
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.environments(1).unwrap(), envs);
}

#[test]
fn active_environment_overrides_base_variables() {
    let s = Store::open_in_memory().unwrap();
    let mut envs = Environments {
        base: vec![var("a", "1"), var("b", "2"), var("", "ignored")],
        environments: vec![Environment { name: "dev".into(), variables: vec![var("b", "3"), var("c", "4")] }],
        active: None,
    };
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap(), map(&[("a", "1"), ("b", "2")]));
    envs.active = Some("dev".into());
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap(), map(&[("a", "1"), ("b", "3"), ("c", "4")]));
    envs.active = Some("gone".into());
    s.set_environments(1, &envs).unwrap();
    assert_eq!(s.variables(1).unwrap(), map(&[("a", "1"), ("b", "2")]));
}

#[test]
fn data_survives_reopening_the_file() {
    let path = std::env::temp_dir().join(format!("soapstone-store-test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let s = Store::open(&path).unwrap();
        s.create_project("Kept").unwrap();
    }
    let s = Store::open(&path).unwrap();
    let names: Vec<String> = s.projects().unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["My Project", "Kept"]);
    std::fs::remove_file(&path).unwrap();
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p soapstone-store`
Expected: compile error, `Store` not found.

- [ ] **Step 4: Implement `lib.rs`**

`open` / `open_in_memory`: `PRAGMA foreign_keys = ON`; if `PRAGMA user_version` is 0, run the schema below and set `user_version = 1` (later phases add numbered migrations); then insert project "My Project" if the table is empty. `rename` / `delete` pick the table from `Kind` with a `match` (never from strings). Schema (exact):

```sql
CREATE TABLE projects (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  environments TEXT NOT NULL DEFAULT '{"base":[],"environments":[],"active":null}'
);
CREATE TABLE services (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  source_url TEXT NOT NULL,      -- URL of documents[0], used by Refresh WSDL
  documents TEXT NOT NULL        -- JSON [{"url":..,"content":..}], root WSDL first
);
CREATE TABLE operations (
  id INTEGER PRIMARY KEY,
  service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
  binding TEXT NOT NULL,
  name TEXT NOT NULL,
  soap_version TEXT NOT NULL,    -- '1.1' | '1.2'
  soap_action TEXT,
  endpoint TEXT,
  orphan INTEGER NOT NULL DEFAULT 0,
  UNIQUE (service_id, binding, name)
);
CREATE TABLE requests (
  id INTEGER PRIMARY KEY,
  operation_id INTEGER NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  body TEXT NOT NULL,
  headers TEXT NOT NULL DEFAULT '[]',
  auth TEXT NOT NULL DEFAULT '{"type":"none"}',
  last_response TEXT             -- JSON HttpResponse
);
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p soapstone-store`
Expected: 5 passed.

- [ ] **Step 6: Stage**

```bash
git add -A
```

---

### Task 5: `soapstone-store` — services, operations, requests

**Files:**
- Create: `crates/soapstone-store/src/services.rs`, `crates/soapstone-store/tests/services.rs`
- Modify: `crates/soapstone-store/src/lib.rs` (`mod services; pub use services::*;`)

**Interfaces:**
- Consumes: `Store`, `StoreError`, `Result` (Task 4); `soapstone_core::{Document, Definition, SoapVersion}`; `soapstone_http::{SoapCall, Auth, Header, HttpResponse}`.
- Produces (re-exported from `lib.rs`), used by Tasks 6 and 11; the TypeScript types in Task 7 mirror these exactly:
  ```rust
  // #[derive(Debug, Clone, PartialEq, Serialize)] #[serde(rename_all = "camelCase")] on all four
  pub struct ServiceNode { pub id: i64, pub name: String, pub operations: Vec<OperationNode> }      // services by id
  pub struct OperationNode { pub id: i64, pub binding: String, pub name: String, pub orphan: bool, pub requests: Vec<RequestNode> } // by id
  pub struct RequestNode { pub id: i64, pub name: String }                                          // by id
  pub struct Request {
      pub id: i64, pub name: String,
      pub operation: String, pub binding: String,
      pub soap_version: String,              // "1.1" | "1.2"
      pub soap_action: Option<String>,
      pub endpoint: String, pub body: String, pub headers: Vec<Header>, pub auth: Auth,
      pub last_response: Option<HttpResponse>,
  }
  // #[derive(Debug, Clone, PartialEq, Deserialize)] #[serde(rename_all = "camelCase")]
  pub struct RequestEdit { pub id: i64, pub endpoint: String, pub body: String, pub headers: Vec<Header>, pub auth: Auth }

  impl Store {
      /// Parses `documents` (root first); one service, its operations, and "Request 1" with the sample for each. Returns the service id.
      pub fn import_service(&mut self, project_id: i64, documents: Vec<Document>) -> Result<i64>
      /// Replaces the documents; updates or adds operations (new ones get "Request 1"); marks the missing ones orphan; never touches requests.
      pub fn refresh_service(&mut self, service_id: i64, documents: Vec<Document>) -> Result<()>
      pub fn service_source(&self, service_id: i64) -> Result<String>
      pub fn tree(&self, project_id: i64) -> Result<Vec<ServiceNode>>
      pub fn request(&self, id: i64) -> Result<Request>
      pub fn save_request(&self, edit: &RequestEdit) -> Result<()>
      /// "Request {n+1}" with a fresh sample (empty body for an orphan operation).
      pub fn create_request(&self, operation_id: i64) -> Result<i64>
      /// The request as stored plus its operation's version and action, and the project's variables.
      pub fn prepare_send(&self, request_id: i64) -> Result<(SoapCall, HashMap<String, String>)>
      pub fn save_response(&self, request_id: i64, response: &HttpResponse) -> Result<()>
  }
  ```

- [ ] **Step 1: Write the failing tests in `tests/services.rs`**

```rust
use soapstone_core::{fetch_all, to_url, Definition, Document, SoapVersion};
use soapstone_http::SoapCall;
use soapstone_store::*;

fn docs(fixture: &str) -> Vec<Document> {
    let path = format!("{}/../soapstone-core/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    fetch_all(&to_url(&path).unwrap()).unwrap()
}
fn sample(fixture: &str, binding: &str, op: &str) -> String {
    let d = Definition::parse(docs(fixture)).unwrap();
    d.sample_request(d.operation(binding, op).unwrap())
}
fn op<'a>(tree: &'a [ServiceNode], name: &str) -> &'a OperationNode {
    tree.iter().flat_map(|s| &s.operations).find(|o| o.name == name).unwrap()
}
fn edit(id: i64, endpoint: &str, body: &str) -> RequestEdit {
    RequestEdit { id, endpoint: endpoint.into(), body: body.into(), headers: vec![], auth: Auth::None }
}

#[test]
fn import_creates_operations_each_with_a_sample_request() {
    let mut s = Store::open_in_memory().unwrap();
    let sid = s.import_service(1, docs("features/service.wsdl")).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!((tree.len(), tree[0].id, tree[0].name.as_str()), (1, sid, "Features"));
    let ops: Vec<_> = tree[0].operations.iter().map(|o| (o.binding.as_str(), o.name.as_str(), o.orphan, o.requests.len())).collect();
    assert_eq!(ops, [("FeaturesSoap", "GetPerson", false, 1), ("FeaturesSoap", "SaveEmployee", false, 1)]);
    let r = s.request(op(&tree, "GetPerson").requests[0].id).unwrap();
    assert_eq!((r.name.as_str(), r.operation.as_str(), r.binding.as_str()), ("Request 1", "GetPerson", "FeaturesSoap"));
    assert_eq!(r.body, sample("features/service.wsdl", "FeaturesSoap", "GetPerson"));
    assert_eq!(r.endpoint, "http://localhost:8080/features");
    assert_eq!((r.soap_version.as_str(), r.soap_action.as_deref()), ("1.1", Some("urn:features/GetPerson")));
    assert_eq!((r.headers.len(), r.auth, r.last_response), (0, Auth::None, None));
    assert!(s.service_source(sid).unwrap().ends_with("features/service.wsdl"));
}

#[test]
fn calculator_keeps_both_bindings_and_versions() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("calculator/calculator.wsdl")).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!((tree[0].name.as_str(), tree[0].operations.len()), ("Calculator", 8));
    let add12 = tree[0].operations.iter().find(|o| o.binding == "CalculatorSoap12" && o.name == "Add").unwrap();
    assert_eq!(s.request(add12.requests[0].id).unwrap().soap_version, "1.2");
}

#[test]
fn importing_something_that_is_not_a_wsdl_stores_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    let html = vec![Document { url: "file:///page.html".into(), content: "<html/>".into() }];
    assert!(matches!(s.import_service(1, html), Err(StoreError::Load(_))));
    assert!(s.tree(1).unwrap().is_empty());
}

#[test]
fn edits_and_the_last_response_are_saved() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("features/service.wsdl")).unwrap();
    let id = op(&s.tree(1).unwrap(), "GetPerson").requests[0].id;
    let e = RequestEdit {
        id,
        endpoint: "{{baseUrl}}/features".into(),
        body: "<x/>".into(),
        headers: vec![Header { name: "X-A".into(), value: "1".into() }],
        auth: Auth::Bearer { token: "{{t}}".into() },
    };
    s.save_request(&e).unwrap();
    let resp = HttpResponse { status: 200, status_text: "OK".into(), time_ms: 12, size: 4, headers: vec![], body: "<r/>".into(), pretty: Some("<r/>".into()), fault: None };
    s.save_response(id, &resp).unwrap();
    let r = s.request(id).unwrap();
    assert_eq!((r.endpoint, r.body, r.headers, r.auth, r.last_response), (e.endpoint, e.body, e.headers, e.auth, Some(resp)));
}

#[test]
fn new_requests_are_numbered_and_start_from_the_sample() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("features/service.wsdl")).unwrap();
    let get = op(&s.tree(1).unwrap(), "GetPerson").id;
    let r = s.request(s.create_request(get).unwrap()).unwrap();
    assert_eq!(r.name, "Request 2");
    assert_eq!(r.body, sample("features/service.wsdl", "FeaturesSoap", "GetPerson"));
    assert_eq!(op(&s.tree(1).unwrap(), "GetPerson").requests.len(), 2);
}

#[test]
fn refresh_marks_missing_operations_orphan_and_keeps_requests() {
    let mut s = Store::open_in_memory().unwrap();
    let sid = s.import_service(1, docs("features/service.wsdl")).unwrap();
    let get = op(&s.tree(1).unwrap(), "GetPerson").requests[0].id;
    s.save_request(&edit(get, "http://edited", "<edited/>")).unwrap();

    s.refresh_service(sid, docs("calculator/calculator.wsdl")).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!(tree[0].operations.len(), 10);
    assert!(op(&tree, "GetPerson").orphan && op(&tree, "SaveEmployee").orphan);
    assert!(!op(&tree, "Add").orphan && op(&tree, "Add").requests.len() == 1);
    assert_eq!(s.request(get).unwrap().body, "<edited/>");

    s.refresh_service(sid, docs("features/service.wsdl")).unwrap();
    let tree = s.tree(1).unwrap();
    assert!(!op(&tree, "GetPerson").orphan && op(&tree, "Add").orphan);
    assert_eq!(op(&tree, "GetPerson").requests.len(), 1);
    assert_eq!(s.request(get).unwrap().endpoint, "http://edited");
}

#[test]
fn prepare_send_combines_request_operation_and_variables() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs("calculator/calculator.wsdl")).unwrap();
    s.set_environments(1, &Environments { base: vec![Variable { name: "baseUrl".into(), value: "http://x".into() }], ..Default::default() }).unwrap();
    let tree = s.tree(1).unwrap();
    let add12 = tree[0].operations.iter().find(|o| o.binding == "CalculatorSoap12" && o.name == "Add").unwrap();
    let id = add12.requests[0].id;
    s.save_request(&edit(id, "{{baseUrl}}/calc", "<b/>")).unwrap();
    let (call, vars) = s.prepare_send(id).unwrap();
    assert_eq!(call, SoapCall {
        endpoint: "{{baseUrl}}/calc".into(),
        soap_version: SoapVersion::V12,
        soap_action: Some("http://tempuri.org/Add".into()),
        headers: vec![],
        auth: Auth::None,
        body: "<b/>".into(),
    });
    assert_eq!(vars["baseUrl"], "http://x");
}

#[test]
fn renaming_and_deleting_work_for_every_kind() {
    let mut s = Store::open_in_memory().unwrap();
    let p = s.create_project("P").unwrap();
    let sid = s.import_service(p, docs("features/service.wsdl")).unwrap();
    let tree = s.tree(p).unwrap();
    let (get, save) = (op(&tree, "GetPerson").requests[0].id, op(&tree, "SaveEmployee").requests[0].id);
    s.rename(Kind::Service, sid, "Svc").unwrap();
    s.rename(Kind::Request, get, "Mine").unwrap();
    let tree = s.tree(p).unwrap();
    assert_eq!((tree[0].name.as_str(), op(&tree, "GetPerson").requests[0].name.as_str()), ("Svc", "Mine"));
    s.delete(Kind::Request, get).unwrap();
    assert!(op(&s.tree(p).unwrap(), "GetPerson").requests.is_empty());
    s.delete(Kind::Project, p).unwrap();
    assert!(s.tree(p).unwrap().is_empty());
    assert!(matches!(s.request(save), Err(StoreError::NotFound(_))));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p soapstone-store --test services`
Expected: compile error, `import_service` not found.

- [ ] **Step 3: Implement `services.rs`**

- `import_service`: `Definition::parse(documents.clone())?` before touching the database (so a non-WSDL stores nothing); then, in one transaction: insert the service (`source_url = documents[0].url`, `documents` as JSON `[{"url":..,"content":..}]`, name from `service_name`), then for each `Definition::operations()` entry in order an `operations` row (`soap_version` `"1.1"`/`"1.2"`, `soap_action`, `endpoint`) and a `requests` row `"Request 1"` with `endpoint = op.endpoint or ""` and `body = sample_request(op)`.
- `fn service_name(documents: &[Document]) -> String` (private): first `{http://schemas.xmlsoap.org/wsdl/}service/@name` in the root document (roxmltree); else the last path segment of `documents[0].url` without query and extension; else `"Service"`.
- `refresh_service`: parse first (error → nothing changes), then one transaction: replace `documents` and `source_url`; set `orphan = 1` on all the service's operations; for each parsed operation, update the row with the same `(binding, name)` (version, action, endpoint, `orphan = 0`) or insert it with "Request 1". The service name is kept (the user may have renamed it).
- `create_request`: parse the service's stored documents, sample of `operation(binding, name)` or `""` when missing; name `"Request {count + 1}"`; endpoint from the operation row or `""`.
- `request` / `prepare_send` on an unknown id → `NotFound("request {id}")`.
- `tree` of an unknown project → empty vector.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p soapstone-store && cargo clippy -p soapstone-store --all-targets -- -D warnings`
Expected: all tests pass (13), no warnings.

- [ ] **Step 5: Stage**

```bash
git add -A
```

---

### Task 6: Tauri app scaffold and commands

**Files:**
- Modify: `Cargo.toml` (members `["crates/*", "app/src-tauri"]`), `.gitignore` (add `/app/node_modules`, `/app/dist`, `/app/src-tauri/gen/schemas`)
- Create: `app/package.json`, `app/package-lock.json` (by `npm install`), `app/index.html`, `app/vite.config.ts`, `app/tsconfig.json`, `app/src/main.tsx`
- Create: `app/src-tauri/Cargo.toml`, `app/src-tauri/build.rs`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/default.json`, `app/src-tauri/app-icon.svg`, `app/src-tauri/icons/*` (generated), `app/src-tauri/src/main.rs`, `app/src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `soapstone_store::*` (Tasks 4–5), `soapstone_http::{client, send, Client, HttpResponse, SendError}` (Tasks 1–3), `soapstone_core::{to_url, fetch_all}`.
- Produces: the IPC surface Task 7 wraps. Tauri converts snake_case Rust parameters to camelCase JS keys. Every command is `async fn` returning `Result<T, String>`; an `Err` rejects the JS promise with that message.

  | Command | JS args | Resolves to |
  |---|---|---|
  | `list_projects` | – | `Project[]` |
  | `create_project` | `{ name }` | new id |
  | `rename` | `{ kind, id, name }` | – |
  | `delete` | `{ kind, id }` | – |
  | `get_tree` | `{ projectId }` | `ServiceNode[]` |
  | `import_service` | `{ projectId, source }` (URL or file path) | new service id |
  | `refresh_service` | `{ serviceId }` | – |
  | `get_request` | `{ id }` | `Request` |
  | `save_request` | `{ edit }` | – |
  | `create_request` | `{ operationId }` | new request id |
  | `send_request` | `{ edit }` (saved first) | `HttpResponse` |
  | `cancel_send` | `{ requestId }` | – |
  | `get_environments` | `{ projectId }` | `Environments` |
  | `set_environments` | `{ projectId, environments }` | – |
  | `log_metric` | `{ name, ms }` (`ms` optional) | – |

- [ ] **Step 1: Frontend skeleton**

`app/package.json` (exact):

```json
{
  "name": "soapstone",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "test": "vitest run",
    "tauri": "tauri"
  },
  "dependencies": {
    "@codemirror/lang-xml": "^6.1.0",
    "@codemirror/state": "^6.7.6",
    "@codemirror/theme-one-dark": "^6.1.3",
    "@codemirror/view": "^6.43.15",
    "@tauri-apps/api": "^2.12.4",
    "@tauri-apps/plugin-dialog": "^2.8.4",
    "codemirror": "^6.0.2",
    "react": "^19.3.1",
    "react-dom": "^19.3.1"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.12.4",
    "@types/react": "^19.3.0",
    "@types/react-dom": "^19.3.0",
    "@vitejs/plugin-react": "^6.1.2",
    "typescript": "~7.0.2",
    "vite": "^8.3.5",
    "vitest": "^5.0.5"
  }
}
```

Run `npm --prefix app install` (installs into `app/node_modules` only; approved with this plan).

- `vite.config.ts`: `@vitejs/plugin-react`; `clearScreen: false`; `server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } }`.
- `tsconfig.json`: the create-vite 9.3 React template's `tsconfig.app.json` options (`target es2023`, `lib ["ES2023","DOM"]`, `module esnext`, `moduleResolution bundler`, `jsx react-jsx`, `verbatimModuleSyntax`, `noEmit`, `skipLibCheck`, `types ["vite/client"]`, `noUnusedLocals`, `noUnusedParameters`) plus `"strict": true`; `include ["src"]`. One file, no project references.
- `index.html`: title "Soapstone", `<div id="root">`, module script `/src/main.tsx`.
- `src/main.tsx` (temporary, Task 7 replaces it): calls `invoke("list_projects")`, renders "Soapstone" and the project names, then `invoke("log_metric", { name: "startup" })`.

- [ ] **Step 2: Tauri crate**

`app/src-tauri/Cargo.toml`: package `soapstone`, `edition = "2024"`, `rust-version = "1.88"`, `license = "MIT"`. Build-dependency `tauri-build = "2.7"`. Dependencies: `tauri = "2.12"`, `tauri-plugin-dialog = "2.8"`, `serde = { version = "1", features = ["derive"] }`, `tokio = { version = "1", features = ["sync", "macros"] }`, and path dependencies on `soapstone-core`, `soapstone-http`, `soapstone-store`. If Tauri's macros reject edition 2024, use `edition = "2021"` for this crate only and record the ruling.

`build.rs`: `fn main() { tauri_build::build() }`.

`tauri.conf.json` (exact):

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Soapstone",
  "version": "0.1.0",
  "identifier": "io.github.gsimei.soapstone",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{ "title": "Soapstone", "width": 1280, "height": 800, "minWidth": 900, "minHeight": 560 }],
    "security": { "csp": "default-src 'self' ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'" }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"]
  }
}
```

If Vite's hot reload is blocked by the CSP in dev mode, add `"devCsp"` with the same policy plus `connect-src 'self' ipc: http://ipc.localhost ws://localhost:1420`.

`capabilities/default.json`: identifier `"default"`, windows `["main"]`, permissions `["core:default", "dialog:default"]`.

`app-icon.svg` (exact), then `npm --prefix app run tauri icon src-tauri/app-icon.svg`. Keep `icons/32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, `icon.png`; delete every other generated file (mobile and Windows Store icons).

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <rect width="1024" height="1024" rx="224" fill="#3B6E63"/>
  <circle cx="470" cy="560" r="250" fill="none" stroke="#F4F1EA" stroke-width="64"/>
  <circle cx="720" cy="300" r="110" fill="none" stroke="#F4F1EA" stroke-width="48"/>
</svg>
```

- [ ] **Step 3: `main.rs` and `commands.rs`**

```rust
// main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
pub struct AppState {
    pub store: Mutex<Store>,
    pub client: soapstone_http::Client,
    pub inflight: Mutex<HashMap<i64, tokio::sync::oneshot::Sender<()>>>,  // request id → cancel
    pub started: Instant,                                                // first line of main()
}
impl AppState {
    /// Recovers from a poisoned lock so one panic does not disable every later command.
    pub fn store(&self) -> MutexGuard<'_, Store>
}
```

- `main`: `Builder::default().plugin(tauri_plugin_dialog::init())`; in `setup`, create `app.path().app_data_dir()`, `Store::open(dir.join("soapstone.db"))`, `app.manage(AppState { .., client: soapstone_http::client(), .. })`; register every command with `generate_handler!`.
- `import_service` / `refresh_service`: take `app: AppHandle` and run inside `tauri::async_runtime::spawn_blocking`: `to_url` → `fetch_all` → store call (refresh reads `service_source` first). Print `metric fetch: {ms:.1} ms ({n} documents)` and `metric import: {ms:.1} ms ({kb} KB)` (the store call alone) to stderr.
- `import_service`, `refresh_service` and `create_request` parse WSDLs, and the core's schema recursion has no depth cap: run their store call through `fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T` (a scoped `std::thread::Builder::new().stack_size(64 << 20)` thread, joined), inside `spawn_blocking`. A hostile schema then fails that thread's stack later instead of the 2 MB worker's (`// ponytail:` comment: depth cap in core is the real fix).
- `send_request`: under the lock, `save_request(&edit)` then `prepare_send(edit.id)`; release the lock; register a oneshot in `inflight`; `tokio::select!` between `soapstone_http::send(..)` and the oneshot (→ `SendError::Cancelled`); remove the entry; on `Ok` call `save_response`; return `Err(e.to_string())` on failure, leaving the stored response unchanged.
- `cancel_send`: remove the sender for `request_id` and fire it; no-op when absent.
- `log_metric(name: String, ms: Option<f64>)`: prints `metric {name}: {ms:.1} ms` to stderr; `ms = None` means time since `started`.

- [ ] **Step 4: Build, lint, test**

Run: `npm --prefix app run build && cargo build -p soapstone && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all succeed; the 1a and Tasks 1–5 tests still pass.

- [ ] **Step 5: Launch in dev mode**

Run (background, stop it after checking): `npm --prefix app run tauri dev`
Expected: a "Soapstone" window shows "My Project"; stderr has `metric startup: … ms`; `~/Library/Application Support/io.github.gsimei.soapstone/soapstone.db` exists.

- [ ] **Step 6: Stage**

```bash
git add -A
```

---

### Task 7: Frontend foundations — API, logic, layout

**Files:**
- Create: `app/src/api.ts`, `app/src/logic.ts`, `app/src/logic.test.ts`, `app/src/Split.tsx`, `app/src/App.tsx`, `app/src/styles.css`
- Modify: `app/src/main.tsx`

**Interfaces:**
- Consumes: the command table in Task 6.
- Produces (used by Tasks 8–10):
  ```ts
  // api.ts — mirrors the Rust serde output exactly
  export type Project = { id: number; name: string };
  export type Header = { name: string; value: string };
  export type Variable = { name: string; value: string };
  export type Auth = { type: "none" } | { type: "basic"; username: string; password: string } | { type: "bearer"; token: string };
  export type Environment = { name: string; variables: Variable[] };
  export type Environments = { base: Variable[]; environments: Environment[]; active: string | null };
  export type RequestNode = { id: number; name: string };
  export type OperationNode = { id: number; binding: string; name: string; orphan: boolean; requests: RequestNode[] };
  export type ServiceNode = { id: number; name: string; operations: OperationNode[] };
  export type HttpResponse = { status: number; statusText: string; timeMs: number; size: number; headers: Header[]; body: string; pretty: string | null; fault: string | null };
  export type Request = { id: number; name: string; operation: string; binding: string; soapVersion: "1.1" | "1.2"; soapAction: string | null; endpoint: string; body: string; headers: Header[]; auth: Auth; lastResponse: HttpResponse | null };
  export type RequestEdit = Pick<Request, "id" | "endpoint" | "body" | "headers" | "auth">;
  export type Kind = "project" | "service" | "request";
  export const api: {
    listProjects(): Promise<Project[]>;
    createProject(name: string): Promise<number>;
    rename(kind: Kind, id: number, name: string): Promise<void>;
    remove(kind: Kind, id: number): Promise<void>;                  // command "delete"
    tree(projectId: number): Promise<ServiceNode[]>;
    importService(projectId: number, source: string): Promise<number>;
    refreshService(serviceId: number): Promise<void>;
    getRequest(id: number): Promise<Request>;
    saveRequest(edit: RequestEdit): Promise<void>;
    createRequest(operationId: number): Promise<number>;
    send(edit: RequestEdit): Promise<HttpResponse>;
    cancelSend(requestId: number): Promise<void>;
    getEnvironments(projectId: number): Promise<Environments>;
    setEnvironments(projectId: number, environments: Environments): Promise<void>;
    logMetric(name: string, ms?: number): Promise<void>;
  };

  // logic.ts — pure, no Tauri imports (only `import type`)
  export const VARIABLE_RE: RegExp;                                  // /\{\{\s*([A-Za-z_][\w.-]*)\s*\}\}/g, the Rust grammar
  export function definedVariables(envs: Environments): Set<string>; // base ∪ active environment, non-empty names
  export function matches(text: string, query: string): boolean;     // every space-separated term is a case-insensitive substring
  export function filterTree(tree: ServiceNode[], query: string): ServiceNode[];
  export type QuickItem = { requestId: number; label: string };
  export function quickOpenItems(tree: ServiceNode[]): QuickItem[];  // label "Service › Operation › Request"
  export function formatSize(bytes: number): string;                // "512 B", "4.1 KB", "2.3 MB"

  // Split.tsx — two panes and a draggable divider; first pane's share of the width persisted in localStorage
  export function Split(props: { storageKey: string; initial: number; min: number; children: [ReactNode, ReactNode] }): JSX.Element
  ```

- [ ] **Step 1: Write the failing tests in `src/logic.test.ts`**

```ts
import { expect, test } from "vitest";
import { VARIABLE_RE, definedVariables, filterTree, formatSize, matches, quickOpenItems } from "./logic";
import type { ServiceNode } from "./api";

const tree: ServiceNode[] = [
  { id: 1, name: "wbs_methods", operations: [
    { id: 10, binding: "wbs_methodsSoap", name: "AVITIK_GETGROUPRESERVES", orphan: false, requests: [{ id: 100, name: "Request 1" }] },
    { id: 11, binding: "wbs_methodsSoap", name: "CreateOptitravelReserve", orphan: false, requests: [{ id: 101, name: "Request 1" }, { id: 102, name: "With traveller" }] },
  ] },
  { id: 2, name: "Calculator", operations: [
    { id: 20, binding: "CalculatorSoap", name: "Add", orphan: false, requests: [{ id: 200, name: "Request 1" }] },
  ] },
];

test("VARIABLE_RE follows the Rust grammar", () => {
  expect([..."{{a}} {{ b }} {{a b}} {{}} {{1x}} {{c.d-e_f}}".matchAll(VARIABLE_RE)].map((m) => m[1])).toEqual(["a", "b", "c.d-e_f"]);
});

test("definedVariables merges base and the active environment only", () => {
  const names = definedVariables({
    base: [{ name: "a", value: "" }, { name: "", value: "x" }],
    environments: [{ name: "dev", variables: [{ name: "b", value: "1" }] }, { name: "prod", variables: [{ name: "c", value: "1" }] }],
    active: "dev",
  });
  expect([...names].sort()).toEqual(["a", "b"]);
});

test("matches needs every term, case-insensitively", () => {
  expect(matches("AVITIK_GETGROUPRESERVES", "avitik group")).toBe(true);
  expect(matches("GetPrices", "get x")).toBe(false);
  expect(matches("anything", "  ")).toBe(true);
});

test("filterTree keeps matching operations and drops empty services", () => {
  expect(filterTree(tree, "")).toBe(tree);
  const names = (q: string) => filterTree(tree, q).map((s) => [s.name, s.operations.map((o) => o.name)]);
  expect(names("add")).toEqual([["Calculator", ["Add"]]]);
  expect(names("calc")).toEqual([["Calculator", ["Add"]]]);
  expect(names("traveller")).toEqual([["wbs_methods", ["CreateOptitravelReserve"]]]);
});

test("quickOpenItems lists every request with its path", () => {
  expect(quickOpenItems(tree).map((i) => [i.requestId, i.label])).toEqual([
    [100, "wbs_methods › AVITIK_GETGROUPRESERVES › Request 1"],
    [101, "wbs_methods › CreateOptitravelReserve › Request 1"],
    [102, "wbs_methods › CreateOptitravelReserve › With traveller"],
    [200, "Calculator › Add › Request 1"],
  ]);
});

test("formatSize", () => {
  expect([formatSize(512), formatSize(4198), formatSize(2411724)]).toEqual(["512 B", "4.1 KB", "2.3 MB"]);
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `npm --prefix app test`
Expected: FAIL, cannot resolve `./logic`.

- [ ] **Step 3: Implement `logic.ts` and `api.ts`**

`filterTree` returns `tree` itself for a blank query; otherwise it keeps an operation when `matches(`${service.name} ${op.name} ${requestNames}`, query)`, with all its requests, and drops services left empty. `api.ts` wraps `invoke` from `@tauri-apps/api/core` with the command names and camelCase argument keys from Task 6.

- [ ] **Step 4: Run to verify they pass**

Run: `npm --prefix app test`
Expected: 6 passed.

- [ ] **Step 5: Layout, styles, app state**

- `styles.css`: tokens on `:root` (`--bg`, `--bg-panel`, `--bg-hover`, `--fg`, `--fg-muted`, `--border`, `--accent`, `--ok`, `--warn`, `--error`, `--font-ui: system-ui`, `--font-mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`), dark values under `@media (prefers-color-scheme: dark)`, `color-scheme: light dark`, full-height layout with no page scroll (panes scroll on their own).
- `Split`: flex row; the divider is a 5 px element using pointer events with pointer capture; width share clamped to `[min, 1 - min]`; saved to `localStorage["soapstone.split.<storageKey>"]`.
- `App.tsx` state: `projects`, `projectId` (restored from `localStorage["soapstone.project"]`, else the first project), `tree`, `environments`, `requestId` (restored from `localStorage["soapstone.request.<projectId>"]` if it is still in the tree), `switchStart` (a `performance.now()` mark set whenever a request is opened). Layout: `<Split storageKey="sidebar" initial={0.22} min={0.12}>` holding the sidebar (Task 8 fills it) and the main area. The main area has a top bar (Task 10 fills it) above the open request (Task 9 renders it, with its own request/response `Split`), or the text "Import a WSDL or pick a request." when none is open. After the first load completes, call `api.logMetric("startup")` once.
- `main.tsx`: renders `<App/>` and imports `styles.css`.

- [ ] **Step 6: Build and check**

Run: `npm --prefix app run build && npm --prefix app test`
Expected: both succeed. `npm --prefix app run tauri dev`: the empty sidebar | main layout fills the window in light and dark mode, and the divider drags and keeps its position after a restart.

- [ ] **Step 7: Stage**

```bash
git add -A
```

---

### Task 8: Sidebar and WSDL import

**Files:**
- Create: `app/src/Sidebar.tsx`, `app/src/ImportDialog.tsx`
- Modify: `app/src/App.tsx`, `app/src/styles.css`

**Interfaces:**
- Consumes: `api`, `ServiceNode`, `Project`, `filterTree`, `matches` (Task 7).
- Produces:
  ```ts
  export function Sidebar(props: {
    projects: Project[]; projectId: number; tree: ServiceNode[]; requestId: number | null;
    onSelectProject(id: number): void;
    onOpenRequest(id: number): void;
    onChanged(): Promise<void>;               // App reloads projects + tree
  }): JSX.Element
  export function ImportDialog(props: { projectId: number; onClose(): void; onImported(serviceId: number): void }): JSX.Element
  ```

- [ ] **Step 1: Implement**

- **Project switcher** (top): a button with the current project's name. Clicking it pops a native menu (`Menu.new` from `@tauri-apps/api/menu`): every project (current one checked), a separator, "New project", "Rename project", "Delete project". "New project" creates "New Project", selects it and opens inline rename. Delete asks with `ask()` from `@tauri-apps/plugin-dialog` (kind `"warning"`).
- **Search** input under the switcher (`aria-label="Search operations"`), filtering with `filterTree`; a non-empty query expands everything.
- **"Import WSDL"** button opens `ImportDialog`.
- **Tree**, with rows as `<button>`s: services (collapsible, expanded by default) → operations → requests.
  - When the service has more than one binding, each operation shows its binding in muted text.
  - An operation with exactly one request is one row that opens it. With two or more, the row toggles its child request rows. With none, clicking calls `createRequest` and opens the new request.
  - Orphan operations are muted, with the tooltip "Removed from the WSDL".
  - The open request (or the single-request operation holding it) is highlighted.
- **Row menus** (right-click, and a "⋯" button shown on hover; both pop the same native menu):
  - Service: "Refresh WSDL", "Rename", "Delete".
  - Operation: "New request".
  - Request: "Rename", "Delete".
  - Rename edits inline (Enter saves, Escape cancels, empty names are refused). Delete asks first. Refresh shows "Refreshing…" on the row; errors go to `message(err, { kind: "error" })`.
- **ImportDialog** (native `<dialog>` opened with `showModal()`): a "WSDL URL or file path" input; a "Browse…" button (`open()` from `@tauri-apps/plugin-dialog`, filter `wsdl`/`xml` plus all files) that fills the input; an "Import" button, disabled and showing "Importing…" while running; the error message shown inline in the dialog. On success the App reloads the tree and opens the new service's first request.

- [ ] **Step 2: Build**

Run: `npm --prefix app run build && npm --prefix app test`
Expected: both succeed.

- [ ] **Step 3: Check in the app (`npm --prefix app run tauri dev`)**

1. Browse to `crates/soapstone-core/tests/fixtures/calculator/calculator.wsdl` → service "Calculator", 8 operations, binding names shown.
2. Import `https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php` (no `?wsdl`) → 18 operations. Importing `https://example.com` shows a readable error and adds nothing. Importing `crates/soapstone-core/tests/fixtures/broken/root.wsdl` shows an error naming `missing.xsd` and adds nothing.
3. Search "group" → only the AVITIK_GETGROUPRESERVES* operations.
4. "New request" on an operation → it gains two child rows.
5. Rename and delete a request, a service and a project; delete asks first.
6. "Refresh WSDL" on Calculator → no visible change.
7. Quit and relaunch → same projects, services, requests and selected project.

- [ ] **Step 4: Stage**

```bash
git add -A
```

---

### Task 9: Request editor, send and response

**Files:**
- Create: `app/src/XmlEditor.tsx`, `app/src/KeyValueTable.tsx`, `app/src/RequestView.tsx`, `app/src/ResponsePane.tsx`
- Modify: `app/src/App.tsx`, `app/src/styles.css`

**Interfaces:**
- Consumes: `api`, `Request`, `RequestEdit`, `HttpResponse`, `Auth`, `Split`, `formatSize`, `VARIABLE_RE` (Task 7).
- Produces:
  ```ts
  export function XmlEditor(props: {
    docKey: string | number;          // a new key replaces the document and its undo history
    value: string;
    onChange?(value: string): void;
    readOnly?: boolean;
    definedVars?: Set<string>;        // {{name}} marked .cm-var when defined, .cm-var-missing otherwise
  }): JSX.Element
  export function KeyValueTable(props: {
    rows: { name: string; value: string }[];
    onChange(rows: { name: string; value: string }[]): void;
    namePlaceholder: string; valuePlaceholder: string;
  }): JSX.Element
  // Rendered by App as <RequestView key={requestId} .../>; owns loading, editing, autosave and sending.
  export function RequestView(props: { requestId: number; definedVars: Set<string>; switchStart: number | null }): JSX.Element
  export function ResponsePane(props: { response: HttpResponse | null; error: string | null; sending: boolean }): JSX.Element
  ```

- [ ] **Step 1: Implement**

- **`XmlEditor`**: one `EditorView` per mount.
  - Extensions: `basicSetup` from `codemirror`, `xml()` from `@codemirror/lang-xml`, `EditorState.readOnly` when `readOnly`.
  - A theme compartment with `oneDark` while `matchMedia("(prefers-color-scheme: dark)")` matches, updated on its `change` event.
  - A compartment with a `MatchDecorator` view plugin over `VARIABLE_RE`, reconfigured when `definedVars` changes.
  - `Prec.highest(keymap.of([{ key: "Mod-Enter", run: () => true }]))`: stops `basicSetup` from inserting a blank line; the window-level handler sends.
  - When `docKey` changes, `view.setState(...)` with the new `value`.
- **`KeyValueTable`**: the given rows plus one empty trailing row; typing in the trailing row adds a row; a "×" button removes one.
- **`RequestView`**:
  - Loads the request with `api.getRequest`, keeps an editable `RequestEdit`, autosaves 500 ms after the last change, and on unmount saves any pending edit right away.
  - After the first render with the loaded request, `requestAnimationFrame(() => api.logMetric("switch", performance.now() - switchStart))` when `switchStart` is set.
  - Layout: `<Split storageKey="panes" initial={0.5} min={0.2}>` with the request side and `<ResponsePane>`.
  - Request side:
    - Title `{operation} › {name}`, with a muted line `SOAP {soapVersion} · SOAPAction "{soapAction ?? ""}"`.
    - Endpoint row: "POST" label, endpoint input (`aria-label="Endpoint"`), Send button (`title="Send (⌘↵ / Ctrl+Enter)"`) that becomes Cancel while sending.
    - Tabs Body | Headers (count) | Auth (type). Body: `XmlEditor` with `definedVars`. Headers: `KeyValueTable` plus the hint "Content-Type and SOAPAction are sent automatically; a header with the same name replaces them." Auth: a `<select>` None / Basic / Bearer, then Username + Password or Token inputs; the hint "Values can use {{variables}}."
  - Sending: from the button, or `Cmd/Ctrl+Enter` through a `window` keydown listener. Cancel any pending autosave, call `api.send(edit)`, keep `{ response, error, sending }`; Cancel calls `api.cancelSend(id)`. The initial response is `request.lastResponse`. A response arriving after the user switched requests is ignored here (the backend already stored it).
- **`ResponsePane`**:
  - Empty: "Send the request to see the response." Sending: "Sending…". Error: the message in an error panel.
  - Response:
    - Status line: badge `{status} {statusText}` (2xx `--ok`, 3xx `--warn`, ≥ 400 `--error`), `{timeMs} ms`, `formatSize(size)`; with a fault, a "SOAP Fault" badge and the fault text.
    - Tabs Body | Headers (count) | Raw. Body: read-only `XmlEditor` with `pretty ?? body`. Headers: a two-column table. Raw: read-only `XmlEditor` with `body`. Each new response gets a new `docKey`.

- [ ] **Step 2: Build**

Run: `npm --prefix app run build && npm --prefix app test`
Expected: both succeed.

- [ ] **Step 3: Check in the app**

1. Calculator `CalculatorSoap/Add`: replace the `?` values with 1 and 2 → Send → `200 OK`, time, size, formatted body with `<AddResult>3</AddResult>`; Headers and Raw tabs filled.
2. `Cmd+Enter` inside the body editor sends and does not insert a line.
3. Endpoint `{{baseUrl}}/calculator.asmx` → Send → "Undefined variables: baseUrl", nothing sent. `{{nope}}` in the body is marked as missing.
4. Endpoint `http://127.0.0.1:9/` → a readable connection error. Switch away and back → the last successful response is shown again.
5. Optitravel `AVITIK_GETGROUPRESERVES` → Send → a response is shown (a "SOAP Fault" badge if the server returns a fault); accented text, if any, is intact.
6. Click through ten requests quickly; edits typed before switching are still there after a relaunch.
7. Switch macOS appearance → editors and panes follow light/dark.

- [ ] **Step 4: Stage**

```bash
git add -A
```

---

### Task 10: Environments and quick open

**Files:**
- Create: `app/src/EnvironmentDialog.tsx`, `app/src/QuickOpen.tsx`
- Modify: `app/src/App.tsx`, `app/src/styles.css`

**Interfaces:**
- Consumes: `api`, `Environments`, `ServiceNode`, `KeyValueTable` (Task 9), `quickOpenItems`, `matches`, `definedVariables` (Task 7).
- Produces:
  ```ts
  export function EnvironmentDialog(props: { initial: Environments; onSave(environments: Environments): void; onClose(): void }): JSX.Element
  export function QuickOpen(props: { tree: ServiceNode[]; onOpen(requestId: number): void; onClose(): void }): JSX.Element
  ```

- [ ] **Step 1: Implement**

- **Top bar** (above the request/response split):
  - Environment `<select aria-label="Environment">` with "No environment" (value `""`) and every environment name. Changing it saves `{ ...environments, active }` and reloads.
  - A "Manage environments" button opens the dialog.
  - App passes `definedVariables(environments)` to `RequestView`.
- **EnvironmentDialog**, a native `<dialog>` with `showModal()` (Escape → `onClose`), working on a local copy:
  - Left list: "Base variables", every environment, and "+ Add environment" (adds "New environment" and selects it).
  - Right side: a name input and "Delete environment" (asks first) for environments, then a `KeyValueTable` of variables.
  - Footer hint: "Use {{name}} in the endpoint, headers, body and auth. The active environment overrides base variables."
  - Footer buttons Cancel and Save. Save is disabled, with the reason shown, while an environment name is empty or duplicated.
  - Renaming the active environment moves `active` with it; deleting it sets `active` to `null`.
- **QuickOpen**, a native `<dialog>`, opened with `Cmd/Ctrl+P` (App `window` keydown, `preventDefault`):
  - An autofocused input.
  - Up to 50 rows of `quickOpenItems(tree)` filtered by `matches(label, query)`.
  - ↑/↓ moves the selection, Enter or click opens the request, Escape closes.

- [ ] **Step 2: Build**

Run: `npm --prefix app run build && npm --prefix app test`
Expected: both succeed.

- [ ] **Step 3: Check in the app**

1. Base `baseUrl = http://www.dneonline.com`; environment "broken" with `baseUrl = http://127.0.0.1:9`.
2. Calculator Add with endpoint `{{baseUrl}}/calculator.asmx`: "No environment" → `200 OK`; "broken" → connection error; back to "No environment" → `200 OK`.
3. `{{baseUrl}}` in the body is marked as defined; `{{nope}}` as missing.
4. Two environments with the same name → Save disabled, with the reason shown.
5. `Cmd+P`, type "avitik group", Enter → that request opens.
6. Relaunch → environments and the active environment are kept.

- [ ] **Step 4: Stage**

```bash
git add -A
```

---

### Task 11: End-to-end check, performance, README

**Files:**
- Create: `crates/soapstone-store/tests/live.rs`, `README.md`
- Modify: `crates/soapstone-store/Cargo.toml` (dev-dependency `tokio = { version = "1", features = ["macros", "rt"] }`)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Bring in core fixes**

Run: `git log --oneline HEAD..feat/phase-1a-core`
If the user committed fixes there, `git merge feat/phase-1a-core`, then run the Step 3 suite.

- [ ] **Step 2: Live backend test (`tests/live.rs`)**

```rust
use soapstone_core::{fetch_all, to_url};
use soapstone_store::Store;

const OPTITRAVEL: &str = "https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php";

#[tokio::test]
#[ignore = "network: the user's real service"]
async fn optitravel_import_send_and_store_the_response() {
    let docs = tokio::task::spawn_blocking(|| fetch_all(&to_url(OPTITRAVEL).unwrap())).await.unwrap().unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.import_service(1, docs).unwrap();
    let tree = s.tree(1).unwrap();
    assert_eq!(tree[0].operations.len(), 18);
    let id = tree[0].operations.iter().find(|o| o.name == "AVITIK_GETGROUPRESERVES").unwrap().requests[0].id;
    let (call, vars) = s.prepare_send(id).unwrap();
    let resp = soapstone_http::send(&soapstone_http::client(), &call, &vars).await.unwrap();
    assert!(resp.status > 0 && !resp.body.is_empty(), "{resp:?}");
    s.save_response(id, &resp).unwrap();
    assert_eq!(s.request(id).unwrap().last_response, Some(resp));
}
```

Run: `cargo test -p soapstone-store --test live -- --ignored --nocapture`
Expected: PASS (the request carries `?` placeholders, so the server answers with an error or fault; any HTTP answer passes).

- [ ] **Step 3: Whole suite**

Run: `npm --prefix app test && npm --prefix app run build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 4: Acceptance in dev mode (with the user)**

`npm --prefix app run tauri dev`, on a clean data directory: import `https://www2.optitravel.net/optitravel/export/wbs/opti_res/wbs_server.php` (no `?wsdl`) → open `AVITIK_GETGROUPRESERVES` → Send → the response shows status, time, size, headers, formatted XML and raw text. Record the outcome in the ledger.

- [ ] **Step 5: Performance (release build)**

```bash
npm --prefix app run tauri build -- --no-bundle
curl -sSL https://developer.ebay.com/webservices/latest/ebaySvc.wsdl -o "$TMPDIR/ebay.wsdl"
cargo run -q --release -p soapstone-core --example soapstone-sample -- "$TMPDIR/ebay.wsdl" --time > /dev/null
./target/release/soapstone
```

Also download a public WSDL with ≥ 300 operations (check the count with `soapstone-sample <file> | wc -l`). In the release app, read stderr for:
- `metric startup` (target < 1000 ms);
- `metric import` after importing both files with Browse… (target < 2000 ms; it includes generating every sample and writing the rows);
- `metric switch`, max over ten requests clicked in a row, including eBay `AddOrder` (an ~8.6k-line sample) (target < 50 ms); the editor must also scroll and type smoothly in that request.

`soapstone-sample --time` gives `parse` and `slowest sample` (target < 20 ms). If a target is missed, profile before changing code. Record every number, file size and operation count in the ledger.

- [ ] **Step 6: README**

`README.md`:
- "# Soapstone", plus one paragraph: a fast, free desktop SOAP client, a lighter alternative to SoapUI, built with Rust and Tauri.
- **Status**: Phase 1b, with a list of what works today.
- **Development**:
  - Prerequisites: Rust ≥ 1.88, Node ≥ 22, and Tauri's system dependencies (link `https://v2.tauri.app/start/prerequisites/`).
  - Commands: `npm --prefix app install`, `npm --prefix app run tauri dev`, and the Step 3 check line.
- **Data**: stored locally in the app data directory (`io.github.gsimei.soapstone`).
- **License**: MIT.

- [ ] **Step 7: Stage**

```bash
git add -A
git status --short   # the Optitravel fixture must not appear
```
