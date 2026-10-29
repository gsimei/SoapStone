// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type HttpResponse, type Request } from "./api";
import { RequestView } from "./RequestView";

vi.mock("./api", () => ({ api: { getRequest: vi.fn(), send: vi.fn(), cancelSend: vi.fn(), saveRequest: vi.fn(), logMetric: vi.fn(), history: vi.fn(), formatXml: vi.fn(), preview: vi.fn(), sample: vi.fn() } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ message: vi.fn(), open: vi.fn(), ask: vi.fn() }));
vi.mock("./XmlEditor", () => ({ XmlEditor: (props: { value: string; readOnly?: boolean; onChange?(body: string): void }) =>
  <textarea aria-label={props.readOnly ? "Response XML" : "Request XML"} value={props.value} readOnly={props.readOnly} onChange={(e) => props.onChange?.(e.target.value)} /> }));

const response: HttpResponse = { status: 200, statusText: "OK", timeMs: 12, size: 4, body: "<r/>", headers: [], pretty: null, fault: null };
function request(id: number): Request { return { id, name: "Request 1", operation: "Op", binding: "B", bindingNamespace: "urn:test", soapVersion: "1.1", soapAction: "Op", endpoint: "http://localhost/service", body: "<request/>", headers: [], auth: { type: "none" }, wsSecurity: { usernameToken: false, username: "", password: "", passwordType: "text", nonce: false, created: false, timestamp: false, ttlSeconds: 300 }, options: { timeoutSeconds: 60, insecure: false, certPath: "", certPassword: "" }, lastResponse: null }; }
function view(id: number) { return <RequestView key={id} requestId={id} definedVars={new Set(["username"])} switchStart={null} />; }

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getRequest).mockImplementation(async (id) => request(id));
  vi.mocked(api.saveRequest).mockResolvedValue();
  vi.mocked(api.cancelSend).mockResolvedValue();
  vi.mocked(api.history).mockResolvedValue([]);
  vi.mocked(api.logMetric).mockResolvedValue();
  vi.mocked(api.formatXml).mockResolvedValue("<r/>");
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", { configurable: true, value() { this.setAttribute("open", ""); } });
});
afterEach(cleanup);

test("navigation preserves the running state and cancellation addresses its execution", async () => {
  let finish!: (response: HttpResponse) => void;
  vi.mocked(api.send).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const ui = render(view(101));
  fireEvent.click(await screen.findByRole("button", { name: "Send" }));
  await waitFor(() => expect(api.send).toHaveBeenCalledTimes(1));
  const runId = vi.mocked(api.send).mock.calls[0][1];
  ui.rerender(view(102));
  await screen.findByRole("button", { name: "Send" });
  ui.rerender(view(101));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(api.cancelSend).toHaveBeenCalledWith(101, runId);
  expect(api.send).toHaveBeenCalledTimes(1);
  await act(async () => finish(response));
  await screen.findByRole("button", { name: "Send" });
});

test("editing and immediate navigation flush the draft without waiting for debounce", async () => {
  const ui = render(view(201));
  fireEvent.change(await screen.findByLabelText("Request XML"), { target: { value: "<edited/>" } });
  ui.rerender(view(202));
  await waitFor(() => expect(api.saveRequest).toHaveBeenCalledWith(expect.objectContaining({ id: 201, body: "<edited/>" })));
});

test("formatting updates the editor and a preview does not send the request", async () => {
  vi.mocked(api.formatXml).mockResolvedValue("<formatted/>");
  vi.mocked(api.preview).mockResolvedValue({ endpoint: "http://localhost/service", headers: [], body: "<Password>[redacted]</Password>" });
  render(view(301));
  fireEvent.click(await screen.findByRole("button", { name: "Format XML" }));
  await waitFor(() => expect((screen.getByLabelText("Request XML") as HTMLTextAreaElement).value).toBe("<formatted/>"));
  fireEvent.click(screen.getByRole("button", { name: "Preview" }));
  await screen.findByRole("heading", { name: "Request preview" });
  expect(api.send).not.toHaveBeenCalled();
  expect((screen.getByLabelText("Response XML") as HTMLTextAreaElement).value).toContain("[redacted]");
});

test("reopening a request waits for its pending autosave before loading", async () => {
  let finish!: () => void;
  let saved = false;
  vi.mocked(api.saveRequest).mockImplementation(() => new Promise<void>((resolve) => { finish = () => { saved = true; resolve(); }; }));
  vi.mocked(api.getRequest).mockImplementation(async (id) => ({ ...request(id), body: id === 401 && saved ? "<latest/>" : "<request/>" }));
  const ui = render(view(401));
  fireEvent.change(await screen.findByLabelText("Request XML"), { target: { value: "<latest/>" } });
  ui.rerender(view(402));
  await waitFor(() => expect(api.saveRequest).toHaveBeenCalled());
  ui.rerender(view(401));
  expect(screen.queryByLabelText("Request XML")).toBeNull();
  await act(async () => finish());
  await waitFor(() => expect((screen.getByLabelText("Request XML") as HTMLTextAreaElement).value).toBe("<latest/>"));
});
