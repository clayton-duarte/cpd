import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { cpdWebPlugin } from "./src/server/plugin.ts";

export default defineConfig({
  plugins: [react(), cpdWebPlugin()],
  server: { port: 5173, strictPort: true },
});
