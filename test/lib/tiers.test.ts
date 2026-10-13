import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  attemptTier, formatTier, levelForEffort, loadCatalog, nextTier, parseTier, piModelArg, validTier, type Catalog,
} from "../../src/lib/tiers.ts";
import { loadConfig } from "../../src/lib/config.ts";
import { sandboxEnv } from "../helpers/index.ts";

/** Same shape the real catalog has today (trial t6). */
const fake: Catalog = {
  O: { model: "github-copilot/claude-opus-5.5", efforts: [1, 2, 3, 4, 5] },
  S: { model: "github-copilot/claude-sonnet-5.5", efforts: [0, 1, 2, 3, 4, 5] },
  H: { model: "github-copilot/claude-haiku-4.5", efforts: [0, 1, 2, 3] },
};

const next = (t: string) => {
  const n = nextTier(fake, t);
  return n ? formatTier(n) : "none";
};

test("parseTier / formatTier round-trip; bad syntax is a usage error", () => {
  assert.deepEqual(parseTier("S1"), { letter: "S", effort: 1 });
  assert.equal(formatTier(parseTier("O5")), "O5");
  assert.equal(formatTier("H0"), "H0");
  for (const bad of ["", "S", "S6", "X1", "s1", "S10", "S-1"]) {
    assert.throws(() => parseTier(bad), (e: any) => e.code === "invalid_tier" && e.exitCode === 2);
  }
});

test("levelForEffort maps 0-5, never minimal", () => {
  assert.deepEqual([0, 1, 2, 3, 4, 5].map(levelForEffort), ["off", "low", "medium", "high", "xhigh", "max"]);
  assert.throws(() => levelForEffort(6), (e: any) => e.code === "invalid_effort");
});

test("ladder table (§7)", () => {
  const table: [string, string][] = [
    ["H1", "H2"], ["H2", "H3"], ["H3", "S3"], ["S0", "S1"], ["S4", "S5"], ["S5", "O5"], ["O1", "O2"], ["O4", "O5"], ["O5", "none"],
  ];
  for (const [from, to] of table) assert.equal(next(from), to, `${from} -> ${to}`);
});

test("ladder: more steps", () => {
  assert.equal(next("H0"), "H1");
  assert.equal(next("S3"), "S4");
  assert.equal(next("O3"), "O4");
});

test("ladder skips to the lowest valid effort >= current on the next model", () => {
  const sparse: Catalog = {
    H: { model: "p/h", efforts: [0, 1] },
    S: { model: "p/s", efforts: [2, 4] },
    O: { model: "p/o", efforts: [3, 5] },
  };
  assert.equal(formatTier(nextTier(sparse, "H1")!), "S2");
  assert.equal(formatTier(nextTier(sparse, "S2")!), "O3"); // S3 is invalid; O's lowest valid >= 2 is 3
  assert.equal(formatTier(nextTier(sparse, "S4")!), "O5");
  assert.equal(nextTier(sparse, "O3"), null); // O4 invalid and no model above O
  assert.equal(nextTier(sparse, "O5"), null);
});

test("attemptTier: S1 -> S1,S2,S3,S4; H3 attempt 2 is S3", () => {
  assert.deepEqual([1, 2, 3, 4].map((n) => formatTier(attemptTier(fake, "S1", n)!)), ["S1", "S2", "S3", "S4"]);
  assert.equal(formatTier(attemptTier(fake, "H3", 2)!), "S3");
  assert.equal(formatTier(attemptTier(fake, parseTier("S1"), 3)!), "S3");
});

test("attemptTier is null once the ladder runs out", () => {
  assert.equal(formatTier(attemptTier(fake, "O4", 2)!), "O5");
  assert.equal(attemptTier(fake, "O4", 3), null);
  assert.throws(() => attemptTier(fake, "S1", 0), (e: any) => e.code === "invalid_attempt_number");
});

test("validTier and piModelArg against a catalog", () => {
  assert.equal(validTier(fake, "O0"), false);
  assert.equal(validTier(fake, "H4"), false);
  assert.equal(validTier(fake, "S0"), true);
  assert.equal(piModelArg(fake, "S3"), "github-copilot/claude-sonnet-5.5:high");
  assert.equal(piModelArg(fake, "S0"), "github-copilot/claude-sonnet-5.5:off");
  assert.equal(piModelArg(fake, "O5"), "github-copilot/claude-opus-5.5:max");
  assert.throws(() => piModelArg(fake, "O0"), (e: any) => e.code === "tier_invalid");
  assert.throws(() => nextTier(fake, "H4"), (e: any) => e.code === "tier_invalid");
});

// --- integration: the real installed pi ---

const config = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5,
  prPollSeconds: 120, herdrSession: null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
};

function useConfig(c: unknown) {
  const home = sandboxEnv().CPD_HOME!;
  process.env.CPD_HOME = home;
  mkdirSync(home, { recursive: true });
  writeFileSync(join(home, "config.json"), JSON.stringify(c));
  return loadConfig();
}

