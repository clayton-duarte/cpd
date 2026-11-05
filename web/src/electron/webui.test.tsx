// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { mountWebui } from "./webui.tsx";

const render = vi.hoisted(() => vi.fn());
vi.mock("react-dom/client", () => ({ createRoot: () => ({ render }) }));
vi.mock("../ui/Root.tsx", () => ({ Root: () => null }));

afterEach(() => { render.mockClear(); vi.restoreAllMocks(); });

function bridge() {
  return {
    sendWeb: vi.fn(),
    onWebMessage: vi.fn(() => () => {}),
    reportHealthy: vi.fn(),
    reportCrash: vi.fn(),
  };
}

type RootElement = { type: unknown; props: Record<string, unknown> };

it("mountWebui renders Root with the bridge health reporters", async () => {
  const b = bridge();
  mountWebui(document.createElement("div"), b);
  expect(render).toHaveBeenCalledTimes(1);
  const element = render.mock.calls[0]![0] as RootElement;
  const { Root } = await import("../ui/Root.tsx");
  expect(element.type).toBe(Root);
  expect(element.props.onCrash).toBe(b.reportCrash);
  expect(element.props.onHealthy).toBe(b.reportHealthy);
  expect(typeof element.props.errors).toBe("function");
});

it("mountWebui installs the crash signals on window", () => {
  const b = bridge();
  const add = vi.spyOn(window, "addEventListener");
  mountWebui(document.createElement("div"), b);
  const types = add.mock.calls.map((c) => c[0]);
  expect(types).toContain("error");
  expect(types).toContain("unhandledrejection");
  const element = render.mock.calls[0]![0] as RootElement;
  const errors = element.props.errors as () => number;
  expect(errors()).toBe(0);
  window.dispatchEvent(new ErrorEvent("error", { message: "late boom", error: new Error("late boom") }));
  expect(b.reportCrash).toHaveBeenCalledWith(expect.objectContaining({ source: "window-error" }));
  expect(errors()).toBe(1);
});
