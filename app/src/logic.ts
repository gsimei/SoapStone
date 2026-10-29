// Pure helpers: no Tauri imports, tested by logic.test.ts.
import type { Environment, Environments, ImportReport, ServiceNode, Variable } from "./api";

/** `{{name}}` with optional spaces inside the braces; the same grammar as the Rust renderer. */
export const VARIABLE_RE = /\{\{ *([A-Za-z_][\w.-]*) *(?:\|raw *)?\}\}/g;

export type VariableInfo = Variable & { source: string };
export function variableInfo(envs: Environments): VariableInfo[] {
  const vars = new Map<string, VariableInfo>();
  for (const v of envs.base) if (v.name.trim()) vars.set(v.name.trim(), { ...v, name: v.name.trim(), source: "Base variables" });
  const active = envs.environments.find((e) => e.name === envs.active);
  for (const v of active?.variables ?? []) if (v.name.trim()) vars.set(v.name.trim(), { ...v, name: v.name.trim(), source: active!.name });
  return [...vars.values()];
}

export const validVariableName = (name: string) => /^[A-Za-z_][\w.-]*$/.test(name);

/** Base variables plus the active environment's, non-empty names only. */
export function definedVariables(envs: Environments): Set<string> {
  const active = envs.environments.find((e) => e.name === envs.active);
  const all = [...envs.base, ...(active?.variables ?? [])];
  return new Set(all.map((v) => v.name.trim()).filter(Boolean));
}

/** Every space-separated term of `query` is a case-insensitive substring of `text`. */
export function matches(text: string, query: string): boolean {
  const haystack = text.toLowerCase();
  return query.toLowerCase().split(/\s+/).filter(Boolean).every((term) => haystack.includes(term));
}

/** Keeps operations whose service, operation or request names match; drops services left empty. */
export function filterTree(tree: ServiceNode[], query: string): ServiceNode[] {
  if (!query.trim()) return tree;
  return tree
    .map((s) => ({
      ...s,
      operations: s.operations.filter((o) => matches(`${s.name} ${o.binding} ${o.bindingNamespace ?? ""} ${o.name} ${o.requests.map((r) => r.name).join(" ")}`, query)),
    }))
    .filter((s) => s.operations.length > 0);
}

export type QuickItem = { requestId: number; label: string };

export function quickOpenItems(tree: ServiceNode[]): QuickItem[] {
  return tree.flatMap((s) =>
    s.operations.flatMap((o) => {
      const ambiguous = s.operations.some((other) => other.id !== o.id && other.name === o.name);
      const binding = ambiguous ? `{${o.bindingNamespace ?? ""}}${o.binding} › ` : "";
      return o.requests.map((r) => ({ requestId: r.id, label: `${s.name} › ${binding}${o.name} › ${r.name}` }));
    }),
  );
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** The environment dialog's working copy: the active environment is a row index, so renames never move it. */
export type EnvDraft = { base: Variable[]; environments: Environment[]; activeIndex: number | null };

export function toDraft(e: Environments): EnvDraft {
  const i = e.environments.findIndex((env) => env.name === e.active);
  return { base: e.base, environments: e.environments, activeIndex: i < 0 ? null : i };
}

/** Names trimmed; the active environment saved by the (new) name of its row. */
export function fromDraft(d: EnvDraft): Environments {
  const environments = d.environments.map((e) => ({ ...e, name: e.name.trim() }));
  return { base: d.base, environments, active: d.activeIndex === null ? null : environments[d.activeIndex].name };
}

export function removeEnvironment(d: EnvDraft, index: number): EnvDraft {
  const a = d.activeIndex;
  return {
    ...d,
    environments: d.environments.filter((_, i) => i !== index),
    activeIndex: a === null || a === index ? null : a > index ? a - 1 : a,
  };
}

/** Spread on every text input: macOS WebKit must not curl quotes or capitalize URLs, header values and names. */
export const plain = { spellCheck: false, autoCorrect: "off", autoCapitalize: "off", autoComplete: "off" } as const;

/** A whole number from a number input: "" or junk is 0, negatives are 0 (the backend takes unsigned values). */
export function wholeNumber(text: string): number {
  return Math.max(0, Math.trunc(Number(text)) || 0);
}

const count = (n: number, noun: string) => `${n} ${noun}${n === 1 ? "" : "s"}`;

/** The text shown after a SoapUI import: what was created, then each note on its own line. */
export function importSummary(r: ImportReport): string {
  const summary = `Imported ${count(r.services, "service")} and ${count(r.requests, "request")}.`;
  return r.notes.length ? `${summary}\n\n${r.notes.join("\n")}` : summary;
}
