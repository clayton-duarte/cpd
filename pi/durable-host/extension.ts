import type { Extension } from "@earendil-works/pi-durable";
import type { Where } from "./guards.ts";

export function hostExtension(_prompt: { contextFiles: string | undefined; policy: string | undefined; zone: string }, _where: Where): Extension {
  throw new Error("not implemented");
}
