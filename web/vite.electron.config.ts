import { builtinModules } from "node:module";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { electronOutDir } from "./src/electron/update/outdir";

// One config, three builds selected by CPD_ELECTRON_TARGET (see build:electron).
const target = process.env["CPD_ELECTRON_TARGET"] ?? "renderer";

const out = electronOutDir(process.env);

const nodeBuiltins = [...builtinModules, ...builtinModules.map((m) => `node:${m}`)];

export default defineConfig(
  target === "renderer"
    ? {
        base: "./",
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
                  input: "src/electron/main.ts",
                  external: ["electron", "node-pty", "@aurigax-ai/ghostty-web", ...nodeBuiltins],
                  output: { format: "es", entryFileNames: "main.js" },
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
