import type { HttpResponse, RequestEdit, TestStep } from "./api";

export type StepResult = { index: number; passed: boolean; message: string; timeMs?: number };
export type SequenceClient = {
  getRequest(id: number): Promise<RequestEdit>;
  send(edit: RequestEdit, variables: Record<string, string>): Promise<HttpResponse>;
};

export function evaluateStep(step: TestStep, response: HttpResponse): string | null {
  if (step.expectedStatus !== null && step.expectedStatus !== response.status) throw new Error(`Expected HTTP ${step.expectedStatus}, received ${response.status}.`);
  if (step.notFault && response.fault) throw new Error(`SOAP Fault: ${response.fault}`);
  if (!step.xpath.trim()) return null;
  const doc = new DOMParser().parseFromString(response.body, "application/xml");
  if (doc.querySelector("parsererror")) throw new Error("The response is not valid XML.");
  const result = doc.evaluate(step.xpath, doc, (prefix) => doc.documentElement.lookupNamespaceURI(prefix), XPathResult.STRING_TYPE, null).stringValue;
  if (step.expectedValue !== null && result !== step.expectedValue) throw new Error("XPath value did not match the expected value.");
  if (step.extractVariable && !result) throw new Error(`XPath returned no value for ${step.extractVariable}.`);
  return result;
}

/** Extracted values exist only for this run; they never overwrite project variables or stored XML. */
export async function runSequence(steps: TestStep[], client: SequenceClient, cancelled: () => boolean, report: (result: StepResult) => void, started: (id: number) => void) {
  const variables: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const [index, step] of steps.entries()) {
    if (cancelled()) return;
    try {
      started(step.requestId);
      const request = await client.getRequest(step.requestId);
      if (cancelled()) return;
      const response = await client.send(request, { ...variables });
      if (cancelled()) return;
      const extracted = evaluateStep(step, response);
      if (step.extractVariable && extracted !== null) variables[step.extractVariable] = extracted;
      report({ index, passed: true, message: "Passed", timeMs: response.timeMs });
    } catch (e) {
      report({ index, passed: false, message: cancelled() ? "Stopped" : String(e) });
      return;
    }
  }
}
