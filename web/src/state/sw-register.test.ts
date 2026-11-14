import { describe, expect, it, vi } from "vitest";
import { registerServiceWorker } from "./sw-register.ts";

const browser = () => {
  const register = vi.fn(async () => ({}));
  return { register, env: { isElectron: false, isDev: false, isSecureContext: true, serviceWorker: { register } } };
};

describe("registerServiceWorker", () => {
  it("skips in Electron", async () => {
    const { register, env } = browser();
    expect(await registerServiceWorker({ ...env, isElectron: true })).toBe("skipped");
    expect(register).not.toHaveBeenCalled();
  });
  it("skips outside a secure context", async () => {
    const { register, env } = browser();
    expect(await registerServiceWorker({ ...env, isSecureContext: false })).toBe("skipped");
    expect(register).not.toHaveBeenCalled();
  });
  it("skips in dev", async () => {
    const { register, env } = browser();
    expect(await registerServiceWorker({ ...env, isDev: true })).toBe("skipped");
    expect(register).not.toHaveBeenCalled();
  });
  it("registers /sw.js at scope / in a secure browser", async () => {
    const { register, env } = browser();
    expect(await registerServiceWorker(env)).toBe("registered");
    expect(register).toHaveBeenCalledWith("/sw.js", { scope: "/" });
  });
});
