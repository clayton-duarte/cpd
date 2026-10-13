import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { locatePi } from "../../src/lib/pi-install.ts";
import { runCli, sandboxEnv } from "../helpers/index.ts";

const config = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5,
  prPollSeconds: 120, herdrSession: null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
};

/** Sandboxed HOME, but pointed at the real pi agent dir (read-only) so the catalog is the real one. */
function envWithConfig(c?: unknown) {
  const env = sandboxEnv({ PI_CODING_AGENT_DIR: locatePi().agentDir });
  if (c !== undefined) {
    mkdirSync(env.CPD_HOME!, { recursive: true });
    writeFileSync(join(env.CPD_HOME!, "config.json"), JSON.stringify(c));
  }
  return env;
}

test("cpd tiers prints the §7 catalog values", async () => {
  const r = await runCli(["tiers"], { env: envWithConfig(config) });
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.deepEqual(r.json, {
    ok: true,
    tiers: {
      O: { model: "github-copilot/claude-opus-5.5", efforts: [1, 2, 3, 4, 5] },
      S: { model: "github-copilot/claude-sonnet-5.5", efforts: [0, 1, 2, 3, 4, 5] },
      H: { model: "github-copilot/claude-haiku-4.5", efforts: [0, 1, 2, 3] },
    },
  });
});

test("cpd tiers without a config is config_missing (exit 1)", async () => {
  const r = await runCli(["tiers"], { env: envWithConfig() });
  assert.equal(r.code, 1);
  assert.equal(r.json.error.code, "config_missing");
});

test("cpd tiers with an unknown model is tier_model_unknown (exit 1)", async () => {
  const r = await runCli(["tiers"], { env: envWithConfig({ ...config, tiers: { ...config.tiers, O: "github-copilot/nope" } }) });
  assert.equal(r.code, 1);
  assert.equal(r.json.error.code, "tier_model_unknown");
});

test("cpd tiers rejects unknown flags (exit 2)", async () => {
  const r = await runCli(["tiers", "--nope"], { env: envWithConfig(config) });
  assert.equal(r.code, 2);
  assert.equal(r.json.error.code, "usage");
});
