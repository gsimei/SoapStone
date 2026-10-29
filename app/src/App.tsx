import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { message } from "@tauri-apps/plugin-dialog";
import { api, type Environments, type Project, type ServiceNode } from "./api";
import { EnvironmentDialog } from "./EnvironmentDialog";
import { definedVariables, variableInfo, quickOpenItems } from "./logic";
import { restoreExecutions } from "./executions";
import { QuickOpen } from "./QuickOpen";
import { Sidebar } from "./Sidebar";
import { Split } from "./Split";
const SequenceDialog = lazy(() => import("./SequenceDialog"));
const RequestView = lazy(() => import("./RequestView").then((m) => ({ default: m.RequestView })));

const NO_ENVIRONMENTS: Environments = { base: [], environments: [], active: null };

const hasRequest = (tree: ServiceNode[], id: number) => quickOpenItems(tree).some((i) => i.requestId === id);

export function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<number | null>(null);
  const [tree, setTree] = useState<ServiceNode[]>([]);
  const [environments, setEnvironments] = useState<Environments>(NO_ENVIRONMENTS);
  const [requestId, setRequestId] = useState<number | null>(null);
  const [switchStart, setSwitchStart] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [managing, setManaging] = useState<Environments | null>(null);
  const [quickOpen, setQuickOpen] = useState(false);
  const [sequence, setSequence] = useState(false);
  const selection = useRef(0);

  /** Reloads projects, then the tree and environments of `prefer` (else the first project). */
  async function load(prefer: number | null) {
    const token = ++selection.current;
    let list = await api.listProjects();
    if (list.length === 0) {
      await api.createProject("My Project");
      list = await api.listProjects();
    }
    const id = list.find((p) => p.id === prefer)?.id ?? list[0].id;
    const [t, envs] = await Promise.all([api.tree(id), api.getEnvironments(id)]);
    if (token !== selection.current) return null;
    setProjects(list);
    setProjectId(id);
    setTree(t);
    setEnvironments(envs);
    localStorage.setItem("soapstone.project", String(id));
    return { id, tree: t };
  }

  /** Switches project and reopens the request last opened there. */
  async function selectProject(prefer: number | null) {
    const loaded = await load(prefer);
    if (!loaded) return;
    const { id, tree: t } = loaded;
    setError(null);
    const saved = Number(localStorage.getItem(`soapstone.request.${id}`));
    setRequestId(hasRequest(t, saved) ? saved : null);
  }

  function openRequest(id: number) {
    setSwitchStart(performance.now());
    setRequestId(id);
    if (projectId !== null) localStorage.setItem(`soapstone.request.${projectId}`, String(id));
  }

  useEffect(() => {
    void restoreExecutions().catch((e) => setError(String(e)));
    selectProject(Number(localStorage.getItem("soapstone.project")) || null)
      .then(() => api.logMetric("startup"))
      .catch((e) => setError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, at startup
  }, []);

  /** After a rename, delete, import or refresh: reload, and close the open request if it is gone. */
  async function changed() {
    const loaded = await load(projectId);
    if (!loaded) return;
    const { tree: t } = loaded;
    setRequestId((r) => (r !== null && hasRequest(t, r) ? r : null));
  }

  /** The dialog closes only once the save worked, so a failure keeps the edits. */
  async function saveEnvironments(envs: Environments) {
    if (projectId === null) return;
    try {
      const warning = await api.setEnvironments(projectId, envs);
      if (warning) await message(warning, { title: "Soapstone", kind: "warning" });
      setManaging(null);
      setEnvironments(await api.getEnvironments(projectId));
    } catch (e) {
      await message(String(e), { title: "Soapstone", kind: "error" });
    }
  }

  /** Only the active name changes: secret values are never read or rewritten. */
  async function switchEnvironment(active: string | null) {
    if (projectId === null) return;
    try {
      await api.setActiveEnvironment(projectId, active);
      setEnvironments(await api.getEnvironments(projectId));
    } catch (e) {
      await message(String(e), { title: "Soapstone", kind: "error" });
    }
  }

  /** The dialog edits revealed values, read from the keychain now (the system may ask first). */
  async function manageEnvironments() {
    if (projectId === null) return;
    try {
      setManaging(await api.getEnvironments(projectId, true));
    } catch (e) {
      await message(String(e), { title: "Soapstone", kind: "error" });
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setQuickOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const definedVars = useMemo(() => definedVariables(environments), [environments]);
  const variables = useMemo(() => variableInfo(environments), [environments]);

  return (
    <Split storageKey="sidebar" initial={0.22} min={0.12}>
      {projectId === null ? (
        <aside className="sidebar" />
      ) : (
        <Sidebar
          projects={projects}
          projectId={projectId}
          tree={tree}
          requestId={requestId}
          onSelectProject={(id) => void selectProject(id).catch((e) => setError(String(e)))}
          onOpenRequest={openRequest}
          onChanged={changed}
        />
      )}
      <main className="main">
        <div className="topbar">
          <select
            aria-label="Environment"
            value={environments.active ?? ""}
            onChange={(e) => void switchEnvironment(e.target.value || null)}
          >
            <option value="">No environment</option>
            {environments.environments.map((env) => (
              <option key={env.name} value={env.name}>
                {env.name}
              </option>
            ))}
          </select>
          <button onClick={() => void manageEnvironments()}>Manage environments</button>
          <button disabled={projectId === null} onClick={() => { window.dispatchEvent(new Event("soapstone:flush")); setSequence(true); }}>Test sequence</button>
          <span className="spacer" />
          <button className="quiet" onClick={() => setQuickOpen(true)} title="Go to request (⌘P / Ctrl+P)">
            Go to request…
          </button>
        </div>
        {error ? (
          <div className="empty error-text">{error}</div>
        ) : requestId === null ? (
          <div className="empty">Import a WSDL or pick a request.</div>
        ) : (
          <Suspense fallback={<div className="empty">Loading editor…</div>}>
            <RequestView key={requestId} requestId={requestId} definedVars={definedVars} variables={variables} switchStart={switchStart} />
          </Suspense>
        )}
        {managing && (
          <EnvironmentDialog initial={managing} onClose={() => setManaging(null)} onSave={(envs) => void saveEnvironments(envs)} />
        )}
        {quickOpen && (
          <QuickOpen
            tree={tree}
            onClose={() => setQuickOpen(false)}
            onOpen={(id) => {
              setQuickOpen(false);
              openRequest(id);
            }}
          />
        )}
        {sequence && projectId !== null && <Suspense fallback={<div className="empty">Loading test sequence…</div>}>
          <SequenceDialog projectId={projectId} tree={tree} onClose={() => setSequence(false)} />
        </Suspense>}
      </main>
    </Split>
  );
}
