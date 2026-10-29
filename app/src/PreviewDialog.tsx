import { useEffect, useRef } from "react";
import type { Preview } from "./api";
import { XmlEditor } from "./XmlEditor";

export function PreviewDialog(props: { preview: Preview; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="dialog preview-dialog" onCancel={(e) => { e.preventDefault(); props.onClose(); }}>
    <h2>Request preview</h2>
    <p className="hint">Variables are resolved and WS-Security is applied. Secrets are hidden. Nothing has been sent.</p>
    <code className="preview-endpoint">POST {props.preview.endpoint}</code>
    <pre className="preview-headers">{props.preview.headers.map((h) => `${h.name}: ${h.value}`).join("\n")}</pre>
    <div className="preview-body"><XmlEditor docKey="preview" value={props.preview.body} readOnly /></div>
    <footer><button onClick={props.onClose}>Close</button></footer>
  </dialog>;
}
