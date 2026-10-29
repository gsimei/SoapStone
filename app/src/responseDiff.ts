/** A bounded comparison of the changed region, without a quadratic allocation for large documents. */
export function responseDiff(before: string, after: string): string {
  if (before === after) return "No differences.";
  const a = before.split("\n"), b = after.split("\n");
  let first = 0;
  while (first < a.length && first < b.length && a[first] === b[first]) first++;
  let endA = a.length, endB = b.length;
  while (endA > first && endB > first && a[endA - 1] === b[endB - 1]) { endA--; endB--; }
  const lines = [`@@ from line ${first + 1} @@`];
  for (const line of a.slice(Math.max(0, first - 2), first)) lines.push(`  ${line}`);
  for (const [prefix, values] of [["- ", a.slice(first, endA)], ["+ ", b.slice(first, endB)]] as const) {
    lines.push(...values.slice(0, 250).map((line) => prefix + line.slice(0, 2000)));
    if (values.length > 250) lines.push(`${prefix}… ${values.length - 250} more lines`);
  }
  for (const line of a.slice(endA, endA + 2)) lines.push(`  ${line}`);
  return lines.join("\n");
}
