# Soapstone Phase 1c — Authentication Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Soapstone sends requests with a client certificate (`.p12`/`.pfx`), WS-Security UsernameToken (text or digest) and Timestamp, and NTLM. Secret variables are kept in the system keychain. Each request has its own timeout and an "ignore TLS certificate errors" switch. WSDLs can be downloaded from servers that need Basic auth or have untrusted certificates.

**Architecture:** Most of the work lands in `soapstone-http`:
- `wss.rs` inserts `<wsse:Security>` into the rendered envelope.
- `ntlm.rs` builds the NTLM messages.
- `send()` picks a dedicated `reqwest` client whenever a request needs its own TLS identity, relaxed certificate checks or one keep-alive connection for NTLM.

`soapstone-store` gains schema version 2: two JSON columns per request and one per service. It also gets a `Vault` that keeps each project's secret values in one keychain entry. `soapstone-core` gets one additive function, `fetch_all_with`. The app and the UI only wire these in.

**Tech Stack:** As 1b, plus:
- `ntlmclient 0.2` (NTLMv2 messages, CC0)
- `p12-keystore 0.4` (PKCS#12, including legacy 3DES/RC2)
- `keyring 4.2` (feature `v1`: macOS Keychain, Windows Credential Manager, Secret Service)
- `sha1 0.11`, `base64 0.23`, `chrono 0.4`, `getrandom 0.4`
- Dev-only: `rustls 0.23` (local TLS test servers), `md4 0.10`, `hmac 0.12`, `md-5 0.10` (an independent NTLMv2 check)

**Spec:** `docs/superpowers/specs/2026-10-29-cliente-soap-design.md`.
- Scope is §9 row 1c: "Certificado cliente, WS-Security UsernameToken/Timestamp, NTLM, variáveis secretas no cofre", validated by "Testes contra servidores locais".
- This plan implements §6 (keyring, all networking in Rust), §8.2 (secret variables), §8.3 (send sequence), §8.4 (the auth table, ignore-TLS option, timeout), §8.6 (WS-Security tab) and §10 ("UsernameToken digest com nonce/created fixos").
- Earlier plans:
  - `docs/superpowers/plans/2026-10-29-soapstone-phase-1a-core.md`
  - `docs/superpowers/plans/2026-10-29-soapstone-phase-1b-app.md`, whose Scope Decisions moved the timeout, ignore-TLS and WSDL-auth items here.

## Global Constraints

**Platform and dependencies**
- Workspace and crate settings are unchanged from 1b: edition 2024, `rust-version = "1.88"`, MIT. No new dependency may raise the MSRV.
- TLS is rustls only (reqwest 0.13 default, aws-lc-rs). No `native-tls`, no OpenSSL at runtime.
- Every `reqwest` client the app builds uses the same base as `client()`: `User-Agent: Soapstone/<version>`, redirects returned rather than followed (`Policy::none()`), and the system proxy plus `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` (reqwest default, spec §8.4).
- `crates/soapstone-core` gets only the additive `FetchOptions` / `fetch_all_with` change in Task 4. The 1b rule "do not edit core" existed because 1a was under review; 1a is now merged (`8ade0de`).

**Send sequence (spec §8.3)**
1. Render `{{variables}}` in this order: endpoint, body, headers, auth, WS-Security, certificate.
2. Insert `<wsse:Security>` into the SOAP Header.
3. Build the HTTP request: Content-Type, SOAPAction, user headers.
4. Apply HTTP auth: Basic, Bearer, NTLM, client certificate.
5. Send.

Fields that are switched off are not rendered, so their undefined variables never block a send: WS-Security fields when both UsernameToken and Timestamp are off, and the certificate password when the certificate path is empty.

**WS-Security (spec §7.4, §8.4)**
- Namespaces and URIs, exact:
  - `wsse` = `http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd`
  - `wsu` = `http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd`
  - PasswordText type: `http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordText`
  - PasswordDigest type: `…#PasswordDigest` (same prefix)
  - Nonce EncodingType: `http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary`
- Digest = `Base64(SHA-1(nonce bytes + created text + password))`. Nonce and Created are each optional; a missing one is left out of the hash.
- Nonce = 16 random bytes. Created and Expires are UTC with milliseconds: `2026-10-29T12:00:00.000Z`. Expires = Created + TTL. TTL defaults to 300 s.
- The Security element carries `mustUnderstand`: `"1"` for SOAP 1.1, `"true"` for 1.2.
- Element order: Timestamp first, then UsernameToken. No `wsu:Id`.

**NTLM (spec §8.4, §11 risk 3)**
- NTLMv2, scheme `NTLM`.
- One dedicated HTTP/1.1 client per send, with `pool_max_idle_per_host(1)`, so all three handshake legs share one TCP connection.
- The 401 body is read in full before the second leg, so the connection goes back to the pool.

**Request settings**
- Timeout defaults to 60 s and is set per request through `RequestBuilder::timeout`. 0 counts as 1.
- "Ignore TLS certificate errors" is per request (spec §8.4) and per WSDL source.

**Secret variables (spec §6, §8.2)**
- Keychain service `io.github.gsimei.soapstone`, account `project-<id>`. There is one entry per project, whose value is JSON `{"<environment name, \"\" for base>": {"<variable>": "<value>"}}`.
- While the keychain works, a secret's value is never written to SQLite; there its value is `""`.
- When the keychain is unavailable, the value is saved in SQLite instead, with the warning: `The system keychain is unavailable (<error>). Secret values were saved in the local database instead.`
- Secret values are shown masked (`type="password"`).
- Tests never touch the real keychain. `Store::open_in_memory()` uses `Vault::memory()`.

**WSDL downloads**
- Basic credentials are sent preemptively, and only to URLs with the root document's origin (scheme, host, port), including the `?wsdl` retry.
- The WSDL username and password may use `{{variables}}`, rendered with the project's variables at import and at refresh. They are stored as typed, like request auth fields.

**Code and UI conventions**
- Inputs: every text `<input>` spreads `{...plain}` from `logic.ts`. `app/src/inputs.test.ts` enforces this.
- UI text in English. Code, comments and commit messages in English.
- Commits: the user commits. Steps marked "Stage" run `git add -A -- . ':!app/mock.html' ':!app/mock.ts'`. Never bypass signing.
- Run `npm --prefix app run build` before compiling the app crate. Cargo and git need the sandbox disabled.

## Scope Decisions

Mine, reviewable:
- **NTLM library:** `ntlmclient` instead of the spec table's `sspi`. `sspi 0.23` needs Rust 1.93 (the workspace is on 1.88) and pulls Kerberos/PKI crates pinned to pre-releases (`picky =7.0.0-rc.27`, `rsa =0.10.0-rc.18`). `ntlmclient` builds NTLMv2 with a few small RustCrypto crates. If a real server rejects its AUTHENTICATE message (no MIC), only `ntlm.rs` changes.
- **PKCS#12:** `p12-keystore` converts the `.p12`/`.pfx` to PEM for `reqwest::Identity::from_pem` (rustls). This avoids a second TLS stack: native-tls would need OpenSSL on Linux.
- **UI layout:**
  - The Auth tab holds the HTTP auth type (None, Basic, Bearer, NTLM) and, below it, a "Client certificate" section (file + password), because a certificate combines with any auth type.
  - A new **Settings** tab holds the timeout and "Ignore TLS certificate errors".
  - Tab order: Body, Headers, Auth, WS-Security, Settings.
- **Switching the active environment** gets its own command, which changes only `active`, so it can never touch secrets. The environment dialog loads the environments with secret values revealed. The rest of the app loads them with secret values blanked, which also keeps the keychain out of startup.
- **Service menu "Edit WSDL source…"** opens the import dialog filled in with the URL and download options; its button is "Save and refresh". "Refresh WSDL" stays a one-click refresh with the stored source. Without this, a changed WSDL password could only be fixed by deleting the service and its requests.
- **Not in 1c:**
  - NTLM for WSDL downloads (Basic only).
  - `Negotiate`/Kerberos (spec §4).
  - `DOMAIN\user` splitting (Domain is its own field).
  - Showing the request as sent (with the generated Security header).
  - A keychain read cache.
  - JKS keystores (spec §4: README explains `keytool`).

## Review Focus

1. **WS-Security on real envelopes:**
   - SoapUI's `<soapenv:Header/>`, no Header at all, a Header that already has children, an Envelope in the default namespace (no prefix), and SOAP 1.2.
   - Expected: the Security block lands as the Header's first child with a correctly prefixed `mustUnderstand`.
   - → Task 1 tests.
2. **Client certificate files:**
   - A legacy-encrypted `.pfx` (3DES/RC2, as exported by Windows, Java and OpenSSL 1.x), a wrong password, a missing file.
   - Expected: the legacy file works; the other two give a readable error naming the file, and nothing is sent.
   - → Task 2 tests.
3. **Keychain unavailable or denied:**
   - On save, secrets fall back to SQLite with a warning.
   - A failed read is an error, never a silently empty password.
   - Switching the active environment never erases secrets.
   - → Task 6 tests.
4. **WSDL credentials and other hosts:** a WSDL that imports a schema from another host must not receive the Basic credentials. → Task 4 `basic_credentials_go_only_to_the_wsdl_origin`.
5. **NTLM server that does not challenge** (it answers 200, or 401 offering only `Basic`/`Negotiate`): its response is shown as is, with no hang and no crash. → Task 3 `ntlm_without_a_challenge_returns_the_server_response`.

---

## File Structure

```
crates/soapstone-http/
  Cargo.toml                 + ntlmclient, p12-keystore, sha1, base64, chrono, getrandom; dev: rustls, md4, hmac, md-5
  src/lib.rs                 Auth::Ntlm, WsSecurity, PasswordType, RequestOptions; SoapCall fields; SendError variants
  src/wss.rs                 apply(): <wsse:Security> into the envelope (new)
  src/ntlm.rs                negotiate(), challenge(), authenticate() (new)
  src/send.rs                base builder, dedicated clients, identity loading, NTLM exchange, timeout
  tests/send.rs              + WS-Security end to end, timeout
  tests/tls.rs               ignore-TLS and client certificate against a local rustls server (new)
  tests/ntlm.rs              NTLM against a local one-connection server (new)
  tests/fixtures/tls/        test-only CA, server cert/key, client .p12 (AES and legacy) + README (new)
crates/soapstone-core/
  src/loader.rs              FetchOptions, fetch_all_with
  src/lib.rs                 re-export
  tests/common/mod.rs        serve_recording(), serve_tls()
  tests/loader.rs            + credentials/origin and insecure tests
crates/soapstone-store/
  Cargo.toml                 + keyring
  src/lib.rs                 migration 2, Variable.secret, Vault, environments(reveal), set_active_environment
  src/vault.rs               Vault: System | Memory | Unavailable (new)
  src/services.rs            Request/RequestEdit ws_security + options, FetchSettings, ServiceSource, fetch_options
  tests/store.rs             + migration test
  tests/services.rs          + options round trip, fetch settings
  tests/secrets.rs           vault behavior (new)
app/src-tauri/src/commands.rs   import/refresh with fetch settings, get_service_source, environments commands
app/src-tauri/src/main.rs       register new commands
app/src/api.ts               new types and wrappers
app/src/RequestView.tsx      Auth (NTLM, certificate), WS-Security and Settings tabs
app/src/KeyValueTable.tsx    optional per-row "Secret" toggle
app/src/EnvironmentDialog.tsx   secret toggle, save warning
app/src/ImportDialog.tsx     download options; edit-source mode
app/src/Sidebar.tsx          "Edit WSDL source…"
app/src/App.tsx              active switch via set_active_environment; dialog loads revealed environments
README.md                    what works, JKS conversion, NTLM/cert notes
```

---

### Task 1: `soapstone-http` — WS-Security UsernameToken and Timestamp

**Files:**
- Create: `crates/soapstone-http/src/wss.rs`
- Modify: `crates/soapstone-http/src/lib.rs`, `crates/soapstone-http/src/send.rs`, `crates/soapstone-http/Cargo.toml`
- Test: unit tests in `wss.rs`; `crates/soapstone-http/tests/send.rs`

**Interfaces:**
- Consumes: `render`, `send` and `SoapCall` from 1b.
- Produces:
  - `pub enum PasswordType { #[default] Text, Digest }`: serde lowercase.
  - `WsSecurity` (Clone, PartialEq, Eq, Debug, Serialize, Deserialize, `#[serde(rename_all = "camelCase", default)]`), with fields:
    - `username_token: bool` — default false
    - `username: String`
    - `password: String`
    - `password_type: PasswordType`
    - `nonce: bool` — default true
    - `created: bool` — default true
    - `timestamp: bool` — default false
    - `ttl_seconds: u32` — default 300
  - `WsSecurity::enabled(&self) -> bool`: `username_token || timestamp`.
  - `SoapCall.ws_security: WsSecurity`.
  - `SendError::WsSecurity(String)`, displayed as `WS-Security: {0}`.
  - `pub(crate) fn wss::apply(envelope: &str, wss: &WsSecurity, now: chrono::DateTime<chrono::Utc>, nonce: [u8; 16]) -> Result<String, String>`.

- [ ] **Step 1: Write the failing unit tests in `wss.rs`**

Fixed inputs:
- `now` = `2026-10-29T12:00:00Z`
- `nonce` = `[0, 1, …, 15]`
- `E11` = `<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:tem="http://tempuri.org/"><soapenv:Header/><soapenv:Body><tem:Add/></soapenv:Body></soapenv:Envelope>`
- `full` = UsernameToken on, `alice` / `secret`, Digest, nonce and created on, Timestamp on, TTL 300.

The expected values were computed with Python's `hashlib`/`base64` as an independent oracle:

```rust
#[test]
fn digest_token_and_timestamp_match_the_profile() {
    let out = apply(E11, &full(), now(), NONCE).unwrap();
    assert_eq!(out, concat!(
        r#"<soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:tem="http://tempuri.org/"><soapenv:Header>"#,
        r#"<wsse:Security soapenv:mustUnderstand="1" xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">"#,
        r#"<wsu:Timestamp><wsu:Created>2026-10-29T12:00:00.000Z</wsu:Created><wsu:Expires>2026-10-29T12:05:00.000Z</wsu:Expires></wsu:Timestamp>"#,
        r#"<wsse:UsernameToken><wsse:Username>alice</wsse:Username>"#,
        r#"<wsse:Password Type="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest">RnwQC+GBXPoduhcjUk+3fhZrHfA=</wsse:Password>"#,
        r#"<wsse:Nonce EncodingType="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary">AAECAwQFBgcICQoLDA0ODw==</wsse:Nonce>"#,
        r#"<wsu:Created>2026-10-29T12:00:00.000Z</wsu:Created></wsse:UsernameToken></wsse:Security>"#,
        r#"</soapenv:Header><soapenv:Body><tem:Add/></soapenv:Body></soapenv:Envelope>"#));
}
```

Further tests, assertions only:

- `digest_leaves_out_a_missing_nonce`: nonce off, created on. The password is `qjlu86mTo+hE8l3DVgMLssW5X6M=`, and the output contains no `<wsse:Nonce`.
- `text_password_without_nonce_or_created`: Text, nonce and created off, Timestamp off. The output contains `…#PasswordText">secret</wsse:Password></wsse:UsernameToken>` and no `<wsu:`.
- `missing_header_is_created_before_the_body`: the input is `E11` without `<soapenv:Header/>`. The output contains `<soapenv:Header><wsse:Security` immediately before `<soapenv:Body>`.
- `existing_header_children_follow_the_security_block`: the Header is `<soapenv:Header><a:Trace xmlns:a="urn:a">1</a:Trace></soapenv:Header>`. The output contains `</wsse:Security><a:Trace`.
- `must_understand_follows_version_and_prefix`:
  - A SOAP 1.2 envelope (`xmlns:env="http://www.w3.org/2003/05/soap-envelope"`) gives `env:mustUnderstand="true"`.
  - An Envelope in the default namespace (`<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/">…`) gives `<wsse:Security xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" soapenv:mustUnderstand="1" …` and a created `<Header>` with no prefix.
- `username_and_password_are_escaped`: `a&b` / `<p>` become `<wsse:Username>a&amp;b</wsse:Username>` and `>&lt;p&gt;<` (Text).
- `body_that_is_not_an_envelope_is_rejected`:
  - `"<x/>"` gives `Err("the body is not a SOAP Envelope")`.
  - `"oops"` gives an `Err` starting with `"the body is not well-formed XML"`.
  - An Envelope without a Body gives `Err("the SOAP Envelope has no Body")`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-http --lib wss`
Expected: FAIL to compile, because `apply` and `WsSecurity` are not defined.

- [ ] **Step 3: Implement the types in `lib.rs` and `wss::apply`**

1. Parse with `roxmltree`. The root must be `Envelope` in the SOAP 1.1 or 1.2 namespace.
2. Read the Envelope's qualified-name prefix from the source text at `root.range().start`.
3. Find the Header and Body children (same namespace).
   - When the Header exists, insert after the end of its start tag. Find that end with a quote-aware scan for `>`. A self-closing `<p:Header/>` becomes `<p:Header>…</p:Header>`.
   - When there is no Header, insert `<p:Header>…</p:Header>` at the start of the Body.
4. Escape text with `quick_xml::escape::escape`.
5. Format time with chrono, `%Y-%m-%dT%H:%M:%S%.3fZ`.

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test -p soapstone-http --lib wss`
Expected: PASS, 8 tests.

- [ ] **Step 5: Write the failing end-to-end test in `tests/send.rs`**

Add `ws_security: WsSecurity::default()` to the `call()` helper. Then:

```rust
#[tokio::test]
async fn ws_security_digest_is_sent_and_verifies() {
    let envelope = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Header/><s:Body><x/></s:Body></s:Envelope>"#;
    let (url, rx) = serve_once(http("200 OK", &[], b"<ok/>"));
    let mut c = call(&url, SoapVersion::V11, None);
    c.body = envelope.into();
    c.ws_security = WsSecurity { username_token: true, username: "{{u}}".into(), password: "{{p}}".into(),
        password_type: PasswordType::Digest, timestamp: true, ttl_seconds: 60, ..Default::default() };
    send(&client(), &c, &vars(&[("u", "alice"), ("p", "s3cret")])).await.unwrap();
    let raw = received(rx);
    // Recompute Base64(SHA-1(nonce + created + password)) from the sent Nonce and Created.
    let (nonce, created, digest) = (between(&raw, "<wsse:Nonce", "</wsse:Nonce>"), between(&raw, "<wsse:UsernameToken>", "</wsse:UsernameToken>"), ..);
    assert_eq!(digest, expected_digest(&nonce, &created, "s3cret"));
    assert!(raw.contains("<wsse:Username>alice</wsse:Username>"));
    // Expires - Created == 60 s
}
```

`between` and `expected_digest` are small helpers in the test: they extract element text and use `sha1` and `base64`. Add one more test:

- `disabled_ws_security_fields_never_block_a_send`: WS-Security is off with `username: "{{nope}}"`, and the body is `"<x/>"` (not an envelope). The send returns `Ok`, and the body arrives unchanged.

- [ ] **Step 6: Run them to see them fail**

Run: `cargo test -p soapstone-http --test send ws_security`
Expected: FAIL, because `send` ignores `ws_security`.

- [ ] **Step 7: Wire it into `send`**

- Render `username` and `password` only when `enabled()`, after auth in the missing-variable order.
- After rendering, when `enabled()`, set `body = wss::apply(&body, &wss, Utc::now(), nonce).map_err(SendError::WsSecurity)?`.
- The nonce comes from `getrandom::fill`.

- [ ] **Step 8: Run the crate's tests**

Run: `cargo test -p soapstone-http`
Expected: PASS, all tests (1b's plus the 10 new ones).

- [ ] **Step 9: Stage**

---

### Task 2: `soapstone-http` — timeout, ignore TLS errors, client certificate

**Files:**
- Create: `crates/soapstone-http/tests/tls.rs`, `crates/soapstone-http/tests/fixtures/tls/{ca.pem,server.pem,server.key,client.p12,client-legacy.p12,README.md}`
- Modify: `crates/soapstone-http/src/lib.rs`, `crates/soapstone-http/src/send.rs`, `crates/soapstone-http/Cargo.toml`, `crates/soapstone-http/tests/send.rs`

**Interfaces:**
- Consumes: Task 1's `SoapCall` and `send`.
- Produces:
  - `RequestOptions` (Clone, PartialEq, Eq, Debug, Serialize, Deserialize, `#[serde(rename_all = "camelCase", default)]`), with fields:
    - `timeout_seconds: u32` — default 60
    - `insecure: bool`
    - `cert_path: String`
    - `cert_password: String`
  - `SoapCall.options: RequestOptions`.
  - `SendError::Certificate(String)`, displayed as `Client certificate: {0}`.
  - `fn base() -> reqwest::ClientBuilder` in `send.rs` (UA, `Policy::none()`, 60 s timeout). `client()` becomes `base().build()`.

- [ ] **Step 1: Generate the test fixtures**

Use Homebrew OpenSSL 3 (`/opt/homebrew/bin/openssl`). Validity is 100 years, so the tests never expire:

```bash
cd crates/soapstone-http/tests/fixtures/tls
openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.pem -days 36500 -subj "/CN=Soapstone Test CA"
openssl req -newkey rsa:2048 -nodes -keyout server.key -out server.csr -subj "/CN=localhost"
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out server.pem -days 36500 \
  -extfile <(printf "subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth")
openssl req -newkey rsa:2048 -nodes -keyout client.key -out client.csr -subj "/CN=Soapstone Test Client"
openssl x509 -req -in client.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out client.pem -days 36500 \
  -extfile <(printf "extendedKeyUsage=clientAuth")
openssl pkcs12 -export -inkey client.key -in client.pem -out client.p12 -passout pass:test
openssl pkcs12 -export -legacy -inkey client.key -in client.pem -out client-legacy.p12 -passout pass:test
rm ca.key client.key client.pem *.csr *.srl
```

Write `README.md`: "Test-only keys for the TLS tests. Never use them elsewhere. Regenerate with the commands in the Phase 1c plan, Task 2."

Expected:
- `openssl pkcs12 -in client-legacy.p12 -info -noout -passin pass:test -legacy` mentions `pbeWithSHA1And3-KeyTripleDES-CBC` or `RC2`.
- The same command on `client.p12` mentions `AES-256-CBC`.

- [ ] **Step 2: Write the failing tests**

In `tests/tls.rs`, `serve_tls(require_client_cert: bool) -> String` (`https://localhost:<port>`) is a std-thread server:
- It uses rustls `ServerConfig::builder_with_provider(aws_lc_rs::default_provider())` with `server.pem`/`server.key`.
- When `require_client_cert` is true, it uses `WebPkiClientVerifier` over `ca.pem`.
- It answers one connection with `200 OK`, body `<ok/>`, and `Connection: close`.

Tests:

- `untrusted_server_certificate_fails_unless_ignored`:
  - With the default options, the result is `Err(SendError::Http(m))`, and `m.to_lowercase()` contains `certificate`.
  - With `insecure: true`, the status is 200.
- `client_certificate_from_p12_is_presented`: run this for `client.p12` and for `client-legacy.p12`, each with password `{{pw}}` (variable `pw` = `test`), `insecure: true`, and a server that requires a client cert. The status is 200.
- `server_requiring_a_certificate_rejects_a_send_without_one`: the result is `Err(SendError::Http(_))`.
- `wrong_password_or_missing_file_is_a_certificate_error`: both cases give `Err(SendError::Certificate(m))` where `m` contains the path, and the server receives nothing.

In `tests/send.rs`:

- `per_request_timeout_applies`: the server accepts and never answers. With `timeout_seconds: 1`, the result is `Err(SendError::Http(m))` within 5 s, with `m.to_lowercase()` containing `timed out`.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p soapstone-http --test tls --test send`
Expected: FAIL to compile, because `options` is not defined.

- [ ] **Step 4: Implement**

- `send` uses the shared client unless `options.insecure`, `!cert_path.is_empty()` or `Auth::Ntlm` (Task 3). In those cases it builds a dedicated client: `base().danger_accept_invalid_certs(insecure)`, plus `.identity(id)` when there is a certificate.
- Every request gets `.timeout(Duration::from_secs(timeout_seconds.max(1).into()))`.
- `fn identity(path: &str, password: &str) -> Result<reqwest::Identity, SendError>` works in four steps, each with its own error:
  1. Read the file. On failure: `cannot read {path}: {e}`.
  2. `KeyStore::from_pkcs12(bytes, password, Pkcs12ImportPolicy::Relaxed)`. On failure: `cannot open {path}: wrong password or not a PKCS#12 file ({e})`.
  3. `private_key_chain()`. On `None`: `{path} has no private key with a certificate`.
  4. Build PEM: `PRIVATE KEY` (PKCS#8 DER) plus each `CERTIFICATE`, base64 wrapped at 64 columns. Then `Identity::from_pem`.
- Render the certificate path and password only when the path is not empty.

- [ ] **Step 5: Run the crate's tests**

Run: `cargo test -p soapstone-http`
Expected: PASS.

- [ ] **Step 6: Stage**

---

### Task 3: `soapstone-http` — NTLM

**Files:**
- Create: `crates/soapstone-http/src/ntlm.rs`, `crates/soapstone-http/tests/ntlm.rs`
- Modify: `crates/soapstone-http/src/lib.rs`, `crates/soapstone-http/src/send.rs`, `crates/soapstone-http/Cargo.toml`

**Interfaces:**
- Consumes: Task 2's dedicated client path.
- Produces:
  - `Auth::Ntlm { username: String, password: String, domain: String }`, with JSON `{"type":"ntlm","username":..,"password":..,"domain":..}`.
  - In `ntlm.rs`:
    - `pub(crate) fn negotiate() -> String`: base64 Type 1.
    - `pub(crate) fn challenge(headers: &HeaderMap) -> Option<Vec<u8>>`: decoded Type 2 from a `WWW-Authenticate: NTLM <b64>` value.
    - `pub(crate) fn authenticate(challenge: &[u8], username: &str, password: &str, domain: &str) -> Result<String, String>`: base64 Type 3.
  - Messages use `ntlmclient`:
    - Negotiate flags: `NEGOTIATE_UNICODE | REQUEST_TARGET | NEGOTIATE_NTLM`.
    - Response: `respond_challenge_ntlm_v2(challenge, target_info, get_ntlm_time(), creds)`.
    - Workstation: `SOAPSTONE`.

- [ ] **Step 1: Write the failing tests in `tests/ntlm.rs`**

`serve_ntlm(first_reply: Option<Vec<u8>>) -> (String, mpsc::Receiver<Vec<String>>)` accepts **one** connection, reads requests with Content-Length bodies, and reports the raw requests over the channel.

When `first_reply` is `None`, it runs the handshake:
1. Request 1 must carry `Authorization: NTLM TlRMTVNTUAAB…` (the Type 1 signature). The server answers `401` with `WWW-Authenticate: NTLM <Type 2>` and a 5-byte body, keep-alive.
   - The Type 2 is built with `ntlmclient::ChallengeMessage`: challenge `[1, 2, 3, 4, 5, 6, 7, 8]`, target name and an `NbDomainName` target-info entry `DOMAIN`, and the terminator.
2. Request 2 is read **from the same stream**. The server answers 200 `<ok/>` with `Connection: close`.

When `first_reply` is `Some`, it is written as the answer to request 1.

Tests (all with `timeout_seconds: 5`, so a client that opens a second connection fails fast instead of hanging):

- `ntlm_handshake_uses_one_connection_and_a_valid_ntlmv2_proof`:
  - The status is 200, and both requests arrived on the one connection, with the POST body sent both times.
  - Parse request 2's Type 3 with `ntlmclient::Message::try_from` and check `user_name == "alice"` and `domain_name == "DOMAIN"`.
  - Verify the NTLMv2 proof independently:
    - `ntowf = HMAC-MD5(MD4(UTF-16LE("s3cret")), UTF-16LE("ALICE" + "DOMAIN"))`
    - `nt_response[..16] == HMAC-MD5(ntowf, [1..=8] ++ nt_response[16..])`
  - The credentials come from variables: `username: "{{u}}"`.
- `ntlm_without_a_challenge_returns_the_server_response`: run with `first_reply` = 200 `<fine/>`, and with 401 + `WWW-Authenticate: Basic realm="x"` and `Connection: close`. The send returns `Ok` with that status, and only one request was made.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-http --test ntlm`
Expected: FAIL to compile, because `Auth::Ntlm` is not defined.

- [ ] **Step 3: Implement `ntlm.rs` and the exchange in `send`**

The dedicated client is `base()` plus TLS options, `.http1_only().pool_max_idle_per_host(1)`. The exchange:
1. `first = request.try_clone()`, with the Type 1 Authorization header set last (after user headers).
2. If the answer is not 401, or `challenge()` is `None`, return that answer.
3. Read its body to the end.
4. Set the Type 3 Authorization header on `request` and execute it.
5. Time runs from the first leg until the last body is read.

A malformed challenge gives `SendError::Http("Invalid NTLM challenge from the server")`.

- [ ] **Step 4: Run the crate's tests and clippy**

Run: `cargo test -p soapstone-http && cargo clippy -p soapstone-http --all-targets -- -D warnings`
Expected: PASS, clean.

- [ ] **Step 5: Stage**

---

### Task 4: `soapstone-core` — WSDL download options

**Files:**
- Modify: `crates/soapstone-core/src/loader.rs`, `crates/soapstone-core/src/lib.rs`, `crates/soapstone-core/Cargo.toml` (dev: `rustls`), `crates/soapstone-core/tests/common/mod.rs`, `crates/soapstone-core/tests/loader.rs`

**Interfaces:**
- Produces:
  - `pub struct FetchOptions { pub username: String, pub password: String, pub insecure: bool }` (Debug, Clone, Default, PartialEq, Eq).
  - `pub fn fetch_all_with(root: &Url, options: &FetchOptions) -> Result<Vec<Document>, LoadError>`. `fetch_all(root)` becomes `fetch_all_with(root, &FetchOptions::default())`.
  - Both are re-exported from `soapstone_core`.

- [ ] **Step 1: Write the failing tests**

In `tests/common/mod.rs`:
- `serve_recording(routes) -> (u16, Arc<Mutex<Vec<String>>>)` records each request's head. `serve` keeps its signature and calls it.
- `serve_tls(body: String) -> u16` answers every GET with 200 and `body`, using `../soapstone-http/tests/fixtures/tls/server.{pem,key}`.

Tests in `tests/loader.rs`:

- `basic_credentials_go_only_to_the_wsdl_origin`:
  - Server B serves `/t.xsd` (a schema). Server A serves `/svc?wsdl`: a minimal `wsdl:definitions` whose `wsdl:types` schema has `<xsd:import namespace="urn:t" schemaLocation="http://127.0.0.1:<B>/t.xsd"/>`.
  - Call `fetch_all_with(A/svc, {username: "u", password: "p"})`.
  - Every request A recorded has `authorization: Basic dTpw`, including the bare `/svc` attempt and the `?wsdl` retry. No request B recorded has an `authorization` header.
- `untrusted_certificate_needs_insecure`:
  - `fetch_all(https://localhost:<port>/svc?wsdl)` gives `Err(LoadError::Fetch { .. })`.
  - `fetch_all_with(.., &FetchOptions { insecure: true, ..Default::default() })` gives `Ok` with 1 document.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-core --test loader`
Expected: FAIL to compile, because `fetch_all_with` is not defined.

- [ ] **Step 3: Implement**

- Build the blocking client with `danger_accept_invalid_certs(options.insecure)`.
- Add `.basic_auth` when `username` is not empty and `url.origin() == root.origin()`.

- [ ] **Step 4: Run the core tests**

Run: `cargo test -p soapstone-core`
Expected: PASS. The golden tests are unchanged.

- [ ] **Step 5: Stage**

---

### Task 5: `soapstone-store` — schema 2: request WS-Security and settings, WSDL download settings

**Files:**
- Modify: `crates/soapstone-store/src/lib.rs`, `crates/soapstone-store/src/services.rs`
- Test: `crates/soapstone-store/tests/store.rs`, `crates/soapstone-store/tests/services.rs`

**Interfaces:**
- Consumes:
  - `WsSecurity` and `RequestOptions` (Tasks 1–2), re-exported from `soapstone_store`.
  - `FetchOptions` (Task 4).
- Produces:
  - Migration 2 (each step in its own transaction; `user_version` becomes 2):
    ```sql
    ALTER TABLE requests ADD COLUMN ws_security TEXT NOT NULL DEFAULT '{}';
    ALTER TABLE requests ADD COLUMN options TEXT NOT NULL DEFAULT '{}';
    ALTER TABLE services ADD COLUMN fetch TEXT NOT NULL DEFAULT '{}';
    ```
  - New fields:
    - `Request.ws_security`, `Request.options`
    - `RequestEdit.ws_security`, `RequestEdit.options`
    - `prepare_send` copies both into `SoapCall`.
  - `FetchSettings { username: String, password: String, insecure: bool }`: Serialize, Deserialize, Default, `#[serde(rename_all = "camelCase", default)]`.
  - `ServiceSource { project_id: i64, url: String, fetch: FetchSettings }`: Serialize, camelCase.
  - Store methods:
    - `import_service(&mut self, project_id, documents, fetch: &FetchSettings) -> Result<i64>`
    - `refresh_service(&mut self, service_id, documents, fetch: &FetchSettings) -> Result<()>`: also stores `fetch`.
    - `service_source(id) -> Result<ServiceSource>`
    - `fetch_options(&self, project_id, fetch: &FetchSettings) -> Result<soapstone_core::FetchOptions>`: renders username and password with `variables(project_id)`.
  - `StoreError::Variables(Vec<String>)`, displayed as `Undefined variables: a, b`.

- [ ] **Step 1: Write the failing tests**

- `version_1_database_is_migrated` (`tests/store.rs`):
  - In a temp file, create 1b's exact v1 schema (a copy kept in the test, as a historical fixture) with `PRAGMA user_version = 1`, one project, service, operation and request.
  - `Store::open` it.
  - `request(id)` returns `ws_security == WsSecurity::default()` and `options == RequestOptions::default()`, and `user_version` is 2.
- `ws_security_and_options_round_trip_to_the_call` (`tests/services.rs`): save an edit with UsernameToken on and `timeout_seconds: 5, insecure: true, cert_path: "/c.p12"`. Then `request()` and `prepare_send().0` both carry those values.
- `service_keeps_its_fetch_settings_and_renders_them`:
  - Import with `{username: "{{wu}}", password: "pw", insecure: true}`. `service_source` returns them, with the project id and the URL.
  - After setting the base variable `wu = "bob"`, `fetch_options` gives `username: "bob"`.
  - Without that variable, it gives `Err(StoreError::Variables(["wu"]))`.
  - `refresh_service` with new settings stores them.

Update the existing tests' `edit()` and `import_service` calls to pass the new arguments.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-store`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`init` becomes: `if version < 1 { SCHEMA; user_version = 1 }`, then `if version < 2 { MIGRATION_2; user_version = 2 }`, each in its own transaction.

- [ ] **Step 4: Run the store tests**

Run: `cargo test -p soapstone-store`
Expected: PASS.

- [ ] **Step 5: Stage**

---

### Task 6: `soapstone-store` — secret variables in the system keychain

**Files:**
- Create: `crates/soapstone-store/src/vault.rs`, `crates/soapstone-store/tests/secrets.rs`
- Modify: `crates/soapstone-store/src/lib.rs`, `crates/soapstone-store/Cargo.toml` (`keyring = "4.2"`, default feature `v1`)

**Interfaces:**
- Produces:
  - `Variable.secret: bool`, with `#[serde(default)]`.
  - `pub enum Vault { System, Memory(RefCell<HashMap<String, String>>), Unavailable }`:
    - `Vault::memory()`.
    - `Unavailable` fails every call; it exists to test the fallback.
    - Methods, keyed by account:
      - `get(&self, account) -> Result<Option<String>, String>`
      - `set(&self, account, value) -> Result<(), String>`
      - `delete(&self, account) -> Result<(), String>`: a missing entry is `Ok`.
    - `System` uses `keyring::Entry::new("io.github.gsimei.soapstone", account)`, where `keyring::Error::NoEntry` maps to `Ok(None)`/`Ok(())`.
  - `Store::with_vault(self, vault: Vault) -> Store`. `open` uses `System`; `open_in_memory` uses `Vault::memory()`.
  - Store methods:
    - `environments(&self, project_id, reveal: bool) -> Result<Environments>`. With `reveal` false, secret values are `""` and the keychain is not touched.
    - `set_environments(&self, project_id, &Environments) -> Result<Option<String>>`: the `Option` is the warning.
    - `set_active_environment(&self, project_id, active: Option<&str>) -> Result<()>`.
    - `variables(project_id)` reveals.
    - `delete(Kind::Project, id)` also deletes `project-<id>` from the vault, ignoring errors.
  - `StoreError::Vault(String)`, displayed as `System keychain: {0}`.

- [ ] **Step 1: Write the failing tests in `tests/secrets.rs`**

Use a temp-file store with `.with_vault(Vault::memory())` and a raw `rusqlite::Connection` to read `projects.environments`.

- `secret_values_live_in_the_vault_not_in_the_database`:
  - Save base `{token, "t0p", secret}` and env `prod` `{pw, "p@ss", secret}` plus a plain `{host, "h", false}`.
  - The SQLite JSON contains neither `t0p` nor `p@ss`, but does contain `"h"`.
  - `environments(.., true)` returns the values; `environments(.., false)` returns `""` for both secrets and `"h"` for host.
  - With `prod` active, `variables()` has all three.
- `switching_the_active_environment_keeps_secrets`: after `set_active_environment(p, Some("prod"))`, `variables()` still has `pw = "p@ss"`.
- `unmarking_or_removing_secrets_updates_the_vault`:
  - Save again with `token` no longer secret: the SQLite JSON now contains `t0p`, and the vault entry no longer has it.
  - Save with no secrets: the vault has no `project-<id>` entry.
- `deleting_a_project_deletes_its_secrets`.
- `unavailable_keychain_falls_back_with_a_warning`:
  - With `Vault::Unavailable`, `set_environments` returns `Some(w)` with `w.starts_with("The system keychain is unavailable")`.
  - The SQLite JSON contains the value, and `variables()` has it.
- `unreadable_keychain_is_an_error_not_an_empty_secret`: save with a memory vault, then `.with_vault(Vault::Unavailable)`. `variables()` gives `Err(StoreError::Vault(_))`.

Add to `tests/store.rs`:

- `#[ignore = "touches the system keychain"] system_keychain_round_trip`: set, get and delete account `test-<pid>` with `Vault::System`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p soapstone-store --test secrets`
Expected: FAIL to compile, because `Vault` is not defined.

- [ ] **Step 3: Implement**

On save:
1. Collect the secrets into a `BTreeMap<String, BTreeMap<String, String>>`, skipping empty values. When it is empty, `delete` the entry.
2. Otherwise `set` the JSON entry. On `Ok`, blank those values in the copy written to SQLite. On `Err(e)`, keep them and return the warning.

On a revealed read:
1. Only when some secret has an empty value in SQLite, `get` the entry and fill the matching values.
2. An `Err` becomes `StoreError::Vault`.

- [ ] **Step 4: Run the store tests and clippy**

Run: `cargo test -p soapstone-store && cargo clippy -p soapstone-store --all-targets -- -D warnings`
Expected: PASS; 1 more ignored test; clean.

- [ ] **Step 5: Stage**

---

### Task 7: Tauri commands

**Files:**
- Modify: `app/src-tauri/src/commands.rs`, `app/src-tauri/src/main.rs`

**Interfaces:**
- Consumes: Tasks 4–6.
- Produces (camelCase JS arguments):
  - `import_service(projectId, source, fetch: FetchSettings) -> i64`
  - `refresh_service(serviceId, source: Option<String>, fetch: Option<FetchSettings>) -> ()`:
    - With both `source` and `fetch`, it downloads from the new source and stores the source and settings only when the refresh succeeds.
    - Otherwise it uses the stored ones.
  - `get_service_source(serviceId) -> ServiceSource`
  - `get_environments(projectId, reveal: bool) -> Environments`
  - `set_environments(projectId, environments) -> Option<String>` (the warning)
  - `set_active_environment(projectId, active: Option<String>) -> ()`
  - `send_request(edit)`: unchanged signature.

Rules:
- Every command that can touch the keychain runs inside `blocking(app, …)`, because a keychain prompt must not block an async worker: `get_environments` with reveal, `set_environments`, `send_request`'s save-and-prepare, and import/refresh (variables for the fetch).
- WSDL fetching uses `fetch_all_with(&url, &store.fetch_options(project_id, &fetch)?)`. The store lock is released before the download, as in 1b.

- [ ] **Step 1: Implement and register the commands**

- [ ] **Step 2: Build and lint**

Run: `npm --prefix app run build && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean; all tests pass.

- [ ] **Step 3: Stage**

---

### Task 8: Frontend — Auth (NTLM, client certificate), WS-Security and Settings tabs

**Files:**
- Modify: `app/src/api.ts`, `app/src/RequestView.tsx`, `app/src/styles.css`
- Test: `app/src/logic.test.ts` (no new pure logic expected), mock-harness checklist

**Interfaces:**
- Consumes: Task 7's commands, and the JSON shapes from Tasks 1–3 and 5.
- Produces, in `api.ts`:
  - `Auth` gains `{ type: "ntlm"; username; password; domain }`.
  - `WsSecurity`, `RequestOptions`, `FetchSettings`, `ServiceSource`.
  - `Variable.secret: boolean`.
  - `Request`/`RequestEdit` gain `wsSecurity` and `options`.
  - Wrappers:
    - `importService(projectId, source, fetch)`
    - `refreshService(serviceId, source?, fetch?)`
    - `getServiceSource(serviceId)`
    - `getEnvironments(projectId, reveal = false)`
    - `setEnvironments(..) -> Promise<string | null>`
    - `setActiveEnvironment(projectId, active)`

- [ ] **Step 1: Implement the tabs**

Tabs: `Body`, `Headers (n)`, `Auth (<type>)`, `WS-Security` (with ` ✓` when enabled), `Settings`.

- **Auth:**
  - The Type select gains `NTLM`, with fields Username, Password (`type="password"`) and Domain.
  - Below, a section "Client certificate" with:
    - "Certificate file (.p12, .pfx)": an input plus Browse…, which opens `open()` with filters `PKCS#12` (`p12`, `pfx`) and `All files` (`*`).
    - "Certificate password": `type="password"`.
  - Hint: "Values can use {{variables}}."
- **WS-Security:**
  - Checkbox "UsernameToken". When it is on: Username; Password (`type="password"`); "Password type" select (`PasswordText`, `PasswordDigest`); checkboxes "Add Nonce" and "Add Created".
  - Checkbox "Timestamp". When it is on: "Time to live (seconds)", a number input with min 1.
  - Hint: "Added to the SOAP Header when you send. Values can use {{variables}}."
- **Settings:**
  - "Timeout (seconds)": a number input, min 1.
  - Checkbox "Ignore TLS certificate errors", with the hint "Only for test servers: the connection is no longer verified."

Every text input spreads `{...plain}`.

- [ ] **Step 2: Check it in the mock harness**

Run: `npm --prefix app test && npm --prefix app run build`, then the mock harness in the built-in browser.

Checklist:
1. Every tab renders, in light and dark.
2. Switching Auth to NTLM shows three fields.
3. Turning UsernameToken off hides its fields, but keeps the typed values when it is turned back on.
4. The edit sent to `send_request` carries `wsSecurity` and `options`.

Expected: all four hold; vitest and build are green.

- [ ] **Step 3: Stage**

---

### Task 9: Frontend — secret variables, WSDL download options, edit WSDL source

**Files:**
- Modify: `app/src/KeyValueTable.tsx`, `app/src/EnvironmentDialog.tsx`, `app/src/App.tsx`, `app/src/ImportDialog.tsx`, `app/src/Sidebar.tsx`, `app/src/styles.css`

**Interfaces:**
- Consumes: Task 8's `api.ts`.

- [ ] **Step 1: Implement**

- **KeyValueTable:** new prop `secrets?: boolean`. When it is set, each row gets a checkbox `aria-label="Secret"` (title "Keep the value in the system keychain"), and a secret row's value input is `type="password"`. Rows carry `secret` only when `secrets` is set.
- **EnvironmentDialog:**
  - It receives the revealed environments.
  - Save `await`s `api.setEnvironments`. A returned warning is shown with `message(warning, { title: "Soapstone", kind: "warning" })` before closing.
  - Hint gains: "Secret values are kept in the system keychain."
- **App:**
  - The top-bar select calls `api.setActiveEnvironment` and then reloads the environments without reveal.
  - "Manage environments" loads `getEnvironments(projectId, true)` before opening the dialog.
- **ImportDialog:**
  - A `<details>` "Download options" holds Username, Password (`type="password"`) and the checkbox "Ignore TLS certificate errors".
  - New optional prop `edit?: { serviceId: number }`. When it is set:
    - The title is "Edit WSDL source" and the button is "Save and refresh".
    - The fields come prefilled from `getServiceSource`, with the options open.
    - Submit calls `refreshService(serviceId, source, fetch)`.
- **Sidebar:** the service menu gains "Edit WSDL source…" after "Refresh WSDL".

- [ ] **Step 2: Check it in the mock harness**

Run: `npm --prefix app test && npm --prefix app run build`, then the harness.

Checklist:
1. A secret row is masked and its toggle round-trips.
2. A save warning shows a dialog.
3. The import options reach `import_service`.
4. "Edit WSDL source…" opens prefilled and calls `refresh_service` with source and fetch.
5. Switching the environment in the top bar calls `set_active_environment`, not `set_environments`.

Expected: all five hold; green.

- [ ] **Step 3: Stage**

---

### Task 10: End-to-end check and README

**Files:**
- Modify: `README.md`, `app/mock.ts` (scratch, untracked; deleted at the end of 1d)

- [ ] **Step 1: Full suite**

Run: `npm --prefix app test && npm --prefix app run build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all green; the ignored tests are the network one and the keychain one.

- [ ] **Step 2: The app launches and stays fast**

Run: `PATH="$HOME/.cargo/bin:$PATH" npm --prefix app run tauri dev`, then read the `metric startup` line.
Expected: under 1 s warm, with no keychain prompt at startup because nothing is revealed.

- [ ] **Step 3: README**

Under "What works", add: client certificates (`.p12`/`.pfx`), NTLM, WS-Security UsernameToken (text/digest) and Timestamp, secret variables in the system keychain, per-request timeout and "Ignore TLS certificate errors", and WSDLs behind Basic auth or untrusted certificates.

Add a "Java keystores" note:

```bash
keytool -importkeystore -srckeystore keystore.jks -destkeystore keystore.p12 -deststoretype PKCS12
```

- [ ] **Step 4: User acceptance (the user's, recorded in the final report)**

The user checks:
- NTLM against a real IIS/Windows service.
- A client certificate against a real mTLS service.
- That the macOS keychain prompt appears once per build for an unsigned app, and that secrets survive a restart.

- [ ] **Step 5: Stage**
