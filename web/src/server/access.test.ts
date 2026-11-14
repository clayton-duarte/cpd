import { describe, expect, it, vi } from "vitest";
import { webAccess } from "./access.ts";

const token = "a".repeat(40);
const request = (credential?: string) => ({ headers: credential === undefined ? {} : { authorization: `Basic ${Buffer.from(credential).toString("base64")}` } });

describe("WebUI access policy", () => {
  it("defaults to unauthenticated loopback and rejects unconfigured LAN bindings", () => {
    expect(webAccess("127.0.0.1").accepts(request())).toBe(true);
    for (const host of ["0.0.0.0", "::", "192.168.1.2"]) {
      expect(() => webAccess(host)).toThrow("requires CPD_WEBUI_ACCESS_TOKEN");
      expect(() => webAccess(host, "short")).toThrow();
    }
  });
  it("requires the backend's own credential for authenticated LAN access", () => {
    const access = webAccess("0.0.0.0", token);
    expect(access.accepts(request())).toBe(false);
    expect(access.accepts(request(`cpd:${token}x`))).toBe(false);
    expect(access.accepts(request(`other:${token}`))).toBe(false);
    expect(access.accepts(request(`cpd:${token}`))).toBe(true);
    expect(access.accepts({ headers: { authorization: `Bearer ${token}` } })).toBe(false);
  });
});

it("does not let a foreign LAN page reuse cached browser credentials", () => {
  const access = webAccess("0.0.0.0", token);
  const authenticated = request(`cpd:${token}`).headers;
  expect(access.acceptsSocket({ headers: { ...authenticated, origin: "https://backend.local", host: "backend.local" } })).toBe(true);
  expect(access.acceptsSocket({ headers: { ...authenticated, origin: "http://attacker.local:3333", host: "backend.local" } })).toBe(false);
  expect(access.acceptsSocket({ headers: { ...authenticated, origin: "null", host: "backend.local" } })).toBe(false);
  // The local default also rejects a foreign browser page targeting loopback.
  expect(webAccess("127.0.0.1").acceptsSocket({ headers: { origin: "http://attacker.local:3333", host: "localhost:3333" } })).toBe(false);
});


it("allows an explicitly trusted UI origin for the three-backend client", () => {
  vi.stubEnv("CPD_WEBUI_ALLOWED_ORIGINS", "https://first.local,https://second.local,https://third.local");
  try {
    const access = webAccess("0.0.0.0", token);
    expect(access.acceptsSocket({ headers: { ...request(`cpd:${token}`).headers, origin: "https://first.local", host: "second.local" } })).toBe(true);
    vi.stubEnv("CPD_WEBUI_ALLOWED_ORIGINS", "https://first.local/path");
    expect(() => webAccess("0.0.0.0", token)).toThrow("exact HTTP(S) origins");
  } finally { vi.unstubAllEnvs(); }
});
