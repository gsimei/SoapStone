import { useEffect, useRef, useState } from "react";
import { api, type ServiceNode, type TestStep } from "./api";
import { cancelRequest, sendRequest, loadRequest } from "./executions";
import { plain, quickOpenItems } from "./logic";
import { runSequence, type StepResult } from "./sequence";

export default function SequenceDialog(props: { projectId: number; tree: ServiceNode[]; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const stop = useRef(false);
  const activeRequest = useRef<number | null>(null);
  const [steps, setSteps] = useState<TestStep[]>([]);
  const [results, setResults] = useState<StepResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const items = quickOpenItems(props.tree);
  useEffect(() => {
    dialog.current?.showModal();
    let alive = true;
    void api.getTestPlan(props.projectId).then((plan) => { if (alive) setSteps(plan); }).catch((e) => { if (alive) setError(String(e)); }).finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [props.projectId]);
  function update(index: number, patch: Partial<TestStep>) { setSteps((rows) => rows.map((s, i) => i === index ? { ...s, ...patch } : s)); setNotice(null); }
  async function save() { await api.saveTestPlan(props.projectId, steps); setNotice("Sequence saved."); }
  async function run() {
    if (busy) return;
    setBusy(true); setError(null); setResults([]); setNotice(null); stop.current = false;
    try {
      await save();
      await runSequence(steps, { getRequest: loadRequest, send: sendRequest }, () => stop.current,
        (result) => setResults((prev) => [...prev, result]), (id) => { activeRequest.current = id; });
      setNotice(stop.current ? "Sequence stopped." : "Sequence finished. See the results below.");
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); activeRequest.current = null; }
  }
  async function cancel() {
    stop.current = true;
    if (activeRequest.current !== null) await cancelRequest(activeRequest.current);
  }
  function move(index: number, direction: number) {
    const rows = [...steps];
    [rows[index], rows[index + direction]] = [rows[index + direction], rows[index]];
    setSteps(rows);
  }
  return <dialog ref={dialog} className="dialog sequence-dialog" onCancel={(e) => { e.preventDefault(); if (!busy) props.onClose(); }}>
    <h2>Test sequence</h2>
    <p className="hint">Run saved requests in order. Check HTTP status, SOAP Faults and XPath values. Extracted variables are available to later steps during this run.</p>
    <fieldset disabled={busy || loading} className="sequence-steps">
      {steps.map((step, index) => <section className="sequence-step" key={index}>
        <div className="input-row">
          <strong>{index + 1}.</strong>
          <select aria-label={`Step ${index + 1} request`} value={step.requestId} onChange={(e) => update(index, { requestId: Number(e.target.value) })}>
            {!items.some((i) => i.requestId === step.requestId) && <option value={step.requestId}>Missing request</option>}
            {items.map((item) => <option key={item.requestId} value={item.requestId}>{item.label}</option>)}
          </select>
          <button disabled={index === 0} aria-label={`Move step ${index + 1} up`} onClick={() => move(index, -1)}>↑</button>
          <button disabled={index === steps.length - 1} aria-label={`Move step ${index + 1} down`} onClick={() => move(index, 1)}>↓</button>
          <button aria-label={`Remove step ${index + 1}`} onClick={() => setSteps((s) => s.filter((_, i) => i !== index))}>Remove</button>
        </div>
        <div className="sequence-fields">
          <label>HTTP status<input {...plain} type="number" min={100} max={599} value={step.expectedStatus ?? ""} placeholder="Any" onChange={(e) => update(index, { expectedStatus: e.target.value === "" ? null : Number(e.target.value) })} /></label>
          <label className="check"><input type="checkbox" checked={step.notFault} onChange={(e) => update(index, { notFault: e.target.checked })} />No SOAP Fault</label>
          <label>XPath<input {...plain} value={step.xpath} placeholder="string(//*[local-name(.)='Token'])" onChange={(e) => update(index, { xpath: e.target.value })} /></label>
          <label>Expected XPath value<input {...plain} value={step.expectedValue ?? ""} placeholder="Leave blank to skip comparison" onChange={(e) => update(index, { expectedValue: e.target.value === "" ? null : e.target.value })} /></label>
          <label>Extract into variable<input {...plain} value={step.extractVariable} placeholder="sessionToken" onChange={(e) => update(index, { extractVariable: e.target.value })} /></label>
        </div>
      </section>)}
      <button disabled={items.length === 0 || steps.length >= 100} onClick={() => setSteps((s) => [...s, { requestId: items[0].requestId, expectedStatus: 200, notFault: true, xpath: "", expectedValue: null, extractVariable: "" }])}>Add step</button>
    </fieldset>
    {results.length > 0 && <ol className="sequence-results">{results.map((r) => <li key={r.index} className={r.passed ? "success-text" : "error-text"}>Step {r.index + 1}: {r.message}{r.timeMs !== undefined ? ` · ${r.timeMs} ms` : ""}</li>)}</ol>}
    {error && <p role="alert" className="error-text">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    <footer>
      <button disabled={busy} onClick={props.onClose}>Close</button>
      <button disabled={busy || loading} onClick={() => void save().catch((e) => setError(String(e)))}>Save</button>
      {busy ? <button onClick={() => void cancel().catch((e) => setError(String(e)))}>Stop</button> : <button className="primary" disabled={loading || steps.length === 0} onClick={() => void run()}>Run sequence</button>}
    </footer>
  </dialog>;
}
