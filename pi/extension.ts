import { adapter } from "./lib/adapter.ts";
import type { PiApi } from "./lib/pi.ts";

/** commands.md §15: the only pi-specific code. Thin entry; the protocol is in ./lib. */
export default function (pi: PiApi): void {
  adapter(pi);
}
