import { builtinModules } from "node:module";
import { defineConfig } from "vite";

// One config, three builds selected by CPD_ELECTRON_TARGET (see build:electron).
const target = process.env["CPD_ELECTRON_TARGET"] ?? "renderer";

const nodeBuiltins = [...builtinModules, ...builtinModules.map((m) => `node:${m}`)];

export default defineConfig(
  target === "renderer"
    ? {
        base: "./",
        build: {
          outDir: "dist-electron/renderer",
          emptyOutDir: true,
          rollupOptions: { input: "electron.html" },
        },
      }
    : {
        build: {
          outDir: "dist-electron",
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
