import type { Extension } from "@earendil-works/pi-durable";

export function cpdExtension(_prompt: { contextFiles: string | undefined; policy: string | undefined; zone: string }, _where: { cwd: string; home: string }): Extension {
  throw new Error("not implemented");
}
