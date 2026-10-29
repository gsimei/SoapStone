import { useState, type ReactNode } from "react";
import { Menu, type MenuItemOptions, type CheckMenuItemOptions, type PredefinedMenuItemOptions } from "@tauri-apps/api/menu";
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { api, type Kind, type OperationNode, type Project, type ServiceNode } from "./api";
import { ImportDialog } from "./ImportDialog";
import { filterTree, importSummary, plain } from "./logic";

type MenuEntry = MenuItemOptions | CheckMenuItemOptions | PredefinedMenuItemOptions;
type Renaming = { kind: Kind; id: number; value: string };

const SEPARATOR: PredefinedMenuItemOptions = { item: "Separator" };

/** Runs a user action; failures go to an error dialog. */
async function run(action: () => Promise<unknown>) {
  try {
    await action();
  } catch (e) {
    await message(String(e), { title: "Soapstone", kind: "error" });
  }
}

async function popup(items: MenuEntry[]) {
  const menu = await Menu.new({ items });
  await menu.popup();
}

export function Sidebar(props: {
  projects: Project[];
  projectId: number;
  tree: ServiceNode[];
  requestId: number | null;
  onSelectProject(id: number): void;
  onOpenRequest(id: number): void;
  onChanged(): Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [renaming, setRenaming] = useState<Renaming | null>(null);
  const [refreshing, setRefreshing] = useState<number | null>(null);
  const [importing, setImporting] = useState(false);
  const [editingSource, setEditingSource] = useState<number | null>(null);

  const project = props.projects.find((p) => p.id === props.projectId);
  const searching = query.trim() !== "";
  const tree = filterTree(props.tree, query);
  const toggle = (set: Set<number>, id: number) => {
    const next = new Set(set);
    if (!next.delete(id)) next.add(id);
    return next;
  };

  async function confirmDelete(kind: Kind, id: number, what: string) {
    if (await ask(`Delete ${what}? This cannot be undone.`, { title: "Delete", kind: "warning" })) {
      await api.remove(kind, id);
      await props.onChanged();
    }
  }

  async function commitRename() {
    if (!renaming) return;
    const name = renaming.value.trim();
    if (!name) return;
    setRenaming(null);
    await run(async () => {
      await api.rename(renaming.kind, renaming.id, name);
      await props.onChanged();
    });
  }

  async function newRequest(op: OperationNode) {
    const id = await api.createRequest(op.id);
    setExpanded((s) => new Set(s).add(op.id));
    await props.onChanged();
    props.onOpenRequest(id);
  }

  async function imported(serviceId: number) {
    setImporting(false);
    await props.onChanged();
    const service = (await api.tree(props.projectId)).find((s) => s.id === serviceId);
    const first = service?.operations.find((o) => o.requests.length > 0)?.requests[0];
    if (first) props.onOpenRequest(first.id);
  }

  async function importSoapui() {
    const file = await open({ multiple: false, directory: false, filters: [{ name: "SoapUI project", extensions: ["xml"] }] });
    if (typeof file !== "string") return;
    const report = await api.importSoapuiProject(file);
    props.onSelectProject(report.projectId);
    await message(importSummary(report), { title: "Import SoapUI project", kind: report.notes.length ? "warning" : "info" });
  }

  function projectMenu() {
    if (!project) return;
    void run(() =>
      popup([
        ...props.projects.map((p) => ({
          text: p.name,
          checked: p.id === props.projectId,
          action: () => props.onSelectProject(p.id),
        })),
        SEPARATOR,
        {
          text: "New project",
          action: () =>
            void run(async () => {
              const id = await api.createProject("New Project");
              props.onSelectProject(id);
              setRenaming({ kind: "project", id, value: "New Project" });
            }),
        },
        { text: "Import SoapUI project…", action: () => void run(importSoapui) },
        { text: "Rename project", action: () => setRenaming({ kind: "project", id: project.id, value: project.name }) },
        {
          text: "Delete project",
          action: () => void run(() => confirmDelete("project", project.id, `project "${project.name}" and everything in it`)),
        },
      ]),
    );
  }

  function serviceMenu(s: ServiceNode) {
    void run(() =>
      popup([
        {
          text: "Refresh WSDL",
          action: () =>
            void run(async () => {
              setRefreshing(s.id);
              try {
                await api.refreshService(s.id);
                await props.onChanged();
              } finally {
                setRefreshing(null);
              }
            }),
        },
        { text: "Edit WSDL source…", action: () => setEditingSource(s.id) },
        SEPARATOR,
        { text: "Rename", action: () => setRenaming({ kind: "service", id: s.id, value: s.name }) },
        { text: "Delete", action: () => void run(() => confirmDelete("service", s.id, `service "${s.name}" and its requests`)) },
      ]),
    );
  }

  function operationMenu(op: OperationNode) {
    const only = op.requests.length === 1 ? op.requests[0] : null;
    void run(() =>
      popup([
        { text: "New request", action: () => void run(() => newRequest(op)) },
        ...(only
          ? [
              SEPARATOR,
              { text: "Rename request", action: () => setRenaming({ kind: "request", id: only.id, value: only.name }) },
              { text: "Delete request", action: () => void run(() => confirmDelete("request", only.id, `request "${only.name}"`)) },
            ]
          : []),
      ]),
    );
  }

  function requestMenu(id: number, name: string) {
    void run(() =>
      popup([
        { text: "Rename", action: () => setRenaming({ kind: "request", id, value: name }) },
        { text: "Delete", action: () => void run(() => confirmDelete("request", id, `request "${name}"`)) },
      ]),
    );
  }

  function openOperation(op: OperationNode) {
    if (op.requests.length === 1) props.onOpenRequest(op.requests[0].id);
    else if (op.requests.length > 1) setExpanded((s) => toggle(s, op.id));
    else void run(() => newRequest(op));
  }

  const renameInput = (kind: Kind, id: number) =>
    renaming?.kind === kind && renaming.id === id ? (
      <input {...plain}
        className="rename"
        autoFocus
        aria-label="Name"
        value={renaming.value}
        onChange={(e) => setRenaming({ ...renaming, value: e.target.value })}
        onFocus={(e) => e.target.select()}
        onKeyDown={(e) => {
          if (e.key === "Enter") void commitRename();
          if (e.key === "Escape") setRenaming(null);
        }}
        onBlur={() => (renaming.value.trim() ? void commitRename() : setRenaming(null))}
      />
    ) : null;

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        {renameInput("project", props.projectId) ?? (
          <button className="project-switch" onClick={projectMenu} title="Switch project">
            <span>{project?.name ?? "…"}</span>
            <span aria-hidden>▾</span>
          </button>
        )}
        <input {...plain}
          type="search"
          aria-label="Search operations"
          placeholder="Search operations"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button onClick={() => setImporting(true)}>Import WSDL</button>
      </div>

      <nav className="tree" aria-label="Services">
        {props.tree.length === 0 && <p className="muted tree-empty">No services yet.</p>}
        {searching && tree.length === 0 && props.tree.length > 0 && <p className="muted tree-empty">No matches.</p>}
        {tree.map((s) => {
          const open = searching || !collapsed.has(s.id);
          const showBinding = new Set(s.operations.map((o) => o.binding)).size > 1;
          return (
            <div key={s.id} role="group">
              <Row
                depth={0}
                className="service"
                onClick={() => setCollapsed((c) => toggle(c, s.id))}
                onMenu={() => serviceMenu(s)}
                rename={renameInput("service", s.id)}
                ariaExpanded={open}
              >
                <span className="chevron" aria-hidden>{open ? "▾" : "▸"}</span>
                <span className="label">{s.name}</span>
                {refreshing === s.id && <span className="muted">Refreshing…</span>}
              </Row>
              {open &&
                s.operations.map((op) => {
                  const single = op.requests.length === 1;
                  const multi = op.requests.length > 1;
                  const opOpen = multi && (searching || expanded.has(op.id));
                  return (
                    <div key={op.id}>
                      <Row
                        depth={1}
                        className={op.orphan ? "orphan" : ""}
                        active={!opOpen && op.requests.some((r) => r.id === props.requestId)}
                        title={op.orphan ? "Removed from the WSDL" : undefined}
                        onClick={() => openOperation(op)}
                        onMenu={() => operationMenu(op)}
                        rename={single ? renameInput("request", op.requests[0].id) : null}
                        ariaExpanded={multi ? opOpen : undefined}
                      >
                        <span className="chevron" aria-hidden>{multi ? (opOpen ? "▾" : "▸") : ""}</span>
                        <span className="label">{op.name}</span>
                        {showBinding && <span className="muted binding">{op.binding}</span>}
                      </Row>
                      {opOpen &&
                        op.requests.map((r) => (
                          <Row
                            key={r.id}
                            depth={2}
                            active={r.id === props.requestId}
                            onClick={() => props.onOpenRequest(r.id)}
                            onMenu={() => requestMenu(r.id, r.name)}
                            rename={renameInput("request", r.id)}
                          >
                            <span className="label">{r.name}</span>
                          </Row>
                        ))}
                    </div>
                  );
                })}
            </div>
          );
        })}
      </nav>

      {importing && (
        <ImportDialog projectId={props.projectId} onClose={() => setImporting(false)} onImported={(id) => void run(() => imported(id))} />
      )}
      {editingSource !== null && (
        <ImportDialog
          projectId={props.projectId}
          edit={{ serviceId: editingSource }}
          onClose={() => setEditingSource(null)}
          onImported={() =>
            void run(async () => {
              setEditingSource(null);
              await props.onChanged();
            })
          }
        />
      )}
    </aside>
  );
}

function Row(props: {
  depth: number;
  className?: string;
  active?: boolean;
  title?: string;
  ariaExpanded?: boolean;
  rename?: ReactNode;
  onClick?(): void;
  onMenu?(): void;
  children: ReactNode;
}) {
  const cls = ["row", `depth-${props.depth}`, props.className, props.active && "active"].filter(Boolean).join(" ");
  return (
    <div
      className={cls}
      onContextMenu={(e) => {
        e.preventDefault();
        props.onMenu?.();
      }}
    >
      {props.rename ?? (
        <button className="row-main" onClick={props.onClick} title={props.title} aria-expanded={props.ariaExpanded} aria-current={props.active || undefined}>
          {props.children}
        </button>
      )}
      {props.onMenu && !props.rename && (
        <button className="row-menu" aria-label="More actions" onClick={props.onMenu}>
          ⋯
        </button>
      )}
    </div>
  );
}
