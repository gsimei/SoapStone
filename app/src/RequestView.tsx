import { useEffect, useRef, useState } from "react";
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { api, type Auth, type HttpResponse, type Request, type RequestEdit, type RequestOptions, type WsSecurity, type Preview } from "./api";
import { KeyValueTable } from "./KeyValueTable";
import { ResponsePane, Tabs } from "./ResponsePane";
import { Split } from "./Split";
import { XmlEditor } from "./XmlEditor";
import { plain, wholeNumber, VARIABLE_RE, type VariableInfo } from "./logic";
import { useExecution, currentExecution, sendRequest, cancelRequest, saveRequest, loadRequest } from "./executions";
import { PreviewDialog } from "./PreviewDialog";

const AUTH_LABEL = { none: "None", basic: "Basic", bearer: "Bearer", ntlm: "NTLM" } as const;

function emptyAuth(type: Auth["type"]): Auth {
  if (type === "basic") return { type, username: "", password: "" };
  if (type === "bearer") return { type, token: "" };
  if (type === "ntlm") return { type, username: "", password: "", domain: "" };
  return { type: "none" };
}

type Tab = "body" | "headers" | "auth" | "wss" | "settings";

/** Rendered as <RequestView key={requestId}/>: loads, edits, autosaves and sends one request. */
export function RequestView(props: { requestId: number; definedVars: Set<string>; variables?: VariableInfo[]; switchStart: number | null }) {
  const [request, setRequest] = useState<Request | null>(null);
  const [edit, setEdit] = useState<RequestEdit | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("body");
  const [response, setResponse] = useState<HttpResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const execution = useExecution(props.requestId);
  const sending = execution.runId !== null;
  const [preview, setPreview] = useState<Preview | null>(null);
  const [toolBusy, setToolBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const alive = useRef(true);
  const pending = useRef<RequestEdit | null>(null);
  const latestEdit = useRef<RequestEdit | null>(null);
  const timer = useRef<number | undefined>(undefined);

  function flush() {
    window.clearTimeout(timer.current);
    const e = pending.current;
    pending.current = null;
    if (e) void saveRequest(e).catch((err) => message(`Could not save the request: ${err}`, { title: "Soapstone", kind: "error" }));
  }

  function update(patch: Partial<RequestEdit>) {
    if (!latestEdit.current) return;
    const next = { ...latestEdit.current, ...patch };
    pending.current = next;
    latestEdit.current = next;
    setEdit(next);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(flush, 500);
  }

  useEffect(() => {
    alive.current = true;
    const revision = currentExecution(props.requestId).revision;
    loadRequest(props.requestId)
      .then((r) => {
        if (!alive.current) return;
        setRequest(r);
        latestEdit.current = { id: r.id, endpoint: r.endpoint, body: r.body, headers: r.headers, auth: r.auth, wsSecurity: r.wsSecurity, options: r.options };
        setEdit(latestEdit.current);
        if (currentExecution(props.requestId).revision === revision) setResponse(r.lastResponse);
      })
      .catch((e) => alive.current && setLoadError(String(e)));
    return () => {
      alive.current = false;
      flush();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the parent keys this view by request id
  }, []);

  useEffect(() => {
    if (execution.runId || execution.revision === 0) return;
    let current = true;
    void api.getRequest(props.requestId).then((r) => { if (current) setResponse(r.lastResponse); }).catch((e) => { if (current) setError(String(e)); });
    return () => { current = false; };
  }, [props.requestId, execution.revision, execution.runId]);

  useEffect(() => {
    const flushEdits = () => flush();
    window.addEventListener("soapstone:flush", flushEdits);
    return () => window.removeEventListener("soapstone:flush", flushEdits);
  }, []);

  useEffect(() => {
    const start = props.switchStart;
    if (request && start !== null) requestAnimationFrame(() => void api.logMetric("switch", performance.now() - start));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, when the request first renders
  }, [request]);

  async function send() {
    if (!edit || sending) return;
    window.clearTimeout(timer.current);
    pending.current = null; // send_request saves the edit first
    setError(null);
    try {
      const r = await sendRequest(edit);
      if (alive.current) setResponse(r);
    } catch (e) {
      if (alive.current) setError(String(e));
    }
  }

  const sendRef = useRef(send);
  sendRef.current = send;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
        if (document.querySelector("dialog[open]")) return;
        e.preventDefault();
        void sendRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (loadError) return <div className="empty error-text">{loadError}</div>;
  if (!request || !edit) return <div className="empty">Loading…</div>;

  const auth = edit.auth;
  const wss = edit.wsSecurity;
  const options = edit.options;
  const setWss = (patch: Partial<WsSecurity>) => update({ wsSecurity: { ...wss, ...patch } });
  const setOptions = (patch: Partial<RequestOptions>) => update({ options: { ...options, ...patch } });
  const missing = [...new Set([...JSON.stringify(edit).matchAll(new RegExp(VARIABLE_RE.source, "g"))].map((m) => m[1]).filter((name) => !props.definedVars.has(name)))];

  async function tool(action: "format" | "validate" | "preview" | "sample") {
    if (!edit || toolBusy) return;
    setToolBusy(true); setError(null); setNotice(null);
    const original = edit.body;
    const replace = (body: string) => {
      if (!alive.current) return;
      if (latestEdit.current?.body !== original) throw new Error("The body changed while this action was running. Try again.");
      update({ body });
    };
    try {
      if (action === "sample") {
        if (await ask("Replace this request body with the WSDL sample? You can undo the replacement in the editor.", { title: "Reset sample", kind: "warning" })) replace(await api.sample(edit.id));
      } else if (action === "format") replace(await api.formatXml(edit.body));
      else {
        const result = await api.preview(edit);
        if (action === "preview") setPreview(result);
        else setNotice("XML and variables are valid.");
      }
    } catch (e) { if (alive.current) setError(String(e)); }
    finally { if (alive.current) setToolBusy(false); }
  }

  async function browseCertificate() {
    const file = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "PKCS#12", extensions: ["p12", "pfx"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof file === "string") setOptions({ certPath: file });
  }

  return (
    <><Split storageKey="panes" initial={0.5} min={0.2}>
      <section className="request" aria-label="Request">
        <header className="request-head">
          <div className="title">
            {request.operation} <span className="muted">›</span> {request.name}
          </div>
          <div className="muted small">
            SOAP {request.soapVersion} · <span title={request.bindingNamespace}>{request.binding}</span> · SOAPAction "{request.soapAction ?? ""}"
          </div>
        </header>
        <div className="endpoint-row">
          <span className="method">POST</span>
          <input {...plain}
            aria-label="Endpoint"
            value={edit.endpoint}
            placeholder="http://host/service"
            onChange={(e) => update({ endpoint: e.target.value })}
          />
          {sending ? (
            <button onClick={() => void cancelRequest(edit.id).catch((e) => setError(String(e)))}>Cancel</button>
          ) : (
            <button className="primary" title="Send (⌘↵ / Ctrl+Enter)" onClick={() => void send()}>
              Send
            </button>
          )}
        </div>
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            ["body", "Body"],
            ["headers", `Headers (${edit.headers.length})`],
            ["auth", `Auth (${AUTH_LABEL[auth.type]})`],
            ["wss", `WS-Security${wss.usernameToken || wss.timestamp ? " ✓" : ""}`],
            ["settings", "Settings"],
          ]}
        />
        <div className="tab-body" hidden={tab !== "body"}>
          <div className="editor-toolbar">
            <button disabled={toolBusy} onClick={() => void tool("format")}>Format XML</button>
            <button disabled={toolBusy} onClick={() => void tool("validate")}>Validate</button>
            <button disabled={toolBusy} onClick={() => void tool("preview")}>Preview</button>
            <button disabled={toolBusy} onClick={() => void tool("sample")}>Reset sample</button>
            <span className="muted small">Variables: {"{{name}}"} · XML fragment: {"{{name|raw}}"}</span>
          </div>
          {missing.length > 0 && <div className="editor-notice error-text">Undefined variables: {missing.join(", ")}. Add them in Manage environments.</div>}
          {notice && <div className="editor-notice" role="status">{notice}</div>}
          <XmlEditor docKey={request.id} value={edit.body} onChange={(body) => update({ body })} definedVars={props.definedVars} variables={props.variables} />
        </div>
        <div className="tab-body scroll padded" hidden={tab !== "headers"}>
          <KeyValueTable
            rows={edit.headers}
            onChange={(headers) => update({ headers })}
            namePlaceholder="Header"
            valuePlaceholder="Value"
          />
          <p className="hint">Content-Type and SOAPAction are sent automatically; a header with the same name replaces them.</p>
        </div>
        <div className="tab-body scroll padded" hidden={tab !== "auth"}>
          <div className="form-grid">
            <label className="field">
              <span>Type</span>
              <select value={auth.type} onChange={(e) => update({ auth: emptyAuth(e.target.value as Auth["type"]) })}>
                <option value="none">None</option>
                <option value="basic">Basic</option>
                <option value="bearer">Bearer</option>
                <option value="ntlm">NTLM</option>
              </select>
            </label>
            {auth.type === "basic" && (
              <>
                <label className="field">
                  <span>Username</span>
                  <input {...plain} value={auth.username} onChange={(e) => update({ auth: { ...auth, username: e.target.value } })} />
                </label>
                <label className="field">
                  <span>Password</span>
                  <input {...plain} type="password" value={auth.password} onChange={(e) => update({ auth: { ...auth, password: e.target.value } })} />
                </label>
              </>
            )}
            {auth.type === "bearer" && (
              <label className="field">
                <span>Token</span>
                <input {...plain} value={auth.token} onChange={(e) => update({ auth: { ...auth, token: e.target.value } })} />
              </label>
            )}
            {auth.type === "ntlm" && (
              <>
                <label className="field">
                  <span>Username</span>
                  <input {...plain} value={auth.username} onChange={(e) => update({ auth: { ...auth, username: e.target.value } })} />
                </label>
                <label className="field">
                  <span>Password</span>
                  <input {...plain} type="password" value={auth.password} onChange={(e) => update({ auth: { ...auth, password: e.target.value } })} />
                </label>
                <label className="field">
                  <span>Domain</span>
                  <input {...plain} value={auth.domain} onChange={(e) => update({ auth: { ...auth, domain: e.target.value } })} />
                </label>
              </>
            )}
          </div>
          <h3 className="section-title">Client certificate</h3>
          <div className="form-grid">
            <label className="field">
              <span>Certificate file (.p12, .pfx)</span>
              <div className="input-row">
                <input {...plain} value={options.certPath} onChange={(e) => setOptions({ certPath: e.target.value })} />
                <button type="button" onClick={() => void browseCertificate()}>
                  Browse…
                </button>
              </div>
            </label>
            <label className="field">
              <span>Certificate password</span>
              <input {...plain} type="password" value={options.certPassword} onChange={(e) => setOptions({ certPassword: e.target.value })} />
            </label>
          </div>
          <p className="hint">Values can use {"{{variables}}"}.</p>
        </div>
        <div className="tab-body scroll padded" hidden={tab !== "wss"}>
          <div className="form-grid">
            <label className="check">
              <input type="checkbox" checked={wss.usernameToken} onChange={(e) => setWss({ usernameToken: e.target.checked })} />
              UsernameToken
            </label>
            {wss.usernameToken && (
              <>
                <label className="field">
                  <span>Username</span>
                  <input {...plain} value={wss.username} onChange={(e) => setWss({ username: e.target.value })} />
                </label>
                <label className="field">
                  <span>Password</span>
                  <input {...plain} type="password" value={wss.password} onChange={(e) => setWss({ password: e.target.value })} />
                </label>
                <label className="field">
                  <span>Password type</span>
                  <select value={wss.passwordType} onChange={(e) => setWss({ passwordType: e.target.value as WsSecurity["passwordType"] })}>
                    <option value="text">PasswordText</option>
                    <option value="digest">PasswordDigest</option>
                  </select>
                </label>
                <label className="check">
                  <input type="checkbox" checked={wss.nonce} onChange={(e) => setWss({ nonce: e.target.checked })} />
                  Add Nonce
                </label>
                <label className="check">
                  <input type="checkbox" checked={wss.created} onChange={(e) => setWss({ created: e.target.checked })} />
                  Add Created
                </label>
              </>
            )}
            <label className="check">
              <input type="checkbox" checked={wss.timestamp} onChange={(e) => setWss({ timestamp: e.target.checked })} />
              Timestamp
            </label>
            {wss.timestamp && (
              <label className="field">
                <span>Time to live (seconds)</span>
                <input {...plain}
                  type="number"
                  min={1}
                  value={wss.ttlSeconds || ""}
                  placeholder="300"
                  onChange={(e) => setWss({ ttlSeconds: wholeNumber(e.target.value) })}
                  onBlur={() => wss.ttlSeconds || setWss({ ttlSeconds: 300 })}
                />
              </label>
            )}
          </div>
          <p className="hint">Added to the SOAP Header when you send. Values can use {"{{variables}}"}.</p>
        </div>
        <div className="tab-body scroll padded" hidden={tab !== "settings"}>
          <div className="form-grid">
            <label className="field">
              <span>Timeout (seconds)</span>
              <input {...plain}
                type="number"
                min={1}
                value={options.timeoutSeconds || ""}
                placeholder="60"
                onChange={(e) => setOptions({ timeoutSeconds: wholeNumber(e.target.value) })}
                onBlur={() => options.timeoutSeconds || setOptions({ timeoutSeconds: 60 })}
              />
            </label>
            <label className="check">
              <input type="checkbox" checked={options.insecure} onChange={(e) => setOptions({ insecure: e.target.checked })} />
              Ignore TLS certificate errors
            </label>
          </div>
          <p className="hint">Only for test servers: the connection is no longer verified.</p>
        </div>
      </section>
      <ResponsePane requestId={props.requestId} revision={execution.revision} response={response} error={error ?? execution.error} sending={sending} />
    </Split>
    {preview && <PreviewDialog preview={preview} onClose={() => setPreview(null)} />}</>
  );
}
