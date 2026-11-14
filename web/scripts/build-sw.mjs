// Post-build: copy manifest and icons into dist/, then emit dist/sw.js with the precache manifest of dist/.
import { copyFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { build } from "vite";
import { injectManifest } from "workbox-build";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "dist");

copyFileSync(path.join(root, "scripts", "manifest.webmanifest"), path.join(dist, "manifest.webmanifest"));
for (const n of [180, 192, 512]) {
  copyFileSync(path.join(root, "build", "icon", "png", `icon_${n}.png`), path.join(dist, `icon_${n}.png`));
}

// workbox injects into JavaScript: bundle the TypeScript worker source first.
const work = mkdtempSync(path.join(tmpdir(), "cpd-sw-"));
await build({
  configFile: false,
  logLevel: "warn",
  build: {
    outDir: work,
    emptyOutDir: true,
    minify: true,
    lib: { entry: path.join(root, "src", "state", "sw.ts"), formats: ["iife"], name: "cpdSw", fileName: () => "sw-src.js" },
  },
});

const { count, warnings } = await injectManifest({
  swSrc: path.join(work, "sw-src.js"),
  swDest: path.join(dist, "sw.js"),
  globDirectory: dist,
  globPatterns: ["**/*.{html,js,css,png,svg,ico,woff,woff2,webmanifest,wasm}"],
  globIgnores: ["sw.js"],
  maximumFileSizeToCacheInBytes: 20 * 1024 * 1024,
});
for (const w of warnings) console.warn(w);
console.log(`sw.js: precached ${count} files`);
rmSync(work, { recursive: true, force: true });
