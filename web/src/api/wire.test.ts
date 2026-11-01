// Wire-surface pinning tests (docs/research/web-client-api.md §1). RED: each calls a pin
// sample that still throws `not implemented`, so they fail now and pass once the impl lands.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { p16UiSamples, p18ResultSamples, v1HostSamples } from "./types";

test("the v1 host frames are typed", () => {
  const types = v1HostSamples().map((f) => f.type);
  // §1.1 pins exactly the nine v1 H→C frames — no more, no fewer, no duplicates.
  expect(types).toHaveLength(9);
  expect(new Set(types).size).toBe(9);
  for (const t of ["welcome", "attached", "snapshot", "events", "result", "error", "host_state", "pong", "bye"] as const) {
    expect(types).toContain(t);
  }
});

test("the P1.6 ui_request and ui_response frames are typed with a still-open marker", () => {
  const src = readFileSync(new URL("./types.ts", import.meta.url), "utf8");
  expect(src).toContain("@open(P1.6)");
  const pins = p16UiSamples();
  expect(pins.some((f) => f.type === "ui_request")).toBe(true);
  expect(pins.some((f) => f.type === "ui_response")).toBe(true);
  expect(pins.some((f) => f.type === "ui_resolved")).toBe(true);
});

test("the P1.8 result shapes are typed with a still-open marker", () => {
  const src = readFileSync(new URL("./types.ts", import.meta.url), "utf8");
  expect(src).toContain("@open(P1.8)");
  const pins = p18ResultSamples();
  // §1.2: list_models and history are open; configure and fork are open too.
  expect(pins.some((r) => "models" in r)).toBe(true);
  expect(pins.some((r) => "entries" in r)).toBe(true);
  expect(pins.some((r) => "agent" in r)).toBe(true);
  expect(pins.some((r) => "conversation_id" in r)).toBe(true);
});
