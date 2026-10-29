import { useEffect, useRef, useState } from "react";
import { api, type HistoryEntry, type HttpResponse } from "./api";
import { formatSize } from "./logic";
import { XmlEditor } from "./XmlEditor";
import { responseDiff } from "./responseDiff";

export function Tabs<T extends string>(props: { value: T; onChange(value: NoInfer<T>): void; tabs: [NoInfer<T>, string][] }) {
  return <div className="tabs" role="tablist">{props.tabs.map(([value, label]) =>
    <button key={value} role="tab" aria-selected={props.value === value} onClick={() => props.onChange(value)}>{label}</button>,
  )}</div>;
}

type ResponseDisplay = "auto" | "xml" | "original";
// Reuse formatting when changing tabs; old responses can still be garbage-collected.
const formattedResponses = new WeakMap<HttpResponse, Promise<string>>();

function formattedResponse(response: HttpResponse): Promise<string> {
  let result = formattedResponses.get(response);
  if (!result) {
    result = response.pretty !== null ? Promise.resolve(response.pretty) : api.formatXml(response.body);
    formattedResponses.set(response, result);
  }
  return result;
}

function ResponseBody(props: { response: HttpResponse; raw: boolean; display: ResponseDisplay; onDisplay(display: ResponseDisplay): void }) {
  const [formatted, setFormatted] = useState<{ response: HttpResponse; text: string } | null>(null);
  const [formatting, setFormatting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    setError(null);
    setFormatting(false);
    const xml = props.response.pretty !== null || props.response.body.trimStart().startsWith("<");
    if (!props.raw && props.display !== "original" && (props.display === "xml" || xml)) {
      setFormatting(true);
      void formattedResponse(props.response).then((text) => {
        if (active) { setFormatted({ response: props.response, text }); setFormatting(false); }
      }, (e) => {
        if (active) { if (props.display === "xml") setError(String(e)); setFormatting(false); }
      });
    }
    return () => { active = false; };
  }, [props.response, props.raw, props.display]);
  const original = props.raw || props.display === "original";
  const text = original ? props.response.body : formatted?.response === props.response ? formatted.text : props.response.pretty ?? props.response.body;
  return <>
    {!props.raw && <div className="editor-toolbar">
      <label>View <select aria-label="Response format" value={props.display} onChange={(e) => props.onDisplay(e.target.value as ResponseDisplay)}>
        <option value="auto">Automatic</option>
        <option value="xml">XML</option>
        <option value="original">Original</option>
      </select></label>
      {formatting && <span className="muted small" role="status">Formatting XML…</span>}
    </div>}
    {error && <div role="alert" className="error-text editor-notice">{error}</div>}
    <XmlEditor docKey="response" value={text} readOnly />
  </>;
}

export function ResponsePane(props: { response: HttpResponse | null; error: string | null; sending: boolean; requestId?: number; revision?: number }) {
  const [tab, setTab] = useState<"body" | "headers" | "raw" | "compare">("body");
  const [display, setDisplay] = useState<ResponseDisplay>("auto");
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [selected, setSelected] = useState("");
  const [historical, setHistorical] = useState<HttpResponse | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const selection = useRef(0);
  useEffect(() => {
    let active = true;
    if (props.requestId !== undefined) void api.history(props.requestId).then((entries) => { if (active) setHistory(entries); }).catch((e) => { if (active) setHistoryError(String(e)); });
    return () => { active = false; };
  }, [props.requestId, props.revision, props.response]);
  async function choose(id: string) {
    const token = ++selection.current;
    setSelected(id); setHistorical(null); setHistoryError(null);
    if (!id) { if (tab === "compare") setTab("body"); return; }
    try { const response = await api.historyResponse(Number(id)); if (token === selection.current) setHistorical(response); }
    catch (e) { if (token === selection.current) setHistoryError(String(e)); }
  }
  const r = selected ? historical : props.response;
  let content;
  if (props.sending) content = <div className="empty">Sending…</div>;
  else if (props.error) content = <div className="error-panel" role="alert">{props.error}</div>;
  else if (!r) content = <div className="empty">{selected ? "Loading history…" : "Send the request to see the response."}</div>;
  else {
    const tone = r.status < 300 ? "ok" : r.status < 400 ? "warn" : "error";
    content = <>
      <div className="status-line">
        <span className={`badge ${tone}`}>{r.status} {r.statusText}</span>
        <span>{r.timeMs} ms</span><span>{formatSize(r.size)}</span>
        {r.fault && <><span className="badge error">SOAP Fault</span><span className="fault" title={r.fault}>{r.fault}</span></>}
      </div>
      <Tabs value={tab} onChange={setTab} tabs={[["body", "Body"], ["headers", `Headers (${r.headers.length})`], ["raw", "Raw"], ...(historical && props.response ? [["compare", "Compare with latest"] as ["compare", string]] : [])]} />
      <div className="tab-body">
        {(tab === "body" || tab === "raw") && <ResponseBody key={`${selected}-${props.revision}-${tab}`} response={r} raw={tab === "raw"} display={display} onDisplay={setDisplay} />}
        {tab === "headers" && <div className="scroll"><table className="headers"><tbody>{r.headers.map((h, i) => <tr key={i}><th>{h.name}</th><td>{h.value}</td></tr>)}</tbody></table></div>}
        {tab === "compare" && historical && props.response && <pre className="response-diff">{responseDiff(historical.body, props.response.body)}</pre>}
      </div>
    </>;
  }
  return <section className="response" aria-label="Response">
    {props.requestId !== undefined && <div className="editor-toolbar">
      <label>History <select aria-label="Response history" value={selected} onChange={(e) => void choose(e.target.value)}>
        <option value="">Latest response</option>
        {history.map((h) => <option key={h.id} value={h.id}>{new Date(h.createdAt).toLocaleString()} · {h.status} · {h.timeMs} ms</option>)}
      </select></label><span className="muted small">Last 20 executions · up to 64 MB</span>
    </div>}
    {historyError && <div className="error-text editor-notice" role="alert">{historyError}</div>}
    {content}
  </section>;
}
