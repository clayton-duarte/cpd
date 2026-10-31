// Wire-surface pinning tests (docs/research/web-client-api.md §1). RED: each calls a pin
// sample that still throws `not implemented`, so they fail now and pass once the impl lands.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { p16UiSamples, p18ResultSamples, v1HostSamples } from "./types";

test("the v1 host frames are typed", () => {
  const types = v1HostSamples().map((f) => f.type);
  expect(types).toContain("welcome");
  expect(types).toContain("attached");
  expect(types).toContain("snapshot");
  expect(types).toContain("events");
  expect(types).toContain("result");
  expect(types).toContain("error");
  expect(types).toContain("host_state");
  expect(types).toContain("pong");
  expect(types).toContain("bye");
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
  expect(pins.some((r) => "models" in r)).toBe(true);
  expect(pins.some((r) => "entries" in r)).toBe(true);
});
