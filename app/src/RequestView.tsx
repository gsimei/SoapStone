import { useEffect, useRef, useState } from "react";
import { message, open } from "@tauri-apps/plugin-dialog";
import { api, type Auth, type HttpResponse, type Request, type RequestEdit, type RequestOptions, type WsSecurity } from "./api";
import { KeyValueTable } from "./KeyValueTable";
import { ResponsePane, Tabs } from "./ResponsePane";
import { Split } from "./Split";
import { XmlEditor } from "./XmlEditor";
import { plain, wholeNumber } from "./logic";

const AUTH_LABEL = { none: "None", basic: "Basic", bearer: "Bearer", ntlm: "NTLM" } as const;

function emptyAuth(type: Auth["type"]): Auth {
  if (type === "basic") return { type, username: "", password: "" };
  if (type === "bearer") return { type, token: "" };
  if (type === "ntlm") return { type, username: "", password: "", domain: "" };
  return { type: "none" };
}

type Tab = "body" | "headers" | "auth" | "wss" | "settings";

/** Rendered as <RequestView key={requestId}/>: loads, edits, autosaves and sends one request. */
export function RequestView(props: { requestId: number; definedVars: Set<string>; switchStart: number | null }) {
  const [request, setRequest] = useState<Request | null>(null);
  const [edit, setEdit] = useState<RequestEdit | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("body");
  const [response, setResponse] = useState<HttpResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const alive = useRef(true);
  const pending = useRef<RequestEdit | null>(null);
  const timer = useRef<number | undefined>(undefined);

  function flush() {
    window.clearTimeout(timer.current);
    const e = pending.current;
    pending.current = null;
    if (e) api.saveRequest(e).catch((err) => message(`Could not save the request: ${err}`, { title: "Soapstone", kind: "error" }));
  }

  function update(patch: Partial<RequestEdit>) {
    setEdit((prev) => {
      const next = { ...prev!, ...patch };
      pending.current = next;
      return next;
    });
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(flush, 500);
  }

  useEffect(() => {
    api
      .getRequest(props.requestId)
      .then((r) => {
        if (!alive.current) return;
        setRequest(r);
        setEdit({ id: r.id, endpoint: r.endpoint, body: r.body, headers: r.headers, auth: r.auth, wsSecurity: r.wsSecurity, options: r.options });
        setResponse(r.lastResponse);
      })
      .catch((e) => alive.current && setLoadError(String(e)));
    return () => {
      alive.current = false;
      flush();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the parent keys this view by request id
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
    setSending(true);
    setError(null);
    try {
      const r = await api.send(edit);
      if (alive.current) setResponse(r);
    } catch (e) {
      if (alive.current) setError(String(e));
    } finally {
      if (alive.current) setSending(false);
    }
  }

  const sendRef = useRef(send);
  sendRef.current = send;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
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
    <Split storageKey="panes" initial={0.5} min={0.2}>
      <section className="request" aria-label="Request">
        <header className="request-head">
          <div className="title">
            {request.operation} <span className="muted">›</span> {request.name}
          </div>
          <div className="muted small">
            SOAP {request.soapVersion} · SOAPAction "{request.soapAction ?? ""}"
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
            <button onClick={() => void api.cancelSend(edit.id)}>Cancel</button>
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
          <XmlEditor docKey={request.id} value={edit.body} onChange={(body) => update({ body })} definedVars={props.definedVars} />
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
      <ResponsePane response={response} error={error} sending={sending} />
    </Split>
  );
}
