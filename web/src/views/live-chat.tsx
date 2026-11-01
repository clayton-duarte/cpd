// The live Chat view (web/README.md "The UI wiring", hs-d819): a session picker, the folded
// transcript and a send box, all through the injected LiveApi. The folded transcript enters the
// existing fold through a `snapshot` frame (fold rule 3), so the Chat rendering engine is
// unchanged.
//
// Stub: `LiveChat` throws `not implemented` until the ui Job fills it in — this is what makes
// the live-chat tests RED.
import type { ReactElement } from "react";
import type { LiveApi } from "../api/live";

/** The live Chat: a session picker, the folded transcript, a send box. */
export interface LiveChatProps {
  /** The live API; absent when the probe failed — then the mock's Chat renders instead. */
  api?: LiveApi;
  token?: string;
}

/** Renders the live Chat (STUB). */
export function LiveChat(props: LiveChatProps): ReactElement {
  void props;
  throw new Error("not implemented");
}
