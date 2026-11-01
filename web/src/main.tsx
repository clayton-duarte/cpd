// The web client entry point: mount the app shell into a DOM root, inside the router and
// against the typed mock (hs-d780). The harness tests the views themselves, not this entry
// (web/src/views/views.test.tsx).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { createMockApi } from "./api/mock";
import { App } from "./app/App";
import "./index.css";

/** The token every request carries (§4.2). The bootstrap replaces it when the server lands. */
const DEV_TOKEN = "dev-token";

/** Mounts the app shell into `root`. */
export function main(root: Element): void {
  const api = createMockApi();
  createRoot(root).render(
    <StrictMode>
      <BrowserRouter>
        <App api={api} token={DEV_TOKEN} />
      </BrowserRouter>
    </StrictMode>,
  );
}

const rootElement = document.getElementById("root");
if (rootElement) {
  main(rootElement);
}
