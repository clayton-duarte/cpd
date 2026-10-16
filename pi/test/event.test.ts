import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { LEAD_MESSAGES } from "../lib/delivery.ts";
import { KINDS } from "../lib/event.ts";

/** The kinds of contract.md §5.2's table: the backticked names of the first column of its rows. */
function contractKinds(): string[] {
  const doc = readFileSync(new URL("../../docs/contract.md", import.meta.url), "utf8");
  const start = doc.indexOf("\n### 5.2 Kinds");
  const end = doc.indexOf("\n### 5.3", start);
  assert.ok(start >= 0 && end > start, "§5.2 is in docs/contract.md");
  const kinds: string[] = [];
  for (const line of doc.slice(start, end).split("\n")) {
    if (!line.startsWith("|")) continue;
    const first = line.split("|")[1]!.trim();
    if (first === "Kind" || /^-+$/.test(first)) continue;
    const names = [...first.matchAll(/`([a-z-]+)`/g)].map((m) => m[1]!);
    assert.ok(names.length > 0, `a kind in row: ${line.slice(0, 60)}`);
    kinds.push(...names);
  }
  return kinds;
}

test("KINDS is exactly the kinds of contract §5.2", () => {
  const contract = contractKinds();
  assert.equal(new Set(contract).size, contract.length, "§5.2 lists no kind twice");
  assert.deepEqual([...KINDS].sort(), [...contract].sort());
});

test("every key of LEAD_MESSAGES is a known kind", () => {
  for (const k of Object.keys(LEAD_MESSAGES)) assert.ok((KINDS as readonly string[]).includes(k), k);
});
