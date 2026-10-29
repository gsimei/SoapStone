import { useEffect, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import type { Environments, Variable } from "./api";
import { KeyValueTable } from "./KeyValueTable";
import { fromDraft, plain, removeEnvironment, toDraft, validVariableName } from "./logic";

/** Edits a local copy of the project's environments; nothing is saved until Save. */
export function EnvironmentDialog(props: { initial: Environments; onSave(environments: Environments): void; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [envs, setEnvs] = useState(() => toDraft(props.initial));
  const [selected, setSelected] = useState(-1); // -1: base variables

  useEffect(() => {
    dialog.current?.showModal();
  }, []);

  const names = envs.environments.map((e) => e.name.trim());
  const duplicate = names.find((n, i) => n && names.indexOf(n) !== i);
  const invalidVariable = [envs.base, ...envs.environments.map((e) => e.variables)].flat().find((v) => v.name.trim() && !validVariableName(v.name.trim()));
  const duplicateVariable = [envs.base, ...envs.environments.map((e) => e.variables)].some((vars) => {
    const names = vars.map((v) => v.name.trim()).filter(Boolean);
    return new Set(names).size !== names.length;
  });
  const problem = names.some((n) => !n)
    ? "Every environment needs a name."
    : duplicate
      ? `Two environments are named "${duplicate}".`
      : invalidVariable ? `Invalid variable name: ${invalidVariable.name}. Use letters, digits, _, . or -; start with a letter or _.`
      : duplicateVariable ? "Variable names must be unique within each environment."
      : null;
  const env = selected >= 0 ? envs.environments[selected] : null;

  function setVariables(variables: Variable[]) {
    if (!env) setEnvs({ ...envs, base: variables });
    else setEnvs({ ...envs, environments: envs.environments.map((e, i) => (i === selected ? { ...e, variables } : e)) });
  }

  function rename(name: string) {
    if (!env) return;
    setEnvs({ ...envs, environments: envs.environments.map((e, i) => (i === selected ? { ...e, name } : e)) });
  }

  function add() {
    let name = "New environment";
    for (let n = 2; names.includes(name); n++) name = `New environment ${n}`;
    setEnvs({ ...envs, environments: [...envs.environments, { name, variables: [] }] });
    setSelected(envs.environments.length);
  }

  async function remove() {
    if (!env) return;
    if (!(await ask(`Delete environment "${env.name}"?`, { title: "Delete environment", kind: "warning" }))) return;
    setEnvs(removeEnvironment(envs, selected));
    setSelected(-1);
  }

  function save() {
    props.onSave(fromDraft(envs));
  }

  return (
    <dialog
      ref={dialog}
      className="dialog env-dialog"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
    >
      <h2>Environments</h2>
      <div className="env-layout">
        <ul className="env-list" role="listbox" aria-label="Environments">
          <li role="option" aria-selected={selected === -1}>
            <button onClick={() => setSelected(-1)}>Base variables</button>
          </li>
          {envs.environments.map((e, i) => (
            <li key={i} role="option" aria-selected={selected === i}>
              <button onClick={() => setSelected(i)}>
                {e.name || <span className="muted">Unnamed</span>}
                {i === envs.activeIndex && <span className="muted"> · active</span>}
              </button>
            </li>
          ))}
          <li>
            <button className="add" onClick={add}>
              + Add environment
            </button>
          </li>
        </ul>
        <div className="env-editor">
          {env && (
            <div className="input-row">
              <input {...plain} aria-label="Environment name" value={env.name} onChange={(e) => rename(e.target.value)} />
              <button onClick={() => void remove()}>Delete environment</button>
            </div>
          )}
          <KeyValueTable
            rows={env ? env.variables : envs.base}
            onChange={setVariables}
            namePlaceholder="Variable"
            valuePlaceholder="Value"
            secrets
          />
        </div>
      </div>
      <p className="hint">
        Use {"{{name}}"} in the endpoint, headers, body and auth. The active environment overrides base variables.
        Secret values are kept in the system keychain.
      </p>
      <footer>
        {problem && <span className="error-text">{problem}</span>}
        <button onClick={props.onClose}>Cancel</button>
        <button className="primary" disabled={!!problem} onClick={save}>
          Save
        </button>
      </footer>
    </dialog>
  );
}
