# Soapstone user guide

[Back to the README](../README.md)

Use the [quick start](../README.md#your-first-request) for your first request. This guide covers authentication,
variables, history, test sequences and project migration.

## Projects and WSDLs

The sidebar organizes **projects → services → operations → requests**. Click the project name to switch projects,
create a project or open the import/export actions. Use an item's **⋯** menu or right-click it for more actions.

- **Import WSDL** accepts a URL or file path and follows imported WSDLs and XML schemas. Each operation starts with
  a generated sample request. Operations with several bindings show the binding name alongside them.
- **New request** adds another request for the same operation; **Duplicate request** copies an existing one.
- **Refresh WSDL** reloads a service while preserving saved requests. Operations removed from the WSDL are marked.
- **Edit WSDL source…** changes the source URL or download settings before refreshing.
- **Search operations** filters the sidebar. **⌘P / Ctrl+P** opens the request picker.

For protected WSDLs, expand **Download options** in the import dialog and enter Basic authentication credentials.
Credentials are sent only to the WSDL's own server. Download options also support untrusted TLS certificates for
test services. Request authentication is configured separately, in the request's **Auth** tab.

## XML editing and request preview

Select an operation or saved request to open its editor. Changes are saved automatically.

| Action | Behavior |
| --- | --- |
| **Format XML** | Reformat the XML body for readability. |
| **Validate** | Resolve variables and check the resulting XML; errors include line and column information. |
| **Preview** | Show the resolved endpoint, headers and XML, with WS-Security applied and sensitive values masked. Nothing is sent. |
| **Reset sample** | Replace only the body with the WSDL-generated sample. The replacement can be undone in the editor. |
| **Send** | Execute the request. Shortcut: **⌘Enter / Ctrl+Enter**. |
| **Cancel** | Stop an in-flight request, including after navigating away and back. |

The XML editor offers variable completion after `{{` and tooltips showing a variable's source and value. Secret
values stay hidden. Invalid resolved XML is rejected before sending.

## Variables and secrets

Open **Manage environments** to edit shared **Base variables** and named environments. Select a named environment
in the top bar to activate it. Active environment values override base values with the same names.

Variables work in the endpoint, headers, body and authentication fields. For example, define `username` and
`password`, mark the latter as **Secret**, and use them in a request:

```xml
<Credentials>
   <Username>{{username}}</Username>
   <Password>{{password}}</Password>
</Credentials>
```

Values containing `&`, `<` or quotes are escaped for XML text and attributes. Inside CDATA, they remain character
data. Use `{{fragment|raw}}` only when the variable deliberately contains XML markup. Substitution happens once;
variable values containing other variable references are not expanded recursively.

Secret variable values are stored in the system keychain: macOS Keychain, Windows Credential Manager or Secret
Service on Linux. The keychain service is `io.github.gsimei.soapstone`, with entries named `project-<id>` and,
for longer values, `project-<id>.1`, `project-<id>.2`, and so on. If no keychain is available, Soapstone falls back to
database storage and displays a warning.

Passwords entered directly into Auth, WS-Security, client certificate or WSDL download fields are stored in the
database. Put a secret `{{variable}}` in those fields to keep its value in the keychain.

## Authentication, certificates and headers

In the request's **Auth** tab, choose **None**, **Basic**, **Bearer** or **NTLM**, then fill in the corresponding
fields. NTLM also accepts a domain. These fields support variables.

For mutual TLS, choose a `.p12` or `.pfx` file under **Client certificate** and enter its password. Soapstone reads
PKCS#12 certificates. If you have a Java keystore, convert it first using the JDK's `keytool`:

```bash
keytool -importkeystore -srckeystore keystore.jks -destkeystore keystore.p12 -deststoretype PKCS12
```

The **WS-Security** tab supports:

- **UsernameToken**, with **PasswordText** or **PasswordDigest**, plus optional Nonce and Created values.
- **Timestamp**, with a configurable time to live.

These elements are added to the SOAP Header when you send or preview the request.

Use **Headers** for custom HTTP headers. Explicit `Content-Type` and `SOAPAction` values replace the headers
Soapstone would generate automatically. Use **Settings** to set the timeout or ignore TLS certificate errors for a
test endpoint; ignoring certificate errors disables server certificate verification for that request.

## Response history and comparison

The response pane shows status, duration and size. **Body** formats XML automatically, including large responses;
use the view selector to choose XML or original text. **Raw** shows the original response text. **Headers** lists
the response headers, and SOAP Faults are flagged in the status area.

Choose an execution from **History** to open an earlier response. **Compare with latest** displays the changed
region with removed and added lines; comparisons of large documents use a bounded excerpt.

History keeps the last 20 responses per request, with a 64 MB budget per request and a 32 MB limit for an individual
response. Failed or cancelled exchanges preserve the previous response. Completed HTTP exchanges are saved before
the execution is released.

## Test sequences

Click **Test sequence** in the top bar. Add saved requests in order and configure the checks for each step:

| Field | Purpose |
| --- | --- |
| **HTTP status** | Require a specific status code, or leave blank to accept any status. |
| **No SOAP Fault** | Fail if the response contains a SOAP Fault. |
| **XPath** | Evaluate an XPath expression against the response XML. |
| **Expected XPath value** | Compare the XPath result with this text; leave blank to skip the comparison. |
| **Extract into variable** | Make the XPath result available to later steps under this variable name. |

For example, evaluate `string(//*[local-name(.)='Token'])`, extract it into `sessionToken`, then reference
`{{sessionToken}}` in a later request. Namespace prefixes declared on the response root are also available to XPath.

Click **Save** to persist the sequence or **Run sequence** to save and execute it. Steps stop on the first failure.
Click **Stop** to cancel the current exchange. Extracted values exist only for that run; they do not overwrite
saved environment values or request XML.

## Import from SoapUI

Open the project menu → **Import SoapUI project…** and select a `*-soapui-project.xml` file. Import creates a new
project and works offline, using the WSDL copies cached inside the SoapUI project.

The importer brings over:

- WSDL interfaces as services and saved requests with their endpoint, body and headers.
- Basic or NTLM credentials, and a WS-Security UsernameToken when a request has a WSS password type.
- Project properties as base variables, converting `${#Project#name}` into `{{name}}` throughout the requests.

The import report lists unsupported or skipped content:

- Other expansions, including `${#TestCase#…}`, `${#Global#…}` and `${=…}`, remain as written.
- REST interfaces, interfaces without a cached WSDL, and requests whose operation cannot be found in the WSDL.
  Import missing service definitions by URL instead.
- Other authentication types, such as OAuth and Kerberos; project-level WS-Security configurations and keystores.
- SoapUI test suites and mock services.

Imported literal credentials are stored in the database. Move them into secret variables when you need keychain
storage.

## Share a project

Open the project menu → **Export project without secrets…** to save a `.soapstone.json` file. On another
installation, use **Import Soapstone project…**. Import creates a new project and works offline because the export
includes cached WSDLs, requests, environments and test sequences.

Export omits response history, secret variable values, literal authentication and certificate passwords, and known
sensitive headers and XML fields. Re-enter credentials after importing.

Before sharing, replace other credentials embedded in XML, WSDL content, URLs or custom fields with secret variables
and inspect the exported file. Export does not scan every possible field for secrets.

## Troubleshooting

| Problem | What to check |
| --- | --- |
| **WSDL import fails** | Confirm the URL returns a WSDL, check its imported schema URLs, and use Download options if Basic authentication is required. |
| **A variable is missing** | Check its spelling in Manage environments and select the correct named environment in the top bar. |
| **XML validation fails** | Use the reported line and column. Check variable values and use `\|raw` only for intentional XML fragments. |
| **A request times out** | Confirm the endpoint and network/VPN access, then adjust Timeout in Settings if appropriate. |
| **The public example is unreachable** | Check your network access and the DataAccess service's availability. Use the WSDL URL for importing and the endpoint without `?WSDL` for sending. |
| **A Java keystore cannot be opened** | Convert `.jks` to PKCS#12, then choose the resulting `.p12` file in Auth. |
| **The keychain is unavailable** | Read the storage warning and check that your system keychain or Linux Secret Service is running. |

For installation and build requirements, return to [Install](../README.md#install). If the problem persists,
[open an issue](https://github.com/gsimei/SoapUI-RE/issues) with a minimal reproduction and any private data removed.
