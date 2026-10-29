// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, render, waitFor } from "@testing-library/react";
import { createElement } from "react";
import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { variableInfo, VARIABLE_RE } from "./logic";
vi.hoisted(() => { Object.defineProperty(window, "matchMedia", { configurable: true, value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) }); });
import { variableCompletion, XmlEditor } from "./XmlEditor";
afterEach(cleanup);

test("external XML edits update the existing CodeMirror document", async () => {
  Object.defineProperty(Range.prototype, "getClientRects", { configurable: true, value: () => [] });
  Object.defineProperty(Range.prototype, "getBoundingClientRect", { configurable: true, value: () => new DOMRect(0, 0, 100, 20) });
  const ui = render(createElement(XmlEditor, { docKey: 1, value: "<a/>" }));
  const content = ui.container.querySelector(".cm-content");
  expect(content?.textContent).toBe("<a/>");
  ui.rerender(createElement(XmlEditor, { docKey: 1, value: "<formatted/>" }));
  await waitFor(() => expect(content?.textContent).toBe("<formatted/>"));
  expect(ui.container.querySelector(".cm-content")).toBe(content);
});

test("active variables carry their origin while secret values stay hidden in completion", () => {
  const vars = variableInfo({ base: [{ name: "host", value: "base" }], environments: [{ name: "dev", variables: [{ name: "host", value: "development" }, { name: "password", value: "must-not-appear", secret: true }] }], active: "dev" });
  expect(vars.find((v) => v.name === "host")).toMatchObject({ source: "dev", value: "development" });
  const state = EditorState.create({ doc: "<x>{{pa}}</x>" });
  const result = variableCompletion(vars)(new CompletionContext(state, 7, false));
  expect(result).not.toBeNull();
  expect(JSON.stringify(result)).not.toContain("must-not-appear");
  expect(result?.options.find((c) => c.label === "{{password}}")).toMatchObject({ info: "Secret value (hidden)", detail: "dev" });
  expect([..."{{password|raw}} {{ host }} {{\thost}}".matchAll(VARIABLE_RE)].map((m) => m[1])).toEqual(["password", "host"]);
});
