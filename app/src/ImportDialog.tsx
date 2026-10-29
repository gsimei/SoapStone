import { useEffect, useRef, useState, type FormEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type FetchSettings } from "./api";
import { plain } from "./logic";

const NO_FETCH: FetchSettings = { username: "", password: "", insecure: false };

/** Imports a WSDL, or with `edit` changes where an imported service's WSDL comes from and refreshes it. */
export function ImportDialog(props: {
  projectId: number;
  edit?: { serviceId: number };
  onClose(): void;
  onImported(serviceId: number): void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [source, setSource] = useState("");
  const [fetch, setFetch] = useState(NO_FETCH);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const editId = props.edit?.serviceId;

  useEffect(() => {
    dialog.current?.showModal();
    if (editId === undefined) return;
    setBusy(true);
    api
      .getServiceSource(editId)
      .then((s) => {
        setSource(s.url);
        setFetch(s.fetch);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  }, [editId]);

  async function browse() {
    const file = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "WSDL", extensions: ["wsdl", "xml"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof file === "string") setSource(file);
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (editId === undefined) {
        props.onImported(await api.importService(props.projectId, source.trim(), fetch));
      } else {
        await api.refreshService(editId, source.trim(), fetch);
        props.onImported(editId);
      }
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  const editing = editId !== undefined;
  return (
    <dialog
      ref={dialog}
      className="dialog import-dialog"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) props.onClose();
      }}
    >
      <form onSubmit={submit}>
        <h2>{editing ? "Edit WSDL source" : "Import WSDL"}</h2>
        <label className="field">
          <span>WSDL URL or file path</span>
          <div className="input-row">
            <input {...plain}
              autoFocus
              value={source}
              onChange={(e) => setSource(e.target.value)}
              placeholder="https://example.com/service?wsdl"
              disabled={busy}
            />
            <button type="button" onClick={browse} disabled={busy}>
              Browse…
            </button>
          </div>
        </label>
        <details className="download-options" open={editing || undefined}>
          <summary>Download options</summary>
          <div className="form-grid">
            <label className="field">
              <span>Username</span>
              <input {...plain} value={fetch.username} onChange={(e) => setFetch({ ...fetch, username: e.target.value })} disabled={busy} />
            </label>
            <label className="field">
              <span>Password</span>
              <input {...plain}
                type="password"
                value={fetch.password}
                onChange={(e) => setFetch({ ...fetch, password: e.target.value })}
                disabled={busy}
              />
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={fetch.insecure}
                onChange={(e) => setFetch({ ...fetch, insecure: e.target.checked })}
                disabled={busy}
              />
              Ignore TLS certificate errors
            </label>
          </div>
          <p className="hint">The username and password go only to the WSDL's own server. Use a secret {"{{variable}}"} to keep the password in the system keychain.</p>
        </details>
        {error && (
          <p className="error-text dialog-error" role="alert">
            {error}
          </p>
        )}
        <footer>
          <button type="button" onClick={props.onClose} disabled={busy}>
            Cancel
          </button>
          <button className="primary" type="submit" disabled={busy || !source.trim()}>
            {busy ? (editing ? "Refreshing…" : "Importing…") : editing ? "Save and refresh" : "Import"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
