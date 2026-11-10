import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
import { cpdWebPlugin } from "./src/server/plugin.ts";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function buildIco(pngBuffers: Array<{ width: number; height: number; buffer: Buffer }>): Buffer {
  const count = pngBuffers.length;
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type 1 = ICO
  header.writeUInt16LE(count, 4); // count

  const dirEntrySize = 16;
  const dirSize = count * dirEntrySize;
  let currentOffset = 6 + dirSize;

  const entries: Buffer[] = [];
  for (const item of pngBuffers) {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(item.width >= 256 ? 0 : item.width, 0);
    entry.writeUInt8(item.height >= 256 ? 0 : item.height, 1);
    entry.writeUInt8(0, 2); // color count
    entry.writeUInt8(0, 3); // reserved
    entry.writeUInt16LE(1, 4); // planes
    entry.writeUInt16LE(32, 6); // bpp
    entry.writeUInt32LE(item.buffer.length, 8); // size
    entry.writeUInt32LE(currentOffset, 12); // offset
    entries.push(entry);
    currentOffset += item.buffer.length;
  }

  return Buffer.concat([header, ...entries, ...pngBuffers.map((p) => p.buffer)]);
}

let cachedFavicons: { svgBytes: Buffer; icoBytes: Buffer } | null = null;

function getFavicons(): { svgBytes: Buffer; icoBytes: Buffer } {
  if (cachedFavicons) return cachedFavicons;

  const svgPath = path.resolve(__dirname, "build/icon/cpd-icon.svg");
  if (!fs.existsSync(svgPath)) {
    throw new Error(`Favicon source not found at ${svgPath}`);
  }
  const svgBytes = fs.readFileSync(svgPath);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "cpd-favicon-"));
  try {
    const sizes = [16, 32, 48];
    const images: Array<{ width: number; height: number; buffer: Buffer }> = [];
    for (const s of sizes) {
      const outPng = path.join(tmpDir, `${s}.png`);
      execFileSync("sips", ["-s", "format", "png", "-z", String(s), String(s), svgPath, "--out", outPng], {
        stdio: "pipe",
      });
      const buf = fs.readFileSync(outPng);
      images.push({ width: s, height: s, buffer: buf });
    }
    const icoBytes = buildIco(images);
    cachedFavicons = { svgBytes, icoBytes };
    return cachedFavicons;
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

export function cpdFavicon(): Plugin {
  return {
    name: "cpd-favicon",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url === "/favicon.svg") {
          const { svgBytes } = getFavicons();
          res.setHeader("Content-Type", "image/svg+xml");
          res.end(svgBytes);
          return;
        }
        if (req.url === "/favicon.ico") {
          const { icoBytes } = getFavicons();
          res.setHeader("Content-Type", "image/x-icon");
          res.end(icoBytes);
          return;
        }
        next();
      });
    },
    generateBundle() {
      const { svgBytes, icoBytes } = getFavicons();
      this.emitFile({
        type: "asset",
        fileName: "favicon.svg",
        source: svgBytes,
      });
      this.emitFile({
        type: "asset",
        fileName: "favicon.ico",
        source: icoBytes,
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), cpdFavicon(), cpdWebPlugin()],
  server: { port: 5173, strictPort: true },
});
