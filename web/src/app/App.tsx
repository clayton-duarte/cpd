// The app shell (docs/research/web-client-api.md §6): routing (react-router, hs-d770),
// the live WebSocket connection and the token bootstrap. It also owns the held P1.6
// `ui_request`s and renders each one as an approval dialog answered via `ui_response`
// (§4.3, hs-d775; the four approval purposes of §1.2).
//
// STUB: throws `not implemented` until the views-impl Job fills it in. This is what makes
// the views harness RED (web/src/views/views.test.tsx).
import type { ReactElement } from "react";
import type { UiRequest } from "../api/types";

/** The shell renders the current route and any held approval request. */
export interface AppProps {
  /** A held `ui_request` to show as an approval dialog, if any (§1.2 row 23). */
  uiRequest?: UiRequest;
}

/** Renders the app shell with the held approval, if any (STUB). */
export function App(_props: AppProps): ReactElement {
  throw new Error("not implemented");
}

/** Renders one approval `ui_request` as a dialog listing its options (STUB). */
export function ApprovalDialog(_props: { request: UiRequest }): ReactElement {
  throw new Error("not implemented");
}
