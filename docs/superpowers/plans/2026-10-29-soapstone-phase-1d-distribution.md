# Soapstone Phase 1d — SoapUI Import and Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A SoapUI `.xml` project becomes a Soapstone project, offline, with its services, saved requests, credentials and properties, plus an import report. GitHub Actions workflows build the installers for macOS (Apple Silicon and Intel), Windows and Linux, and the README explains how to install an unsigned build.

**Architecture:**
- `soapstone-store` gains `soapui.rs`: a pure parser (`parse_soapui`) from the project XML to a plain `SoapuiProject` value, and `Store::import_soapui`, which persists it in one transaction.
  - Services come from each interface's `con:definitionCache`, through the existing `Definition::parse`. No network.
  - Saved calls replace an operation's generated "Request 1".
- One Tauri command and one sidebar menu item wire it in.
- Distribution is configuration: `.github/workflows/ci.yml` (tests on the three systems) and `.github/workflows/release.yml` (installers with `tauri-apps/tauri-action`, into a draft release). Locally, `tauri build` produces the macOS `.app` and `.dmg` for this machine's architecture.

**Tech Stack:** As 1c. No new crates. GitHub Actions: `actions/checkout@v4`, `actions/setup-node@v4`, `dtolnay/rust-toolchain@stable`, `swatinem/rust-cache@v2`, `tauri-apps/tauri-action@v0`.

**Spec:** `docs/superpowers/specs/2026-10-29-cliente-soap-design.md`.
- Scope is §9 row 1d: "Importar projeto SoapUI; GitHub Actions gerando `.dmg` (Apple Silicon + Intel), `.msi`/`.exe`, `.AppImage`/`.deb`; README com instruções de instalação sem assinatura".
- It implements §3 items 6 and 7, §7.5 (the project format) and §8.5 (the import rules).
- Earlier plans: `2026-10-29-soapstone-phase-1a-core.md`, `-1b-app.md`, `-1c-auth.md`.

## Global Constraints

- Rust edition 2024, rust-version 1.88; MIT; UI, code, commits and README in English.
- Nothing is pushed, tagged or published. The workflows are files only. Releases are drafts and run only on a `v*` tag or by hand (`workflow_dispatch`).
- No code signing or notarization (§4: "app sai sem assinatura"). No secrets in the workflows beyond the automatic `GITHUB_TOKEN`.
- Installing anything new (Rust targets, system packages) needs the user's OK first. The local build covers this Mac's architecture only. Intel and the other systems are built by the workflow.
- The user's real SoapUI projects (`~/Optgest-soapui-project.xml`, `~/OPTGEST-WWW2-soapui-project.xml`) are read only for the acceptance check. They are never copied into the repo, and their values are never printed. Test fixtures are written by hand.
- SoapUI is EUPL: no code or files copied from it. The fixture is our own XML in its documented format (§7.5).
- §8.5:
  - Properties become base variables.
  - `${#Project#name}` becomes `{{name}}` in every field.
  - Other expansions (`${#TestCase#…}`, `${#Global#…}`, `${=…}`) stay as they are and are listed in the report.
  - Test suites and mock services are ignored with "N test suites ignored; they arrive in Stage 2".

## Review Focus

1. **Expansions that cannot become variables:** a property name with a space or other characters outside `[A-Za-z_][\w.-]*`, and `${#Project#x}` where `x` is not a property. Expected: the text stays unchanged and the report names the request; such a property is left out of the base variables with a note. → Task 1 `expansions_convert_or_are_reported`.
2. **Interfaces that cannot be imported:** REST interfaces, a WSDL interface without `definitionCache`, a cache that does not parse. Expected: each is skipped with a note, and the rest of the project still imports. → Task 2 `bad_interfaces_are_skipped_and_reported`.
3. **Calls that do not match an operation** (renamed in a newer WSDL): a request needs an operation, so the call is not imported and the report names it. → Task 2 `calls_for_unknown_operations_are_reported`.
4. **Credentials:** `authType` Basic, Preemptive, "Global HTTP Settings" with a username, NTLM (with domain), and OAuth/Kerberos (ignored, reported); `wssPasswordType` PasswordText/PasswordDigest becomes a UsernameToken with the call's username and password. → Task 1.
5. **Request headers stored as an escaped XML fragment** in `con:setting id="…@request-headers"`, including values with `${#Project#…}` and XML entities. → Task 1.

