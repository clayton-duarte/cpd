// Contract §6.18 "Reload": the host flow behind `ServerDeps.reload`. The wiring in run.ts builds the
// port from its own pending counter and reporter state; ServerDeps.reload is a thin adapter over it.
import { ProtocolError } from "./protocol.ts";

/** The host's own state at the trigger (contract §6.18). `editorText` is always `""`: the host is headless. */
export type ReloadGuard = { busy: boolean; pending: boolean; state: "idle" | "working" | "waiting" | "blocked" };

export type ReloadPort = {
  /** The rebuild: re-read the prompt sources and re-import the extension modules; the new extension, or a throw for `internal`. */
  rebuild(): unknown;
  /** The host's state: the pending counter, the queued messages and the wanted state of the lib's `Guard`. */
  guard(): ReloadGuard;
  /** pi's own load of the package: `{ ok: true }`, or the refusal reason of `validatePackage`. */
  validate(extension: unknown): { ok: true } | { ok: false; reason: string };
  /** pi-durable's in-place install replaces the extension with its name. */
  install(extension: unknown): Promise<void>;
};

/** A refusal as the wire answers it: code `refused`, the reason in the message and in `details`. */
const refused = (reason: string): ProtocolError => new ProtocolError("refused", `reload refused: ${reason}`, { reason });

/** Refuses in the lib's order (busy, pending, state), then validates; a refusal is a `ProtocolError('refused')`. */
export function createReload(port: ReloadPort): { reload(): Promise<{ reloaded: boolean }> } {
  return {
    async reload(): Promise<{ reloaded: boolean }> {
      const g = port.guard();
      if (g.busy) throw refused("busy");
      if (g.pending) throw refused("pending");
      if (g.state !== "idle") throw refused("state");
      const extension = port.rebuild();
      const validation = port.validate(extension);
      if (!validation.ok) throw refused(validation.reason);
      await port.install(extension);
      return { reloaded: true };
    },
  };
}
