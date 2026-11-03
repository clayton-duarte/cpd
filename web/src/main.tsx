import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

function App() {
  return <p>cpd web</p>;
}

const root = document.getElementById("root");
if (root === null) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
