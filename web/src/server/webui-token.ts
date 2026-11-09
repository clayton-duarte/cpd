import { randomBytes } from "node:crypto";
import { chmodSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** contract §6.16 "The WebUI token": this boot's secret, and the removal of its file. */
export interface IssuedToken {
  readonly token: string;
  remove(): void;
}

/**
 * Creates a random secret of 32 bytes (hex) and writes it to `<cpdHome>/run/webui-token`, mode 0600, replacing the file of any
 * earlier start. The token stays with this process, the cpd CLI and the host: it is never sent to the browser.
 */
export function writeWebuiToken(cpdHome: string): IssuedToken {
  const run = join(cpdHome, "run");
  const file = join(run, "webui-token");
  const token = randomBytes(32).toString("hex");
  mkdirSync(run, { recursive: true });
  rmSync(file, { force: true });
  writeFileSync(file, token, { mode: 0o600 });
  chmodSync(file, 0o600);
  return { token, remove: () => rmSync(file, { force: true }) };
}
