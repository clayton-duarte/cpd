import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { cpdWebPlugin } from "./src/server/plugin.ts";

export default defineConfig({
  plugins: [react(), tailwindcss(), cpdWebPlugin()],
  server: { host: "0.0.0.0", port: 33333, strictPort: true },
});
