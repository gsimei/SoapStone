import { useSyncExternalStore } from "react";
import { api, type RequestEdit } from "./api";

type Execution = { runId: string | null; error: string | null; revision: number };
const idle: Execution = { runId: null, error: null, revision: 0 };
const executions = new Map<number, Execution>();
const listeners = new Set<() => void>();
const saves = new Map<number, Promise<void>>();
const runningLocally = new Set<string>();
let polling: ReturnType<typeof setTimeout> | undefined;

function publish(id: number, value: Execution) {
  executions.set(id, value);
  for (const listener of listeners) listener();
}

export function currentExecution(id: number): Execution { return executions.get(id) ?? idle; }

export async function waitForSaves(id?: number) {
  await Promise.all(id === undefined ? [...saves.values()] : [saves.get(id)]);
}

export async function loadRequest(id: number) {
  await waitForSaves(id);
  return api.getRequest(id);
}

export function useExecution(id: number): Execution {
  return useSyncExternalStore((listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; }, () => currentExecution(id));
}

/** Serialize autosaves, including the flush performed while navigating away. */
export function saveRequest(edit: RequestEdit): Promise<void> {
  const next = (saves.get(edit.id) ?? Promise.resolve()).catch(() => {}).then(() => api.saveRequest(edit));
  saves.set(edit.id, next);
  void next.finally(() => { if (saves.get(edit.id) === next) saves.delete(edit.id); }).catch(() => {});
  return next;
}

export async function sendRequest(edit: RequestEdit, variables?: Record<string, string>) {
  if (currentExecution(edit.id).runId) throw new Error("This request is already running.");
  const runId = crypto.randomUUID();
  runningLocally.add(runId);
  const revision = currentExecution(edit.id).revision;
  publish(edit.id, { runId, error: null, revision });
  let error: string | null = null;
  try {
    await saves.get(edit.id);
    return await api.send(edit, runId, variables);
  } catch (e) { error = String(e); throw e; }
  finally {
    runningLocally.delete(runId);
    if (currentExecution(edit.id).runId === runId) publish(edit.id, { runId: null, error, revision: revision + 1 });
  }
}

export async function cancelRequest(id: number) {
  const runId = currentExecution(id).runId;
  if (runId) await api.cancelSend(id, runId);
}

/** Restore sends after a WebView reload; local sends update their own state without polling. */
export async function restoreExecutions() {
  clearTimeout(polling);
  const active = await api.activeRuns();
  for (const run of active) {
    if (!currentExecution(run.requestId).runId) publish(run.requestId, { ...currentExecution(run.requestId), runId: run.runId });
  }
  for (const [id, state] of executions) {
    if (state.runId && !runningLocally.has(state.runId) && !active.some((r) => r.runId === state.runId)) {
      publish(id, { runId: null, error: null, revision: state.revision + 1 });
    }
  }
  if (active.some((run) => !runningLocally.has(run.runId))) polling = setTimeout(() => void restoreExecutions().catch(() => {}), 1000);
}
