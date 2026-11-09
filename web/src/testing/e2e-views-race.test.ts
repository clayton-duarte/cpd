import { beforeEach, describe, expect, it, vi } from "vitest";

// Run the authored scenarios with the updater already finished before the first load.
const mocks = vi.hoisted(() => {
  const scenarios: Record<string, (fixtures: unknown) => Promise<void>> = {};
  const page = {
    waitForLoadState: vi.fn(async () => {}),
    reload: vi.fn(async () => {}),
    locator: vi.fn((selector: string) => ({ selector, count: async () => 0 })),
  };
  return { scenarios, page, marker: { value: "" }, reads: [] as string[], closed: [] as string[] };
});

vi.mock("node:fs", () => ({
  readFileSync: (path: string) => {
    mocks.reads.push(path);
    return path.includes("dist-electron.preview") ? "preview-view-from-branch" : "prod-view-from-checkout";
  },
}));

vi.mock("./e2e-fixtures", () => {
  const test = Object.assign((name: string, fn: (fixtures: unknown) => Promise<void>) => {
    mocks.scenarios[name.includes("prod view") ? "prod" : "preview"] = fn;
  }, { beforeAll: vi.fn(), beforeEach: vi.fn(), afterAll: vi.fn() });
  const expectPage = (target: { selector?: string } | boolean | number | string) => ({
    toBe: (value: unknown) => expect(target).toBe(value),
    toContain: (value: string) => expect(target).toContain(value),
    toBeVisible: async () => {
      // No placeholder exists when the first load sees the already-built renderer.
      expect((target as { selector: string }).selector).toBe("#built-view");
    },
    toHaveText: async (value: string) => {
      expect((target as { selector: string }).selector).toBe("#built-view");
      expect(value).toBe(mocks.marker.value);
    },
  });
  expectPage.poll = (read: () => unknown) => ({ toBe: (value: unknown) => expect(read()).toBe(value) });
  return {
    test, expect: expectPage, firstWindow: async () => mocks.page,
    launchApp: async () => ({
      evaluate: async (fn: (electron: unknown) => unknown) => fn({
        BrowserWindow: { getAllWindows: () => [{ isVisible: () => false }] },
        app: { getPath: () => "cpd-e2e-userdata-test" },
      }),
      windows: () => [mocks.page], close: async () => { mocks.closed.push("app"); },
    }),
    startRendererServer: async () => ({ url: "http://example.invalid", close: async () => { mocks.closed.push("server"); } }),
    stageFakeBin: vi.fn(), webRoot: "/mock",
  };
});

describe("view scenarios", () => {
  beforeEach(() => {
    mocks.page.reload.mockClear();
    mocks.page.locator.mockClear();
    mocks.reads.length = 0;
    mocks.closed.length = 0;
  });

  it.each([
    ["prod", "prod-view-from-checkout"],
    ["preview", "preview-view-from-branch"],
  ])("accepts the built %s renderer on the first load", async (view, marker) => {
    mocks.marker.value = marker;
    await import("./e2e-views.spec");
    expect(mocks.scenarios[view]).toBeDefined();
    await mocks.scenarios[view]!({ cpdHome: "/mock/home", userDataDir: "/mock/userdata" });
    expect(mocks.page.reload).toHaveBeenCalledOnce();
    expect(mocks.page.locator).toHaveBeenCalledWith("#built-view");
    expect(mocks.reads.some((path) => path.includes(view === "prod" ? "dist-electron/" : "dist-electron.preview/"))).toBe(true);
    if (view === "preview") expect(mocks.reads.some((path) => path.includes("dist-electron/"))).toBe(true);
    expect(mocks.closed).toEqual(["app", "server"]);
  });
});