---

### Task 1: Parse a SoapUI project

**Files:**
- Create: `crates/soapstone-store/src/soapui.rs`, `crates/soapstone-store/tests/soapui.rs`, `crates/soapstone-store/tests/fixtures/soapui/sample-soapui-project.xml`
- Modify: `crates/soapstone-store/src/lib.rs` (module and re-exports)

**Interfaces:**
- Produces (all `Debug, Clone, PartialEq`):
  - `pub struct SoapuiProject { pub name: String, pub properties: Vec<Variable>, pub interfaces: Vec<SoapuiInterface>, pub notes: Vec<String> }`
  - `pub struct SoapuiInterface { pub name: String, pub binding: String /* local name */, pub definition: String, pub documents: Vec<Document> /* root first; empty when not cached */, pub calls: Vec<SoapuiCall> }`
  - `pub struct SoapuiCall { pub operation: String, pub name: String, pub endpoint: String, pub body: String, pub headers: Vec<Header>, pub auth: Auth, pub ws_security: WsSecurity }`
  - `pub fn parse_soapui(xml: &str) -> Result<SoapuiProject, String>`: `Err` only when the text is not XML or its root is not `{http://eviware.com/soapui/config}soapui-project`.

Rules:
- `con:interface` with `xsi:type` ending in `WsdlInterface` (or no type and a `bindingName`) is a WSDL interface. Others are skipped with the note `Skipped interface "<name>": only WSDL (SOAP) interfaces are imported.`
- `bindingName` `{ns}Local` gives `Local`.
- `definitionCache`: parts in file order with the part whose `con:url` equals `rootPart` moved first; `Document { url, content }` from `con:url` and `con:content`.
- Calls: `con:operation/@name` and `con:call/@name`, `con:endpoint`, `con:request`.
- Headers: the text of `con:setting` whose `id` ends in `@request-headers` is parsed as XML; each `entry` element gives `Header { name: @key, value: @value }`.
- `con:credentials`, by `con:authType`:
  - `Basic` or `Preemptive`, or `Global HTTP Settings` / missing with a non-empty username: `Auth::Basic`.
  - `NTLM`: `Auth::Ntlm` with `con:domain`.
  - `No Authorization`, or no username: `Auth::None`.
  - Anything else (OAuth 2.0, SPNEGO/Kerberos…): `Auth::None` and the note `"<operation> › <call>": <authType> authentication is not supported; imported without it.`
- `@wssPasswordType` `PasswordText` / `PasswordDigest`: `WsSecurity { username_token: true, username, password, password_type, ..Default::default() }` with the credentials' username and password.
- Expansions, in every string field (endpoint, body, header names and values, usernames, passwords, domain):
  - `${#Project#name}` with a valid variable name (`[A-Za-z_][A-Za-z0-9_.-]*`) that is one of the properties becomes `{{name}}`.
  - Any other `${…}` stays, and adds one note per call: `"<operation> › <call>" keeps SoapUI expressions Soapstone cannot resolve: ${#TestCase#x}, …` (distinct, in order).
- Properties: `con:properties/con:property` name/value. Names that are not valid variable names are left out with a note.
- `con:testSuite` count N > 0 adds `N test suites ignored; they arrive in Stage 2.`, `con:mockService` likewise (`mock services`). An outgoing WS-Security configuration (`con:wssContainer/con:outgoing`) or `@sslKeystore` adds one note each that it was not imported.

- [ ] **Step 1: Write the fixture and the failing tests**

The fixture is our own XML: project "Sample" with
- properties `base = http://localhost:8080`, `user = bob`, `my prop = x`;
- a WSDL interface `{http://tempuri.org/}CalculatorSoap` whose `definitionCache` holds `crates/soapstone-core/tests/fixtures/calculator/calculator.wsdl` (CDATA, `rootPart` = its URL);
- operation `Add` with calls `Basic one` (endpoint `${#Project#base}/calc`, Basic `${#Project#user}`/`pw`, a request-headers setting with `X-Trace = ${#Project#user}` and `X-Amp = a&b`, body with `${#TestCase#id}`) and `NTLM one` (NTLM `u`/`p`/`CORP`, `wssPasswordType="PasswordDigest"`);
- operation `Subtract` with call `OAuth one` (authType `OAuth 2.0`);
- a REST interface, one test suite, one mock service.

