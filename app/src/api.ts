// IPC wrappers. Types mirror the Rust serde output exactly.
import { invoke } from "@tauri-apps/api/core";

export type Project = { id: number; name: string };
export type Header = { name: string; value: string };
/** `secret`: the value lives in the system keychain (absent on rows the user just added). */
export type Variable = { name: string; value: string; secret?: boolean };
export type Auth =
  | { type: "none" }
  | { type: "basic"; username: string; password: string }
  | { type: "bearer"; token: string }
  | { type: "ntlm"; username: string; password: string; domain: string };
export type WsSecurity = {
  usernameToken: boolean;
  username: string;
  password: string;
  passwordType: "text" | "digest";
  nonce: boolean;
  created: boolean;
  timestamp: boolean;
  ttlSeconds: number;
};
export type RequestOptions = { timeoutSeconds: number; insecure: boolean; certPath: string; certPassword: string };
export type FetchSettings = { username: string; password: string; insecure: boolean };
export type ServiceSource = { projectId: number; url: string; fetch: FetchSettings };
export type Environment = { name: string; variables: Variable[] };
export type Environments = { base: Variable[]; environments: Environment[]; active: string | null };
export type RequestNode = { id: number; name: string };
export type OperationNode = { id: number; binding: string; name: string; orphan: boolean; requests: RequestNode[] };
export type ServiceNode = { id: number; name: string; operations: OperationNode[] };
export type HttpResponse = {
  status: number;
  statusText: string;
  timeMs: number;
  size: number;
  headers: Header[];
  body: string;
  pretty: string | null;
  fault: string | null;
};
export type Request = {
  id: number;
  name: string;
  operation: string;
  binding: string;
  soapVersion: "1.1" | "1.2";
  soapAction: string | null;
  endpoint: string;
  body: string;
  headers: Header[];
  auth: Auth;
  wsSecurity: WsSecurity;
  options: RequestOptions;
  lastResponse: HttpResponse | null;
};
export type RequestEdit = Pick<Request, "id" | "endpoint" | "body" | "headers" | "auth" | "wsSecurity" | "options">;
export type Kind = "project" | "service" | "request";
export type ImportReport = { projectId: number; services: number; requests: number; notes: string[] };

export const api = {
  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string) => invoke<number>("create_project", { name }),
  rename: (kind: Kind, id: number, name: string) => invoke<void>("rename", { kind, id, name }),
  remove: (kind: Kind, id: number) => invoke<void>("delete", { kind, id }),
  tree: (projectId: number) => invoke<ServiceNode[]>("get_tree", { projectId }),
  importService: (projectId: number, source: string, fetch: FetchSettings) =>
    invoke<number>("import_service", { projectId, source, fetch }),
  /** With `source` and `fetch`: download from there and keep them if it works. Otherwise the stored ones. */
  refreshService: (serviceId: number, source?: string, fetch?: FetchSettings) =>
    invoke<void>("refresh_service", { serviceId, source, fetch }),
  getServiceSource: (serviceId: number) => invoke<ServiceSource>("get_service_source", { serviceId }),
  getRequest: (id: number) => invoke<Request>("get_request", { id }),
  saveRequest: (edit: RequestEdit) => invoke<void>("save_request", { edit }),
  createRequest: (operationId: number) => invoke<number>("create_request", { operationId }),
  send: (edit: RequestEdit) => invoke<HttpResponse>("send_request", { edit }),
  cancelSend: (requestId: number) => invoke<void>("cancel_send", { requestId }),
  /** `reveal`: secret values from the system keychain (may ask the user); otherwise they are empty. */
  getEnvironments: (projectId: number, reveal = false) => invoke<Environments>("get_environments", { projectId, reveal }),
  /** Resolves to a warning when secrets could not go to the system keychain. */
  setEnvironments: (projectId: number, environments: Environments) =>
    invoke<string | null>("set_environments", { projectId, environments }),
  setActiveEnvironment: (projectId: number, active: string | null) =>
    invoke<void>("set_active_environment", { projectId, active }),
  importSoapuiProject: (path: string) => invoke<ImportReport>("import_soapui_project", { path }),
  logMetric: (name: string, ms?: number) => invoke<void>("log_metric", { name, ms }),
};
