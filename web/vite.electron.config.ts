import { builtinModules } from "node:module";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { electronOutDir } from "./src/electron/update/outdir";

// One config, three builds selected by CPD_ELECTRON_TARGET (see build:electron).
const target = process.env["CPD_ELECTRON_TARGET"] ?? "renderer";

const out = electronOutDir(process.env);

const nodeBuiltins = [...builtinModules, ...builtinModules.map((m) => `node:${m}`)];

// The main build emits two entries: main.js (Electron) and server.js (re-imported on SIGUSR1).
const entryFiles = {
  main: { entryFileNames: "main.js" },
  server: { entryFileNames: "server.js" },
};

export default defineConfig(
  target === "renderer"
    ? {
        base: "./",
        // Renderer build id: the sha buildBundle stamps; "dev" outside an update build.
        define: { __CPD_WEB_SHA__: JSON.stringify(process.env["CPD_WEB_SHA"] ?? "dev") },
        plugins: [react(), tailwindcss()],
        build: {
          outDir: `${out}/renderer`,
          emptyOutDir: true,
          rollupOptions: { input: "electron.html" },
        },
      }
    : {
        build: {
          outDir: out,
          emptyOutDir: false,
          ssr: true,
          target: "node22",
          minify: false,
          rollupOptions:
            target === "main"
              ? {
                  input: { main: "src/electron/main.ts", server: "src/server/plugin.ts" },
                  external: ["electron", "node-pty", "@aurigax-ai/ghostty-web", "ws", ...nodeBuiltins],
                  output: { format: "es", entryFileNames: (chunk) => entryFiles[chunk.name === "server" ? "server" : "main"].entryFileNames },
                }
              : {
                  input: "src/electron/preload.ts",
                  external: ["electron"],
                  output: { format: "cjs", entryFileNames: "preload.cjs" },
                },
        },
        ssr: { noExternal: true },
      },
);
