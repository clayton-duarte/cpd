import type { WireError } from "../protocol/domain.ts";

export function routeErrorTarget(_error: WireError): "cpd" | "host" {
  throw new Error("not implemented");
}
