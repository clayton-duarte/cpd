import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { CustomEditor } from "@earendil-works/pi-coding-agent";
import type { ExtensionContext } from "@earendil-works/pi-coding-agent";

/** What the install needs of pi's context: the editor slot. */
export type QuitCtx = { ui: Pick<ExtensionContext["ui"], "setEditorComponent"> };

type SubmitEditor = { onSubmit?: (text: string) => void };

/** True for `/quit` and `/q` (trimmed). */
export const isQuitCommand = (text: string): boolean => ["/quit", "/q"].includes(text.trim());

/**
 * Installs the editor wrapper that writes the quit marker at `CPD_QUIT_MARKER` when the User submits exactly `/quit` or `/q`
 * (trimmed), then lets the original submit run (pro-d361). pi's session_shutdown reason is always `quit`, so only the
 * editor can tell `/quit` from a signal. pi assigns `onSubmit` after `setEditorComponent`, so the assignment is intercepted.
 */
export function installQuitMarker(ctx: QuitCtx, host: { env: Record<string, string | undefined> }): void {
  const marker = host.env.CPD_QUIT_MARKER;
  if (!marker) return;
  ctx.ui.setEditorComponent((tui, theme, keybindings) => {
    const editor = new CustomEditor(tui, theme, keybindings, { embedWorkingStatus: true }) as unknown as SubmitEditor;
    let original: ((text: string) => void) | undefined;
    Object.defineProperty(editor, "onSubmit", {
      configurable: true,
      get: () => original && ((text: string) => {
        if (isQuitCommand(text)) {
          mkdirSync(dirname(marker), { recursive: true });
          writeFileSync(marker, "");
        }
        original?.(text);
      }),
      set: (fn) => { original = fn; },
    });
    return editor as never;
  });
}
