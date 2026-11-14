export interface SwRequest {
  method: string;
  url: string;
  mode: string;
  origin: string;
}
export type SwRoute = "precache" | "navigation-fallback" | "network";

const NETWORK_PATHS = ["/cpd-ws", "/api/"];

export function routeRequest(req: SwRequest): SwRoute {
  if (req.method !== "GET") return "network";
  const url = new URL(req.url);
  if (url.origin !== req.origin) return "network";
  if (NETWORK_PATHS.some((p) => url.pathname === p || url.pathname.startsWith(p))) return "network";
  if (req.mode === "navigate") return "navigation-fallback";
  return "precache";
}
