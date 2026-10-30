import { lstatSync, realpathSync, rmSync, symlinkSync } from "node:fs";
import { basename, dirname, join } from "node:path";

/** The grammar of `is_version` in core/src/install/adapter.rs. */
const VERSION = /^pi\.(legacy|[0-9][^]*?-[0-9][^]*)$/;

/** A pin: `<share>/pi.pin.<pid>`, a symlink to the bare name of the version directory the process loaded the adapter from (contract, the pin). Returns its path, or null when `adapterDir` is not a version directory. */
export function pinAdapter(adapterDir: string, pid: number = process.pid): string | null {
  const real = realpathSync(adapterDir);
  const name = basename(real);
  if (!VERSION.test(name)) return null;
  const pin = join(dirname(real), `pi.pin.${pid}`);
  if (lstatSync(pin, { throwIfNoEntry: false }) !== undefined) rmSync(pin);
  symlinkSync(name, pin);
  const given = join(dirname(adapterDir), `pi.pin.${pid}`); // the caller's spelling of the share, when it is the same directory
  return realpathSync(dirname(adapterDir)) === dirname(real) ? given : pin;
}
