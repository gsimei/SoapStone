# Soapstone

A fast, free desktop SOAP client: a lighter alternative to SoapUI, built with Rust and Tauri. Import a WSDL, pick an
operation, edit the generated request and send it.

## Status

Phase 1d. What works today:

- Import a WSDL from a URL or a file, including its imported schemas and WSDLs (SOAP 1.1 and 1.2, document and RPC).
- Projects, each with its services → operations → requests, kept in a local SQLite database.
- A sample request for every operation, generated like SoapUI's; several named requests per operation.
- XML editor (CodeMirror) with `{{variables}}` highlighted; send with ⌘↵ / Ctrl+Enter; cancel a send.
- Response with status, time, size, headers, formatted XML and raw text; SOAP Faults are flagged.
- Environments: base variables plus named environments (Insomnia-style `{{name}}`) in the endpoint, headers, body and
  auth.
- Basic, Bearer and NTLM auth, custom headers (they replace the automatic `Content-Type` / `SOAPAction`).
- Client certificates (`.p12` / `.pfx`) for mutual TLS.
- WS-Security UsernameToken (PasswordText or PasswordDigest, with Nonce and Created) and Timestamp.
- Secret variables: their values are kept in the system keychain (macOS Keychain, Windows Credential Manager, Secret
  Service on Linux), not in the database. Without a keychain they fall back to the database, with a warning.
- Per-request timeout and "Ignore TLS certificate errors".
- WSDLs behind Basic auth (the credentials go only to the WSDL's own server) or served with an untrusted certificate;
  "Edit WSDL source…" changes a service's URL and download options.
- Refresh WSDL keeps your requests; operations removed from the WSDL are marked.
- Search operations, quick open (⌘P / Ctrl+P), light and dark mode.
- Import a SoapUI project (see below).

## Install

Download the installer for your system from [GitHub Releases](https://github.com/gsimei/SoapUI-RE/releases). The
builds are not signed, so each system asks once before opening them:

- **macOS:** open the `.dmg` (`aarch64` for Apple Silicon, `x64` for Intel) and drag Soapstone to Applications. The
  first time, right-click Soapstone → Open → Open. Or run
  `xattr -dr com.apple.quarantine /Applications/Soapstone.app`.
- **Windows:** run the `.msi` or the `-setup.exe`. If SmartScreen appears, choose "More info" → "Run anyway".
- **Linux:** `sudo apt install ./Soapstone_*.deb`, or `chmod +x Soapstone_*.AppImage` and run the AppImage.

## Import from SoapUI

Project menu → "Import SoapUI project…" and pick a `*-soapui-project.xml`. It works offline: the WSDLs come from the
copy SoapUI keeps inside the project, nothing is downloaded. It creates a new project with:

- every WSDL interface as a service, and every saved request with its endpoint, body, headers and credentials (Basic or
  NTLM, and a WS-Security UsernameToken when the request has a WSS password type);
- project properties as base variables, and `${#Project#name}` turned into `{{name}}` everywhere.

When it finishes, it lists what it left out:

- other expansions (`${#TestCase#…}`, `${#Global#…}`, `${=…}`), which stay as written;
- REST interfaces, interfaces without a saved WSDL (import those by URL), and requests whose operation is not in the
  WSDL;
- other auth types (OAuth, Kerberos…), project-level WS-Security configurations and keystores;
- test suites and mock services, which arrive in a later version.

## Development

Prerequisites: Rust ≥ 1.88, Node ≥ 22, and
[Tauri's system dependencies](https://v2.tauri.app/start/prerequisites/).

```bash
npm --prefix app install
npm --prefix app run tauri dev
```

Checks:

```bash
npm --prefix app test && npm --prefix app run build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

## Java keystores

Soapstone reads PKCS#12 files. Convert a Java keystore first:

```bash
keytool -importkeystore -srckeystore keystore.jks -destkeystore keystore.p12 -deststoretype PKCS12
```

## Data

Everything is stored locally in the app data directory (`io.github.gsimei.soapstone`), not in your project folders.
Only secret variable values go to the system keychain, under the service `io.github.gsimei.soapstone` (entries
`project-<id>`, plus `project-<id>.1`, … for long values). Passwords typed straight into the Auth, WS-Security,
certificate or WSDL download fields are stored in the database; put a secret `{{variable}}` there to keep them in the
keychain.

## Releasing

1. Bump the version in `app/src-tauri/tauri.conf.json`, `app/package.json` and `app/src-tauri/Cargo.toml`.
2. Push a tag `v<version>`. The Release workflow builds the installers for macOS (Apple Silicon and Intel), Windows
   and Linux, and attaches them to a draft release.
3. Review the draft on GitHub and publish it.

## License

MIT
