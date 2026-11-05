import { describe, expect, it, vi } from "vitest";
import { installCrashSignals } from "./crash-signals.ts";
import type { CrashReport } from "./health.ts";

type Listener = (event: unknown) => void;
function fakeTarget() {
  const listeners = new Map<string, Set<Listener>>();
  return {
    listeners,
    addEventListener(type: string, l: Listener) { (listeners.get(type) ?? listeners.set(type, new Set()).get(type)!).add(l); },
    removeEventListener(type: string, l: Listener) { listeners.get(type)?.delete(l); },
    emit(type: string, event: unknown) { for (const l of [...(listeners.get(type) ?? [])]) l(event); },
    count() { let n = 0; for (const s of listeners.values()) n += s.size; return n; },
  };
}

describe("installCrashSignals", () => {
  it("installCrashSignals reports window error events", () => {
    const target = fakeTarget();
    const onCrash = vi.fn<(r: CrashReport) => void>();
    installCrashSignals(target, onCrash);
    const error = new Error("boom");
    target.emit("error", { message: "boom", error });
    expect(onCrash).toHaveBeenCalledTimes(1);
    const report = onCrash.mock.calls[0]![0];
    expect(report.source).toBe("window-error");
    expect(report.error).toContain("boom");
  });

  it("installCrashSignals reports unhandledrejection with the reason message", () => {
    const target = fakeTarget();
    const onCrash = vi.fn<(r: CrashReport) => void>();
    installCrashSignals(target, onCrash);
    target.emit("unhandledrejection", { reason: new Error("nope") });
    target.emit("unhandledrejection", { reason: "plain" });
    expect(onCrash).toHaveBeenCalledTimes(2);
    expect(onCrash.mock.calls[0]![0]).toMatchObject({ source: "unhandledrejection", error: "nope" });
    expect(onCrash.mock.calls[1]![0]).toMatchObject({ source: "unhandledrejection", error: "plain" });
  });

  it("installCrashSignals counts errors", () => {
    const target = fakeTarget();
    const signals = installCrashSignals(target, () => {});
    expect(signals.errors()).toBe(0);
    target.emit("error", { message: "a", error: new Error("a") });
    target.emit("unhandledrejection", { reason: new Error("b") });
    target.emit("error", { message: "c" });
    expect(signals.errors()).toBe(3);
  });

  it("installCrashSignals dispose removes the listeners", () => {
    const target = fakeTarget();
    const onCrash = vi.fn<(r: CrashReport) => void>();
    const signals = installCrashSignals(target, onCrash);
    expect(target.count()).toBeGreaterThan(0);
    signals.dispose();
    expect(target.count()).toBe(0);
    target.emit("error", { message: "late" });
    expect(onCrash).not.toHaveBeenCalled();
  });
});
