// @vitest-environment jsdom
import { expect, test, vi } from "vitest";
import { evaluateStep, runSequence, type StepResult } from "./sequence";
import type { HttpResponse, RequestEdit, TestStep } from "./api";
import { responseDiff } from "./responseDiff";

const step: TestStep = { requestId: 1, expectedStatus: 200, notFault: true, xpath: "string(//*[local-name(.)='Token'])", expectedValue: null, extractVariable: "token" };
const response: HttpResponse = { status: 200, statusText: "OK", timeMs: 5, size: 0, body: "<r xmlns='urn:response'><Token>abc</Token></r>", headers: [], pretty: null, fault: null };

test("XPath extraction supports namespaced SOAP XML and assertions fail clearly", () => {
  expect(evaluateStep(step, response)).toBe("abc");
  expect(() => evaluateStep(step, { ...response, status: 500 })).toThrow("Expected HTTP 200");
  expect(() => evaluateStep(step, { ...response, fault: "Bad credentials" })).toThrow("SOAP Fault");
  expect(() => evaluateStep({ ...step, expectedValue: "different" }, response)).toThrow("XPath value did not match");
  expect(() => evaluateStep(step, { ...response, body: "<broken>" })).toThrow("not valid XML");
});

test("extracted values reach only later steps and an assertion stops the sequence", async () => {
  const send = vi.fn().mockResolvedValueOnce(response).mockResolvedValueOnce({ ...response, status: 403 });
  const getRequest = vi.fn(async (id: number) => ({ id } as RequestEdit));
  const results: StepResult[] = [];
  await runSequence([step, { ...step, requestId: 2 }, { ...step, requestId: 3 }], { getRequest, send }, () => false, (r) => results.push(r), () => {});
  expect(send.mock.calls[0][1]).toEqual({});
  expect(send.mock.calls[1][1]).toEqual({ token: "abc" });
  expect(send).toHaveBeenCalledTimes(2);
  expect(results.map((r) => r.passed)).toEqual([true, false]);
});

test("stop during request loading never sends the next request", async () => {
  let stopped = false;
  const send = vi.fn();
  await runSequence([step], { getRequest: async () => { stopped = true; return { id: 1 } as RequestEdit; }, send }, () => stopped, () => {}, () => {});
  expect(send).not.toHaveBeenCalled();
});

test("response comparison bounds large differences and includes both values", () => {
  expect(responseDiff("same", "same")).toBe("No differences.");
  expect(responseDiff("<r>old</r>", "<r>new</r>")).toContain("- <r>old</r>\n+ <r>new</r>");
  expect(responseDiff("old\n".repeat(5000), "new\n".repeat(5000)).split("\n").length).toBeLessThan(510);
});
