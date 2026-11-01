import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The web client dev/build config (docs/research/web-client-api.md §6, A2): React + the
// Tailwind v4 plugin, and the dev server on port 5173 against the typed mock.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    strictPort: true,
  },
});
