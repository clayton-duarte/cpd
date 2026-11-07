import { readFileSync } from "node:fs";
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

/** The `hello` client name of the cpd desktop's WebUI (`protocol.ts`, the `client` field), the surface that is tagged WebUI on the wire. */
export const WEB_CLIENT = "cpd-web";

/** `$CPD_HOME`, as the adapter reads it (the same default as `explore.ts`, `live-stream.ts`, `startup.ts`). */
export const cpdHome = (): string => process.env.CPD_HOME || join(homedir(), ".cpd");

/** The marker's `view` values, as the tag shows them. */
const VIEWS: Record<string, View> = { tui: "TUI", webui: "WebUI", "webui-preview": "WebUI Preview" };

/** Whether `pid` is a live process: `EPERM` still means it exists (another user's), only a missing one is dead. */
function live(pid: unknown): boolean {
  if (typeof pid !== "number" || !Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === "EPERM";
  }
}

/** Reads the view marker `$CPD_HOME/run/ui-view.json`; `unknown` for a missing file, malformed JSON or a dead pid. */
export function readView(home: string = cpdHome()): View {
  let marker: unknown;
  try {
    marker = JSON.parse(readFileSync(join(home, "run", "ui-view.json"), "utf8"));
  } catch {
    return "unknown"; // a missing file or malformed JSON: no marker, so no view
  }
  if (typeof marker !== "object" || marker === null) return "unknown";
  const { view, pid } = marker as { view?: unknown; pid?: unknown };
  if (typeof view !== "string" || !live(pid)) return "unknown";
  const shown = Object.hasOwn(VIEWS, view) ? VIEWS[view] : undefined;
  return shown ?? "unknown";
}

/** The surface and view tag `[via <origin> · view: <view>] `, prefixed to the User's text. */
export function tag(origin: Origin, view: View): string {
  return `[via ${origin === "tui" ? "TUI" : "WebUI"} · view: ${view}] `;
}

/**
 * Registers the interactive-input tag (commands.md §15.3): a prompt typed interactively into pi is prefixed with the
 * tag; input that starts with `/` is passed through untagged (rewriting command text breaks pi's skill and template
 * expansion), and non-interactive input is left alone. An image with no text is tagged too, and keeps its images.
 */
export function registerOriginTag(pi: PiApi): void {
  pi.on("input", (event) => {
    if (event.source !== "interactive") return { action: "continue" };
    if (event.text.startsWith("/")) return { action: "continue" };
    const text = tag("tui", readView()) + event.text;
    // The transform replaces the whole input, images included: an image-only message must still deliver its images.
    return event.images === undefined ? { action: "transform", text } : { action: "transform", text, images: event.images };
  });
}
