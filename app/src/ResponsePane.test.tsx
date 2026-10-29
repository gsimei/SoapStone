// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type HttpResponse } from "./api";
import { ResponsePane } from "./ResponsePane";

vi.mock("./api", () => ({ api: { history: vi.fn(), historyResponse: vi.fn(), formatXml: vi.fn() } }));
vi.mock("./XmlEditor", () => ({ XmlEditor: (props: { value: string }) => <pre data-testid="xml-editor">{props.value}</pre> }));
afterEach(cleanup);
beforeEach(() => { vi.clearAllMocks(); vi.mocked(api.history).mockResolvedValue([]); });

test("only the selected response editor mounts and historical responses can be compared", async () => {
  const latest: HttpResponse = { status: 200, statusText: "OK", timeMs: 1, size: 13, body: "<r>latest</r>", headers: [{ name: "Content-Type", value: "text/xml" }], pretty: null, fault: null };
  vi.mocked(api.history).mockResolvedValue([{ id: 9, createdAt: "2026-10-29T12:00:00Z", status: 200, timeMs: 2, size: 10 }]);
  vi.mocked(api.historyResponse).mockResolvedValue({ ...latest, body: "<r>old</r>" });
  vi.mocked(api.formatXml).mockImplementation(async (xml) => xml);
  render(<ResponsePane requestId={1} response={latest} error={null} sending={false} />);
  expect(screen.getAllByTestId("xml-editor")).toHaveLength(1);
  fireEvent.click(screen.getByRole("tab", { name: "Headers (1)" }));
  expect(screen.queryByTestId("xml-editor")).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: "Raw" }));
  expect(screen.getAllByTestId("xml-editor")).toHaveLength(1);
  await screen.findByRole("option", { name: /200.*2 ms/ });
  fireEvent.change(screen.getByLabelText("Response history"), { target: { value: "9" } });
  await waitFor(() => expect(screen.getByTestId("xml-editor").textContent).toBe("<r>old</r>"));
  fireEvent.click(screen.getByRole("tab", { name: "Compare with latest" }));
  expect(screen.queryByTestId("xml-editor")).toBeNull();
  expect(screen.getByText(/- <r>old<\/r>/).textContent).toContain("+ <r>latest</r>");
});

test("large XML formats automatically while Original and Raw preserve the received text", async () => {
  const value = "x".repeat(300 * 1024);
  const body = `<?xml version="1.0"?><r><value>${value}</value></r>`;
  const formatted = `<?xml version="1.0"?>\n<r>\n   <value>${value}</value>\n</r>`;
  const response: HttpResponse = { status: 200, statusText: "OK", timeMs: 1, size: body.length, body, headers: [], pretty: null, fault: null };
  vi.mocked(api.formatXml).mockResolvedValue(formatted);
  render(<ResponsePane response={response} error={null} sending={false} />);
  await waitFor(() => expect(screen.getByTestId("xml-editor").textContent).toBe(formatted));
  expect(api.formatXml).toHaveBeenCalledWith(body);
  expect(screen.queryByRole("button", { name: "Format XML" })).toBeNull();
  fireEvent.change(screen.getByLabelText("Response format"), { target: { value: "original" } });
  expect(screen.getByTestId("xml-editor").textContent).toBe(body);
  fireEvent.click(screen.getByRole("tab", { name: "Raw" }));
  expect(screen.getByTestId("xml-editor").textContent).toBe(body);
  fireEvent.click(screen.getByRole("tab", { name: "Body" }));
  expect((screen.getByLabelText("Response format") as HTMLSelectElement).value).toBe("original");
  fireEvent.change(screen.getByLabelText("Response format"), { target: { value: "auto" } });
  await waitFor(() => expect(screen.getByTestId("xml-editor").textContent).toBe(formatted));
  expect(api.formatXml).toHaveBeenCalledTimes(1);
});

test("automatic display keeps non-XML unchanged and allows an explicit format choice", async () => {
  const body = "Service unavailable\n  original spacing";
  const response: HttpResponse = { status: 503, statusText: "Unavailable", timeMs: 1, size: body.length, body, headers: [], pretty: null, fault: null };
  vi.mocked(api.formatXml).mockRejectedValue("Not valid XML");
  render(<ResponsePane response={response} error={null} sending={false} />);
  expect(screen.getByTestId("xml-editor").textContent).toBe(body);
  expect(api.formatXml).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("Response format"), { target: { value: "xml" } });
  await screen.findByRole("alert");
  expect(screen.getByTestId("xml-editor").textContent).toBe(body);
  fireEvent.change(screen.getByLabelText("Response format"), { target: { value: "auto" } });
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});

test("late formatting never replaces a newer response", async () => {
  const first: HttpResponse = { status: 200, statusText: "OK", timeMs: 1, size: 0, body: "<first><x/></first>", headers: [], pretty: null, fault: null };
  const second = { ...first, body: "<second><x/></second>" };
  let finish!: (text: string) => void;
  vi.mocked(api.formatXml).mockImplementation((body) => body === first.body ? new Promise((resolve) => { finish = resolve; }) : Promise.resolve("<second>\n   <x/>\n</second>"));
  const ui = render(<ResponsePane response={first} error={null} sending={false} />);
  ui.rerender(<ResponsePane response={second} error={null} sending={false} />);
  await waitFor(() => expect(screen.getByTestId("xml-editor").textContent).toBe("<second>\n   <x/>\n</second>"));
  await act(async () => finish("<first>\n   <x/>\n</first>"));
  expect(screen.getByTestId("xml-editor").textContent).toBe("<second>\n   <x/>\n</second>");
});
