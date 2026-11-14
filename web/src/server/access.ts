import { createHash, timingSafeEqual } from "node:crypto";
import type { IncomingMessage, ServerResponse, Server } from "node:http";

interface AccessRequest {
  headers: { authorization?: string | undefined; origin?: string | string[] | undefined; host?: string | undefined };
}

/** The client credential is separate from the process-local durable-host credential. */
export function webAccess(host: string, token = process.env["CPD_WEBUI_ACCESS_TOKEN"]) {
  const loopback = host === "127.0.0.1" || host === "::1" || host === "localhost";
  if (!loopback && (!token || token.length < 32)) {
    throw new Error("LAN WebUI access requires CPD_WEBUI_ACCESS_TOKEN with at least 32 characters");
  }
  if (token !== undefined && token.length < 32) {
    throw new Error("CPD_WEBUI_ACCESS_TOKEN must have at least 32 characters");
  }
  const digest = (value: string) => createHash("sha256").update(value).digest();
  const expected = token === undefined ? null : digest(`cpd:${token}`);
  const accepts = (request: AccessRequest): boolean => {
    if (expected === null) return true;
    const header = request.headers.authorization;
    if (typeof header !== "string" || !header.startsWith("Basic ")) return false;
    return timingSafeEqual(expected, digest(Buffer.from(header.slice(6), "base64").toString("utf8")));
  };
  const allowed = (process.env["CPD_WEBUI_ALLOWED_ORIGINS"] ?? "").split(",").map((s) => s.trim()).filter(Boolean);
  for (const origin of allowed) {
    const parsed = new URL(origin);
    if (origin.includes("*") || !["http:", "https:"].includes(parsed.protocol) || parsed.origin !== origin) {
      throw new Error("CPD_WEBUI_ALLOWED_ORIGINS requires exact HTTP(S) origins without paths or wildcards");
    }
  }
  const acceptsSocket = (request: AccessRequest): boolean => {
    if (!accepts(request)) return false;
    const origin = request.headers.origin;
    // Non-browser clients still need the configured credential. Browsers must prove a trusted UI origin.
    if (origin === undefined) return true;
    if (typeof origin !== "string") return false;
    try {
      const parsed = new URL(origin);
      if (!["http:", "https:"].includes(parsed.protocol) || parsed.origin !== origin) return false;
      if (allowed.includes(origin)) return true;
      const host = request.headers.host;
      return host !== undefined && new URL(`${parsed.protocol}//${host}`).host === parsed.host;
    } catch { return false; }
  };
  return {
    accepts,
    acceptsSocket,
    http(request: IncomingMessage, response: ServerResponse): boolean {
      if (accepts(request)) return true;
      response.writeHead(401, {
        "WWW-Authenticate": 'Basic realm="CPD", charset="UTF-8"',
        "Cache-Control": "no-store",
        "Content-Type": "text/plain; charset=utf-8",
      });
      response.end("Authenticate as cpd with this backend's access token.\n");
      return false;
    },
    /** Also covers Vite's HMR upgrade, before any WebSocket listener runs. */
    guardUpgrades(server: Pick<Server, "prependListener">): void {
      if (expected === null) return;
      server.prependListener("upgrade", (request, socket) => {
        if (!acceptsSocket(request)) {
          socket.write("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n");
          socket.destroy();
        }
      });
    },
  };
}
