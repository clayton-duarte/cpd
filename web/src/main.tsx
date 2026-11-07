import { createRoot } from "react-dom/client";
import { browserFileReader, browserScheduler } from "./ui/browser-boundaries.ts";
import { browserWebSocket, createWebSocketTransport } from "./protocol/ws-transport.ts";
import { Root } from "./ui/Root.tsx";
import "./ui/app.css";

const root = document.getElementById("root");
if (root === null) throw new Error("missing #root");
// The browser build connects same-origin to the endpoint the Vite dev server hosts (15.1).
const scheme = location.protocol === "https:" ? "wss:" : "ws:";
const url = `${scheme}//${location.host}/cpd-ws`;
const transport = createWebSocketTransport({ url, createSocket: browserWebSocket, scheduler: browserScheduler });
// No StrictMode: its simulated unmount would close the one-shot Connection.
createRoot(root).render(<Root transport={transport} scheduler={browserScheduler} reader={browserFileReader} />);
