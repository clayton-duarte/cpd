import { describe, expect, it, vi } from "vitest";

// Exercise the authored Playwright scenario, but replace Electron and the fixture's I/O.
const { scenario, page } = vi.hoisted(() => {
  const scenario: { prod?: (fixtures: unknown) => Promise<void> } = {};
  const page = {
    waitForLoadState: vi.fn(async () => {}),
    reload: vi.fn(async () => {}),
    locator: vi.fn((selector: string) => ({ selector, count: async () => 0 })),
  };
  return { scenario, page };
});

vi.mock("./e2e-fixtures", () => {
  const test = Object.assign((name: string, fn: (fixtures: unknown) => Promise<void>) => {
    if (name.includes("prod view")) scenario.prod = fn;
  }, { beforeAll: vi.fn(), beforeEach: vi.fn(), afterAll: vi.fn() });
  const expectPage = (target: { selector?: string } | boolean | number | string) => ({
    toBe: (value: unknown) => expect(target).toBe(value),
    toContain: (value: string) => expect(target).toContain(value),
    toBeVisible: async () => {
      // The updater finished before first load: only the built renderer is visible.
      expect((target as { selector: string }).selector).toBe("#built-view");
    },
    toHaveText: async (value: string) => {
      expect((target as { selector: string }).selector).toBe("#built-view");
      expect(value).toBe("prod-view-from-checkout");
    },
  });
  expectPage.poll = () => ({ toBe: (value: unknown) => expect(value).toBe(true) });
  return {
    test, expect: expectPage, firstWindow: async () => page,
    launchApp: async () => ({
      evaluate: async (fn: (electron: unknown) => unknown) => fn({
        BrowserWindow: { getAllWindows: () => [{ isVisible: () => false }] },
        app: { getPath: () => "cpd-e2e-userdata-test" },
      }),
      windows: () => [page], close: async () => {},
    }),
    startRendererServer: async () => ({ url: "http://example.invalid", close: async () => {} }),
    stageFakeBin: vi.fn(), webRoot: "/mock",
  };
});

describe("prod view scenario", () => {
  it("accepts a built renderer already loaded before the first assertion", async () => {
    await import("./e2e-views.spec");
    expect(scenario.prod).toBeDefined();
    await scenario.prod!({ cpdHome: "/mock/home", userDataDir: "/mock/userdata" });
    expect(page.reload).toHaveBeenCalledOnce();
    expect(page.locator).toHaveBeenCalledWith("#built-view");
  });
});
