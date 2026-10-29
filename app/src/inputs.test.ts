import { expect, test } from "vitest";

const sources = import.meta.glob("./*.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

test("every text input turns off WebKit's spelling and text substitutions", () => {
  // macOS WebKit would otherwise curl quotes and capitalize letters in URLs, header values and variable names.
  const bare = Object.entries(sources).flatMap(([file, src]) =>
    [...src.matchAll(/<input\b[^]*?\/>/g)]
      .map((m) => m[0])
      .filter((tag) => !tag.includes("{...plain}") && !tag.includes('type="checkbox"'))
      .map((tag) => `${file}: ${tag.replace(/\s+/g, " ").slice(0, 70)}`),
  );
  expect(bare).toEqual([]);
});
