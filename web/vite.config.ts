import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The cpd dev-server plugin is added later by another Job.
export default defineConfig({
  plugins: [react()],
  server: { port: 5173, strictPort: true },
});
