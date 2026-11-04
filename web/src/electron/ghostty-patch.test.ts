// @vitest-environment node
import { existsSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const here = (p: string) => new URL(p, import.meta.url);
const pkgDir = here("../../node_modules/@aurigax-ai/ghostty-web/");

type Pt = [number, number];
interface Rec {
  pts: Pt[];
  fills: number;
  strokes: number;
  ctx: unknown;
}

function recorder(): Rec {
  const rec: Rec = { pts: [], fills: 0, strokes: 0, ctx: null };
  const sample = (cx: number, cy: number, rx: number, ry: number, rot: number, a0: number, a1: number) => {
    const n = 32;
    for (let i = 0; i <= n; i++) {
      const a = a0 + ((a1 - a0) * i) / n;
      const px = rx * Math.cos(a);
      const py = ry * Math.sin(a);
      rec.pts.push([cx + px * Math.cos(rot) - py * Math.sin(rot), cy + px * Math.sin(rot) + py * Math.cos(rot)]);
    }
  };
  const handlers: Record<string, (...a: number[]) => void> = {
    moveTo: (x, y) => rec.pts.push([x, y]),
    lineTo: (x, y) => rec.pts.push([x, y]),
    arc: (x, y, r, a0, a1) => sample(x, y, r, r, 0, a0, a1),
    ellipse: (x, y, rx, ry, rot, a0, a1) => sample(x, y, rx, ry, rot, a0, a1),
    rect: (x, y, w, h) => rec.pts.push([x, y], [x + w, y + h]),
    fill: () => void rec.fills++,
    stroke: () => void rec.strokes++,
  };
  rec.ctx = new Proxy(
    {},
    {
      get: (_t, k: string) => handlers[k] ?? (() => undefined),
      set: () => true,
    },
  );
  return rec;
}

interface BoxModule {
  drawBoxGlyph(ctx: unknown, cp: number, x: number, y: number, w: number, h: number): boolean;
}
async function loadBox(): Promise<BoxModule> {
  const spec: string = pathToFileURL(new URL("lib/box-glyphs.ts", pkgDir).pathname).href;
  return (await import(/* @vite-ignore */ spec)) as BoxModule;
}

const W = 10;
const H = 20;
async function draw(cp: number) {
  const { drawBoxGlyph } = await loadBox();
  const rec = recorder();
  const claimed = drawBoxGlyph(rec.ctx, cp, 0, 0, W, H);
  return { rec, claimed };
}
const has = (pts: Pt[], x: number, y: number) => pts.some(([px, py]) => Math.abs(px - x) < 1e-6 && Math.abs(py - y) < 1e-6);

test("ghostty-web patch is registered for the pinned version", () => {
  const pkg = JSON.parse(readFileSync(here("../../package.json"), "utf8"));
  const version: string = pkg.dependencies["@aurigax-ai/ghostty-web"];
  const yaml = readFileSync(here("../../../pnpm-workspace.yaml"), "utf8");
  const key = `@aurigax-ai/ghostty-web@${version}`;
  const m = yaml.match(new RegExp(`patchedDependencies:\\s*\\n(?:[ \\t]+.*\\n|\\s*\\n)*?[ \\t]+['"]?${key.replace(/[.@/]/g, "\\$&")}['"]?\\s*:\\s*['"]?([^'"\\n]+)['"]?`));
  expect(m, `patchedDependencies entry for ${key}`).not.toBeNull();
  const file = (m?.[1] ?? "").trim();
  expect(file.startsWith("patches/")).toBe(true);
  expect(existsSync(here(`../../../${file}`))).toBe(true);
});

test("ghostty-web atlas slot padding is a full cell", () => {
  const src = readFileSync(new URL("lib/webgl-renderer.ts", pkgDir), "utf8");
  expect(src).toMatch(/this\.padX\s*=\s*Math\.ceil\(this\.cellW\)\s*;/);
  expect(src).not.toMatch(/this\.padX\s*=\s*Math\.ceil\(this\.cellW\s*\/\s*2\)/);
});

test("powerline solid triangles fill the whole cell", async () => {
  const right = await draw(0xe0b0);
  expect(right.claimed).toBe(true);
  expect(right.rec.fills).toBeGreaterThan(0);
  for (const [x, y] of [[0, 0], [W, H / 2], [0, H]] as Pt[]) expect(has(right.rec.pts, x, y)).toBe(true);
  const left = await draw(0xe0b2);
  expect(left.claimed).toBe(true);
  expect(left.rec.fills).toBeGreaterThan(0);
  for (const [x, y] of [[W, 0], [0, H / 2], [W, H]] as Pt[]) expect(has(left.rec.pts, x, y)).toBe(true);
});

test("powerline half-ellipses fill the whole cell height", async () => {
  for (const cp of [0xe0b4, 0xe0b6]) {
    const { rec, claimed } = await draw(cp);
    expect(claimed).toBe(true);
    expect(rec.fills).toBeGreaterThan(0);
    const ys = rec.pts.map((p) => p[1]);
    expect(Math.min(...ys)).toBeCloseTo(0, 5);
    expect(Math.max(...ys)).toBeCloseTo(H, 5);
    for (const [x] of rec.pts) {
      expect(x).toBeGreaterThanOrEqual(-1e-6);
      expect(x).toBeLessThanOrEqual(W + 1e-6);
    }
  }
});

test("powerline outlines stay inside the cell", async () => {
  const half = Math.max(1, Math.round(Math.min(W, H) / 9)) / 2;
  for (const cp of [0xe0b1, 0xe0b3, 0xe0b5, 0xe0b7]) {
    const { rec, claimed } = await draw(cp);
    expect(claimed).toBe(true);
    expect(rec.strokes).toBeGreaterThan(0);
    expect(rec.pts.length).toBeGreaterThan(1);
    for (const [x, y] of rec.pts) {
      expect(x).toBeGreaterThanOrEqual(-half - 1e-6);
      expect(x).toBeLessThanOrEqual(W + half + 1e-6);
      expect(y).toBeGreaterThanOrEqual(-half - 1e-6);
      expect(y).toBeLessThanOrEqual(H + half + 1e-6);
    }
  }
});

test("powerline glyphs outside E0B0 to E0B7 are not drawn by geometry", async () => {
  expect((await draw(0xe0b8)).claimed).toBe(false);
  expect((await draw(0xe0a0)).claimed).toBe(false);
  expect((await draw(0x2500)).claimed).toBe(true);
  // the patch must exist: E0B0 claimed proves the geometry is wired in
  expect((await draw(0xe0b0)).claimed).toBe(true);
});
