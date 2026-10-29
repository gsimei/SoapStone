import { useRef, useState } from "react";
import type { HttpResponse } from "./api";
import { formatSize } from "./logic";
import { XmlEditor } from "./XmlEditor";

export function Tabs<T extends string>(props: { value: T; onChange(value: NoInfer<T>): void; tabs: [NoInfer<T>, string][] }) {
  return (
    <div className="tabs" role="tablist">
      {props.tabs.map(([value, label]) => (
        <button key={value} role="tab" aria-selected={props.value === value} onClick={() => props.onChange(value)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function ResponsePane(props: { response: HttpResponse | null; error: string | null; sending: boolean }) {
  const [tab, setTab] = useState<"body" | "headers" | "raw">("body");
  // Each new response object gets a new editor document.
  const seen = useRef<{ response: HttpResponse | null; key: number }>({ response: null, key: 0 });
  if (seen.current.response !== props.response) seen.current = { response: props.response, key: seen.current.key + 1 };
  const key = seen.current.key;

  const r = props.response;
  let content;
  if (props.sending) content = <div className="empty">Sending…</div>;
  else if (props.error)
    content = (
      <div className="error-panel" role="alert">
        {props.error}
      </div>
    );
  else if (!r) content = <div className="empty">Send the request to see the response.</div>;
  else {
    const tone = r.status < 300 ? "ok" : r.status < 400 ? "warn" : "error";
    content = (
      <>
        <div className="status-line">
          <span className={`badge ${tone}`}>
            {r.status} {r.statusText}
          </span>
          <span>{r.timeMs} ms</span>
          <span>{formatSize(r.size)}</span>
          {r.fault && (
            <>
              <span className="badge error">SOAP Fault</span>
              <span className="fault" title={r.fault}>
                {r.fault}
              </span>
            </>
          )}
        </div>
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            ["body", "Body"],
            ["headers", `Headers (${r.headers.length})`],
            ["raw", "Raw"],
          ]}
        />
        <div className="tab-body" hidden={tab !== "body"}>
          <XmlEditor docKey={`body-${key}`} value={r.pretty ?? r.body} readOnly />
        </div>
        <div className="tab-body scroll" hidden={tab !== "headers"}>
          <table className="headers">
            <tbody>
              {r.headers.map((h, i) => (
                <tr key={i}>
                  <th>{h.name}</th>
                  <td>{h.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="tab-body" hidden={tab !== "raw"}>
          <XmlEditor docKey={`raw-${key}`} value={r.body} readOnly />
        </div>
      </>
    );
  }

  return (
    <section className="response" aria-label="Response">
      {content}
    </section>
  );
}
