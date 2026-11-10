// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { createAppRouter } from "./router.tsx";

describe("router", () => {
  it("matches agent routes and renders corresponding view in middle column", () => {
    const router = createAppRouter();
    expect(router).toBeDefined();
    expect(() => router.buildLocation({ to: "/a/$context", params: { context: "cpd" } })).toThrow();
  });

  it("renders empty prompt on root route", () => {
    const router = createAppRouter();
    expect(router).toBeDefined();
    expect(true).toBe(false);
  });

  it("renders not-found card with recovery button on unknown route without crashing", () => {
    const router = createAppRouter();
    expect(router).toBeDefined();
    expect(true).toBe(false);
  });

  it("preserves mounted context tree and inbox across route transitions", () => {
    const router = createAppRouter();
    expect(router).toBeDefined();
    expect(true).toBe(false);
  });

  it("navigates route when selecting agent from tree inbox and manager view", () => {
    const router = createAppRouter();
    expect(router).toBeDefined();
    expect(true).toBe(false);
  });

  it("restores agent route from hash history and supports back forward navigation", () => {
    const router = createAppRouter();
    expect(router).toBeDefined();
    expect(true).toBe(false);
  });
});
