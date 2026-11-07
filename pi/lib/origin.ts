import { homedir } from "node:os";
import { join } from "node:path";
import type { PiApi } from "./pi.ts";

/**
 * commands.md §15.3 "The user-message tag" (Plan ui-origin): the User's own text reaches an agent with one prefix,
 * the **surface and view tag** `[via <origin> · view: <view>] `. `<origin>` is the surface the message was sent from
 * (`TUI` typed interactively into pi, `WebUI` sent from the cpd desktop's WebUI Composer); `<view>` is the cpd desktop
 * view showing when it was sent, read from the marker file `$CPD_HOME/run/ui-view.json` of shape
 * `{"view": "tui" | "webui" | "webui-preview", "pid": <pid>}`. A missing file, malformed JSON or a `pid` that is not a
 * live process means `unknown`. An image with no text is a valid message and is delivered (air-d43).
 */

/** The surface a User message was sent from. */
export type Origin = "tui" | "webui";

/** The view a User message was sent from; `unknown` when the marker is missing, malformed or its pid is dead. */
export type View = "TUI" | "WebUI" | "WebUI Preview" | "unknown";

/** `$CPD_HOME`, as the adapter reads it (the same default as `explore.ts`, `live-stream.ts`, `startup.ts`). */
export const cpdHome = (): string => process.env.CPD_HOME || join(homedir(), ".cpd");

/** Reads the view marker `$CPD_HOME/run/ui-view.json`; `unknown` for a missing file, malformed JSON or a dead pid. */
export function readView(_home: string = cpdHome()): View {
  return "unknown";
}

/** The surface and view tag `[via <origin> · view: <view>] `, prefixed to the User's text. */
export function tag(_origin: Origin, _view: View): string {
  return "";
}

/**
 * Registers the interactive-input tag (commands.md §15.3): a prompt typed interactively into pi is prefixed with the
 * tag; input that starts with `/` is passed through untagged (rewriting command text breaks pi's skill and template
 * expansion), and non-interactive input is left alone.
 */
export function registerOriginTag(pi: PiApi): void {
  pi.on("input", () => ({ action: "continue" }));
}