test("real pi catalog: validation and model argument", async () => {
  const catalog = await loadCatalog(useConfig(config));
  assert.deepEqual(catalog, fake);
  assert.equal(validTier(catalog, "O0"), false);
  assert.equal(validTier(catalog, "H4"), false);
  assert.equal(validTier(catalog, "S0"), true);
  assert.equal(piModelArg(catalog, "S3"), "github-copilot/claude-sonnet-5.5:high");
});

test("real pi catalog: an unknown model is tier_model_unknown naming it", async () => {
  const c = useConfig({ ...config, tiers: { ...config.tiers, H: "github-copilot/no-such-model" } });
  await assert.rejects(
    loadCatalog(c),
    (e: any) => e.code === "tier_model_unknown" && e.details.missing[0].letter === "H" && e.message.includes("no-such-model"),
  );
});

// --- locating pi (§7) and loading from that release, with a stub release ---

import { chmodSync, realpathSync, symlinkSync } from "node:fs";
import { locatePi } from "../../src/lib/pi-install.ts";
import { tmpDir } from "../helpers/index.ts";

function stubInstall(version = "9.9.9") {
  const root = tmpDir("cpd-pi-");
  const agent = join(realpathSync(root), "agent"); // macOS tmp is a symlink; locatePi returns realpaths
  mkdirSync(join(agent, "bin"), { recursive: true });
  writeFileSync(join(agent, "bin", "pi"), "#!/bin/sh\n");
  chmodSync(join(agent, "bin", "pi"), 0o755);
  mkdirSync(join(agent, "install"), { recursive: true });
  writeFileSync(join(agent, "install", "current-version"), version + "\n");
  const scope = join(agent, "install", "releases", version, "node_modules", "@earendil-works");
  mkdirSync(join(scope, "pi-coding-agent", "dist"), { recursive: true });
  mkdirSync(join(scope, "pi-ai", "dist"), { recursive: true });
  writeFileSync(
    join(scope, "pi-coding-agent", "dist", "index.js"),
    `export const ModelRuntime = { create: async (o) => ({ getModel: (p, id) => o.allowModelNetwork === false && p === "prov" ? { id, levels: id === "low-only" ? ["off", "minimal", "low"] : ["low", "medium", "max"] } : undefined }) };`,
  );
  writeFileSync(join(scope, "pi-ai", "dist", "index.js"), `export const getSupportedThinkingLevels = (m) => m.levels;`);
  // a symlink to the launcher in another dir, as a user's PATH would have
  const linkDir = join(root, "link-bin");
  mkdirSync(linkDir);
  symlinkSync(join(agent, "bin", "pi"), join(linkDir, "pi"));
  return { agent, linkDir, scope };
}

test("locatePi follows the realpath of pi on PATH to the release in current-version", () => {
  const s = stubInstall();
  const pi = locatePi({ PATH: s.linkDir });
  assert.equal(pi.agentDir, s.agent);
  assert.equal(pi.version, "9.9.9");
  assert.equal(pi.codingAgentEntry, join(s.scope, "pi-coding-agent", "dist", "index.js"));
  assert.equal(pi.piAiEntry, join(s.scope, "pi-ai", "dist", "index.js"));
});

test("locatePi fails loud with pi_not_found at each step", () => {
  const notFound = (e: any) => e.code === "pi_not_found";
  assert.throws(() => locatePi({ PATH: tmpDir() }), notFound);
  const noVersion = stubInstall();
  writeFileSync(join(noVersion.agent, "install", "current-version"), "../evil\n");
  assert.throws(() => locatePi({ PATH: noVersion.linkDir }), notFound);
  const noRelease = stubInstall();
  writeFileSync(join(noRelease.agent, "install", "current-version"), "1.0.0\n");
  assert.throws(() => locatePi({ PATH: noRelease.linkDir }), notFound);
  const noFile = stubInstall();
  writeFileSync(join(noFile.agent, "install", "current-version"), "");
  assert.throws(() => locatePi({ PATH: noFile.linkDir }), notFound);
});

test("loadCatalog loads ModelRuntime and getSupportedThinkingLevels from the located release", async () => {
  const s = stubInstall("7.7.7");
  const savedPath = process.env.PATH;
  process.env.PATH = s.linkDir;
  try {
    const cfg = useConfig({ ...config, tiers: { O: "prov/big", S: "prov/mid", H: "prov/low-only" } });
    const catalog = await loadCatalog(cfg);
    assert.deepEqual(catalog, {
      O: { model: "prov/big", efforts: [1, 2, 5] },
      S: { model: "prov/mid", efforts: [1, 2, 5] },
      H: { model: "prov/low-only", efforts: [0, 1] }, // minimal has no effort digit
    });
  } finally {
    process.env.PATH = savedPath;
  }
});
