export interface SwRequest {
  method: string;
  url: string;
  mode: string;
  origin: string;
}
export type SwRoute = "precache" | "navigation-fallback" | "network";

// Stub: implemented by a later Job.
export function routeRequest(_req: SwRequest): SwRoute {
  return "precache";
}
