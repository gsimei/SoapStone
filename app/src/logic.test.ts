import { expect, test } from "vitest";
import { VARIABLE_RE, definedVariables, filterTree, formatSize, fromDraft, matches, quickOpenItems, importSummary, removeEnvironment, toDraft, wholeNumber } from "./logic";
import type { ServiceNode } from "./api";

const tree: ServiceNode[] = [
  { id: 1, name: "wbs_methods", operations: [
    { id: 10, binding: "wbs_methodsSoap", name: "AVITIK_GETGROUPRESERVES", orphan: false, requests: [{ id: 100, name: "Request 1" }] },
    { id: 11, binding: "wbs_methodsSoap", name: "CreateOptitravelReserve", orphan: false, requests: [{ id: 101, name: "Request 1" }, { id: 102, name: "With traveller" }] },
  ] },
  { id: 2, name: "Calculator", operations: [
    { id: 20, binding: "CalculatorSoap", name: "Add", orphan: false, requests: [{ id: 200, name: "Request 1" }] },
  ] },
];

test("VARIABLE_RE follows the Rust grammar", () => {
  expect([..."{{a}} {{ b }} {{a b}} {{}} {{1x}} {{c.d-e_f}}".matchAll(VARIABLE_RE)].map((m) => m[1])).toEqual(["a", "b", "c.d-e_f"]);
});

test("definedVariables merges base and the active environment only", () => {
  const names = definedVariables({
    base: [{ name: "a", value: "" }, { name: "", value: "x" }],
    environments: [{ name: "dev", variables: [{ name: "b", value: "1" }] }, { name: "prod", variables: [{ name: "c", value: "1" }] }],
    active: "dev",
  });
  expect([...names].sort()).toEqual(["a", "b"]);
});

test("matches needs every term, case-insensitively", () => {
  expect(matches("AVITIK_GETGROUPRESERVES", "avitik group")).toBe(true);
  expect(matches("GetPrices", "get x")).toBe(false);
  expect(matches("anything", "  ")).toBe(true);
});

test("filterTree keeps matching operations and drops empty services", () => {
  expect(filterTree(tree, "")).toBe(tree);
  const names = (q: string) => filterTree(tree, q).map((s) => [s.name, s.operations.map((o) => o.name)]);
  expect(names("add")).toEqual([["Calculator", ["Add"]]]);
  expect(names("calc")).toEqual([["Calculator", ["Add"]]]);
  expect(names("traveller")).toEqual([["wbs_methods", ["CreateOptitravelReserve"]]]);
});

test("quickOpenItems lists every request with its path", () => {
  expect(quickOpenItems(tree).map((i) => [i.requestId, i.label])).toEqual([
    [100, "wbs_methods › AVITIK_GETGROUPRESERVES › Request 1"],
    [101, "wbs_methods › CreateOptitravelReserve › Request 1"],
    [102, "wbs_methods › CreateOptitravelReserve › With traveller"],
    [200, "Calculator › Add › Request 1"],
  ]);
});

test("quick open and search distinguish equal operation names across namespaces", () => {
  const tree: ServiceNode[] = [{ id: 1, name: "Service", operations: [
    { id: 1, binding: "B", bindingNamespace: "urn:first", name: "Op", orphan: false, requests: [{ id: 10, name: "Request 1" }] },
    { id: 2, binding: "B", bindingNamespace: "urn:second", name: "Op", orphan: false, requests: [{ id: 20, name: "Request 1" }] },
  ] }];
  expect(new Set(quickOpenItems(tree).map((i) => i.label)).size).toBe(2);
  expect(filterTree(tree, "urn:second")[0].operations.map((o) => o.id)).toEqual([2]);
});

test("formatSize", () => {
  expect([formatSize(512), formatSize(4198), formatSize(2411724)]).toEqual(["512 B", "4.1 KB", "2.3 MB"]);
});

const envs = {
  base: [],
  environments: [{ name: "dev", variables: [] }, { name: "prod", variables: [] }, { name: "qa", variables: [] }],
  active: "prod",
};

test("the active environment follows its row, not its name", () => {
  const draft = toDraft(envs);
  // Renaming the active one keeps it active.
  const renamed = { ...draft, environments: draft.environments.map((e, i) => (i === 1 ? { ...e, name: "production" } : e)) };
  expect(fromDraft(renamed).active).toBe("production");
  // Typing "prod-eu" into another row passes through "prod" without stealing the active environment.
  const typed = { ...draft, environments: draft.environments.map((e, i) => (i === 2 ? { ...e, name: " prod-eu " } : e)) };
  expect(fromDraft(typed)).toMatchObject({ active: "prod", environments: [{ name: "dev" }, { name: "prod" }, { name: "prod-eu" }] });
});

test("removing an environment keeps or clears the active one", () => {
  expect(fromDraft(removeEnvironment(toDraft(envs), 0)).active).toBe("prod");
  expect(fromDraft(removeEnvironment(toDraft(envs), 2)).active).toBe("prod");
  expect(fromDraft(removeEnvironment(toDraft(envs), 1)).active).toBeNull();
  expect(fromDraft(toDraft({ ...envs, active: "gone" })).active).toBeNull();
});

test("wholeNumber never yields a negative or fractional value", () => {
  expect(["30", "", "abc", "-5", "2.7"].map(wholeNumber)).toEqual([30, 0, 0, 0, 2]);
});

test("importSummary counts what was imported, then lists the notes", () => {
  expect(importSummary({ projectId: 2, services: 1, requests: 9, notes: [] })).toBe("Imported 1 service and 9 requests.");
  expect(importSummary({ projectId: 2, services: 2, requests: 1, notes: ["a", "b"] })).toBe("Imported 2 services and 1 request.\n\na\nb");
});
