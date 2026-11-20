import { WebError } from "../../protocol/domain.ts";
import { createEventsDialect } from "./frames/events-dialect.ts";
import type { Dialect } from "./frames/types.ts";
import { protocolError } from "./frames/validation.ts";
import { createViewDialect } from "./frames/view-dialect.ts";

export { dialogFromRequest } from "./frames/ui-request.ts";
export type { BaseFrame, Dialect, DialectOutput, DurableFacts, DurableTranscript, Gap, IncrementFrame } from "./frames/types.ts";

// The dialect seam (CONTRACT 10.8): durable-host protocol 1 (pi-durable 1.3.0 `snapshot` / `events`) and
// protocol 2 (pi-durable 1.5.0 `view` / `ops`) -> domain transcript.

export const SUPPORTED_HOSTS: readonly { protocol: 1 | 2; piDurable: string }[] = [
  { protocol: 1, piDurable: "1.3.0" },
  { protocol: 2, piDurable: "1.5.0" },
];
export function assertSupportedHost(welcome: { pi_durable: string; protocol: number }): void {
  if (!SUPPORTED_HOSTS.some((h) => h.protocol === welcome.protocol && h.piDurable === welcome.pi_durable)) {
    throw new WebError(
      "unsupported_host_version",
      `durable host pi-durable ${welcome.pi_durable} protocol ${welcome.protocol}; only ${SUPPORTED_HOSTS.map((h) => `pi-durable ${h.piDurable} protocol ${h.protocol}`).join(" and ")} are supported`,
      { piDurable: welcome.pi_durable, protocol: welcome.protocol },
    );
  }
}

/** Frames of the other protocol (and the frames of a bare test) carry no epoch or seq: fill the stream cursor in. */
function createViewDialectFor(protocol: 1 | 2): Dialect {
  const inner = createViewDialect();
  let epoch = "";
  let seq = 0;
  return {
    get transcript() {
      return inner.transcript;
    },
    get gap() {
      return inner.gap;
    },
    applyBase(frame) {
      if (!("view" in frame)) return protocolError(`expected a view frame on protocol ${protocol}`);
      const next = { epoch: frame.epoch ?? "v1", seq: frame.seq ?? 1 };
      const out = inner.applyBase({ view: frame.view, ...next });
      ({ epoch, seq } = next);
      return out;
    },
    applyIncrement(frame) {
      if (!("ops" in frame)) return protocolError(`expected an ops frame on protocol ${protocol}`);
      const out = inner.applyIncrement({ ops: frame.ops, epoch: frame.epoch ?? epoch, seq: frame.seq ?? seq + 1 });
      seq = frame.seq ?? seq + 1;
      return out;
    },
    applyHistory: (frame) => inner.applyHistory(frame),
  };
}

export function createDialect(protocol: 1 | 2): Dialect {
  if (protocol === 2) return createViewDialectFor(2);
  const events = createEventsDialect();
  // Protocol 1 hosts only send snapshot/events; view-shaped frames reach the shared view fold (Phase 1 unit tests feed it these).
  const view = createViewDialectFor(1);
  let active: Dialect = events;
  return {
    get transcript() {
      return active.transcript;
    },
    get gap() {
      return active.gap;
    },
    applyBase(frame) {
      active = "view" in frame ? view : events;
      return active.applyBase(frame);
    },
    applyIncrement(frame) {
      active = "ops" in frame ? view : active;
      return active.applyIncrement(frame);
    },
    applyHistory: (frame) => active.applyHistory(frame),
  };
}
