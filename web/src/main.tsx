// The web client entry point: mount the app shell into a DOM root, inside the router and
// against the typed mock (hs-d780). The harness tests the views themselves, not this entry
// (web/src/views/views.test.tsx). With the dev live adapter on (CPD_WEB_LIVE=1) the
// `/__cpd/health` probe answers ok and the live API is handed to `App` instead
// (web/README.md "The UI wiring").
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { createLiveApi, type LiveApi } from "./api/live";
import { createMockApi } from "./api/mock";
import { App } from "./app/App";
import "./index.css";

/** The token every request carries (§4.2). The bootstrap replaces it when the server lands. */
const DEV_TOKEN = "dev-token";

/**
 * Probes `GET /__cpd/health` with the token and builds the live API only when it answers
 * `ok`; any other answer or any failure leaves the mock as the default (web/README.md).
 */
async function probeLive(token: string): Promise<LiveApi | undefined> {
  try {
    const response = await fetch("/__cpd/health", { headers: { "x-cpd-token": token } });
    if (!response.ok) return undefined;
    const body = (await response.json()) as { ok?: boolean };
    return body.ok === true ? createLiveApi({ token }) : undefined;
  } catch {
    return undefined;
  }
}

/** Mounts the app shell into `root`. */
export async function main(root: Element): Promise<void> {
  const api = createMockApi();
  const live = await probeLive(DEV_TOKEN);
  createRoot(root).render(
    <StrictMode>
      <BrowserRouter>
        <App api={api} token={DEV_TOKEN} live={live} />
      </BrowserRouter>
    </StrictMode>,
  );
}

const rootElement = document.getElementById("root");
if (rootElement) {
  void main(rootElement);
}