Tests in `tests/soapui.rs`:
- `wsdl_interfaces_calls_and_properties_are_read`: name "Sample"; properties `base`, `user`; one interface, binding `CalculatorSoap`, documents[0].url equals the root part's URL and its content parses with `Definition::parse`; three calls with the right operation names.
- `credentials_headers_and_ws_security_map`: `Basic one` has `Auth::Basic { username: "{{user}}", password: "pw" }` and headers `[X-Trace = {{user}}, X-Amp = a&b]`; `NTLM one` has `Auth::Ntlm { domain: "CORP", .. }` and a digest UsernameToken for `u`; `OAuth one` has `Auth::None`.
- `expansions_convert_or_are_reported`: endpoint `{{base}}/calc`; the body still has `${#TestCase#id}`; the notes contain the `${#TestCase#id}` line, the OAuth line, `my prop`, `Skipped interface`, `1 test suites ignored` and `1 mock services ignored`.
- `not_a_soapui_project_is_an_error`: `<a/>` and `not xml` give `Err`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-store --test soapui`
Expected: FAIL to compile, because `parse_soapui` is not defined.

- [ ] **Step 3: Implement `parse_soapui` with roxmltree**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p soapstone-store --test soapui`
Expected: PASS.

- [ ] **Step 5: Stage**

---

### Task 2: Persist a SoapUI project

**Files:**
- Modify: `crates/soapstone-store/src/soapui.rs`, `crates/soapstone-store/src/services.rs` (only to share the insert helpers), `crates/soapstone-store/tests/soapui.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - `pub struct ImportReport { pub project_id: i64, pub services: usize, pub requests: usize, pub notes: Vec<String> }` (Serialize, camelCase).
  - `Store::import_soapui(&mut self, project: SoapuiProject) -> Result<ImportReport>`:
    - One transaction: a new project named after the SoapUI project, its properties as base variables (not secret), then each interface with documents as a service (source URL = the interface's `definition`, default fetch settings).
    - Operations whose `(binding, name)` has calls get those calls as requests, in order, instead of "Request 1". Endpoint, body, headers, auth and WS-Security come from the call. Operations without calls keep "Request 1".
    - An interface without documents, or whose documents fail `Definition::parse`, is skipped with the note `Skipped interface "<name>": <reason>.`
    - A call whose operation is not in the WSDL adds the note `"<operation> › <call>" was not imported: the WSDL has no operation "<operation>".`
    - Notes are the parser's, then these.

- [ ] **Step 1: Write the failing tests**

- `soapui_project_becomes_a_project`: import the fixture. The new project is named "Sample"; `variables()` has `base` and `user`; the tree has one service "Calculator" with `Add` holding `Basic one`, `NTLM one` and `Subtract` holding `OAuth one`; `Multiply` still has "Request 1"; `prepare_send` of `Basic one` renders with the variables; the report counts 1 service and 3 + the remaining generated requests.
- `bad_interfaces_are_skipped_and_reported`: a project with a WSDL interface without a cache and one whose cache is `<html/>`, plus the good one: Ok, one service, and two `Skipped interface` notes.
- `calls_for_unknown_operations_are_reported`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-store --test soapui`
Expected: FAIL to compile, because `import_soapui` is not defined.

- [ ] **Step 3: Implement**, reusing `insert_operation`, `service_name`, `documents_json` and `unique_operations` from `services.rs` (made `pub(crate)`).

- [ ] **Step 4: Run the store tests and clippy**

Run: `cargo test -p soapstone-store && cargo clippy -p soapstone-store --all-targets -- -D warnings`
Expected: PASS; clean.

- [ ] **Step 5: Acceptance against the user's real projects (not committed)**

Run an `#[ignore]`d test `users_real_projects_import` that imports each of `~/Optgest-soapui-project.xml` and `~/OPTGEST-WWW2-soapui-project.xml` when present and prints only counts (services, requests, notes) and the import time.
Expected: both import with one service and every call as a request; time well under 2 s.

- [ ] **Step 6: Stage**

---

### Task 3: Import from the app

**Files:**
- Modify: `app/src-tauri/src/commands.rs`, `app/src-tauri/src/main.rs`, `app/src/api.ts`, `app/src/Sidebar.tsx`

