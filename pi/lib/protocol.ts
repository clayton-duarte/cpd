import { cpdProtocol, installedIdentity, toCpdError } from "./cpd.ts";
import { errorMessage } from "./delivery.ts";
import { PROTOCOL } from "./event.ts";
import type { Host } from "./host.ts";

/** commands.md §15.2, step 0. Never throws; the adapter keeps following whatever it finds. */
export async function checkProtocol(host: Host): Promise<void> {
  try {
    const cpd = await cpdProtocol(host.exec, { timeoutMs: host.timing.callTimeoutMs, signal: host.abort.signal });
    host.versionFailures.clear();
    if (cpd === PROTOCOL) return;
    const identity = installedIdentity() ?? "cpd";
    if (host.mismatched.has(identity)) return;
    host.mismatched.add(identity);
    const speaks = cpd === null ? "no protocol (`cpd --version` has no `protocol`)" : `protocol ${cpd}`;
    const m = errorMessage("protocol mismatch", "protocol_mismatch", `cpd speaks ${speaks}, this adapter ${PROTOCOL}; reload pi (/reload)`, { cpd, adapter: PROTOCOL });
    host.pi.sendMessage(m.message, m.options);
  } catch (e) {
    const err = toCpdError(e);
    if (host.versionFailures.has(err.code)) return;
    host.versionFailures.add(err.code);
    host.deliver("protocol check", err);
  }
}