**Interfaces:**
- Produces:
  - Command `import_soapui_project(path: String) -> ImportReport`: reads the file, parses and imports inside `blocking` and `on_big_stack`, logs `metric soapui-import`.
  - `api.importSoapuiProject(path) -> Promise<ImportReport>`.
  - The project menu gains "Import SoapUI project…" after "New project": it opens `open()` with the filter `SoapUI project` (`xml`), imports, selects the new project, and shows the report with `message()` (kind `info`, or `warning` when there are notes): `Imported <services> services and <requests> requests.` followed by the notes, one per line.

- [ ] **Step 1: Implement**
- [ ] **Step 2: Check it in the mock harness**: the menu item cannot be clicked in a browser, so check the handler by calling it from the harness: `import_soapui_project` receives the path, the report dialog text is right, and the new project is selected.
- [ ] **Step 3: Run** `npm --prefix app test && npm --prefix app run build && cargo clippy --workspace --all-targets -- -D warnings`. Expected: green.
- [ ] **Step 4: Stage**

---

### Task 4: Workflows, local installer and README

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/workflows/release.yml`
- Modify: `crates/soapstone-http/tests/send.rs` (deferred minor #18), `README.md`

- [ ] **Step 1: Make the connection-refused test portable**

The test accepts both "connection refused" (macOS, Linux) and "actively refused" (Windows), lower-cased. Run `cargo test -p soapstone-http --test send`. Expected: PASS.

- [ ] **Step 2: `ci.yml`**

On `push` and `pull_request`. Matrix `macos-latest`, `windows-latest`, `ubuntu-22.04`. Steps: checkout; Linux packages (`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`); Node 22 with npm cache; stable Rust with clippy; rust-cache; `npm --prefix app ci`; `npm --prefix app test`; `npm --prefix app run build`; `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 3: `release.yml`**

On `push` of tags `v*` and `workflow_dispatch`. `permissions: contents: write`. Matrix:
- `macos-latest` with `--target aarch64-apple-darwin`
- `macos-latest` with `--target x86_64-apple-darwin` (adds that Rust target)
- `windows-latest`
- `ubuntu-22.04`

Each runs `tauri-apps/tauri-action@v0` with `projectPath: app`, `tagName: v__VERSION__`, `releaseName: Soapstone v__VERSION__`, `releaseDraft: true`, `prerelease: false` and the README's install notes as `releaseBody`.

Check both files parse as YAML (`python3 -c 'import yaml…'` if PyYAML is available, else `ruby -ryaml`).

- [ ] **Step 4: Local installer**

Run: `PATH="$HOME/.cargo/bin:$PATH" npm --prefix app run tauri build`
Expected: `target/release/bundle/macos/Soapstone.app` and `target/release/bundle/dmg/Soapstone_0.1.0_aarch64.dmg`. Record their sizes, launch the `.app` twice and record `metric startup` (target < 1 s warm).

- [ ] **Step 5: README**

- "Install" section: download from GitHub Releases; per system, how to open an unsigned build:
  - macOS: open the `.dmg`, drag to Applications, then right-click → Open the first time (or `xattr -dr com.apple.quarantine /Applications/Soapstone.app`).
  - Windows: `.msi` or `-setup.exe`; SmartScreen → "More info" → "Run anyway".
  - Linux: `.deb` (`sudo apt install ./Soapstone_*.deb`) or `.AppImage` (`chmod +x`, then run).
- "Import from SoapUI" section: what is imported and what is reported (§8.5).
- "Releasing" section: bump the version in `app/src-tauri/tauri.conf.json`, `app/package.json` and `app/src-tauri/Cargo.toml`, push a `v<version>` tag, then publish the draft release.
- Status line becomes "Phase 1d".

- [ ] **Step 6: Full suite**

Run: `npm --prefix app test && npm --prefix app run build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all green.

- [ ] **Step 7: Stage**

---

## Scope Decisions

- Interfaces without a `definitionCache` are skipped, not downloaded: §8.5 says the import "funciona offline, sem baixar nada". The user can import that WSDL by URL afterwards.
- SoapUI's project-level WS-Security configurations (`wssContainer`), keystores and test suites are reported, not imported (§4, §8.5).
- No Intel build locally: it would need `rustup target add x86_64-apple-darwin`, which is an install. The release workflow builds it.
