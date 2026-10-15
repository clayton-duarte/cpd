import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { loadConfig } from "../../src/lib/config.ts";
import { sandboxEnv } from "../helpers/index.ts";

const validConfig = {
  version: 1,
  concurrency: 3,
  retryCap: 2,
  attemptTimeoutMinutes: 30,
  askTimeoutMinutes: 60,
  tickSeconds: 5,
  prPollSeconds: 120,
  herdrSession: null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3",
  defaultReviewTier: "S4",
  reviewMode: "fork",
  leadPiPackages: ["npm:@plannotator/pi-extension@0.28.17", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"],
};

function useConfig(content?: unknown): void {
  const home = sandboxEnv().CPD_HOME!;
  process.env.CPD_HOME = home;
  if (content !== undefined) {
    mkdirSync(home, { recursive: true });
    writeFileSync(join(home, "config.json"), typeof content === "string" ? content : JSON.stringify(content));
  }
}

test("a valid config loads", () => {
  useConfig(validConfig);
  assert.deepEqual(loadConfig(), validConfig);
});

test("reviewMode none is accepted (no Reviewer); other values are not", () => {
  useConfig({ ...validConfig, reviewMode: "none" });
  assert.equal(loadConfig().reviewMode, "none");
});

test("a string herdrSession is accepted", () => {
  useConfig({ ...validConfig, herdrSession: "cpd-test" });
  assert.equal(loadConfig().herdrSession, "cpd-test");
});

test("a missing file is config_missing (exit 1)", () => {
  useConfig();
  assert.throws(() => loadConfig(), (e: any) => e.code === "config_missing" && e.exitCode === 1);
});

test("an unknown key is config_invalid with zod issues in details", () => {
  useConfig({ ...validConfig, surprise: true });
  assert.throws(
    () => loadConfig(),
    (e: any) => e.code === "config_invalid" && e.exitCode === 1 && Array.isArray(e.details.issues) && e.details.issues.length > 0,
  );
});

test("an unknown key inside tiers is rejected too", () => {
  useConfig({ ...validConfig, tiers: { ...validConfig.tiers, X: "a/b" } });
  assert.throws(() => loadConfig(), (e: any) => e.code === "config_invalid");
});

test("wrong values are config_invalid", () => {
  for (const bad of [
    { ...validConfig, version: 2 },
    { ...validConfig, concurrency: 0 },
    { ...validConfig, leadTier: "O6" },
    { ...validConfig, reviewMode: "other" },
    { ...validConfig, tiers: { ...validConfig.tiers, O: "no-slash" } },
    { ...validConfig, herdrSession: "" },
  ]) {
    useConfig(bad);
    assert.throws(() => loadConfig(), (e: any) => e.code === "config_invalid");
  }
});

test("a missing key is config_invalid", () => {
  const { retryCap: _omit, ...rest } = validConfig;
  useConfig(rest);
  assert.throws(() => loadConfig(), (e: any) => e.code === "config_invalid");
});

test("invalid JSON is config_invalid", () => {
  useConfig("{ not json");
  assert.throws(() => loadConfig(), (e: any) => e.code === "config_invalid");
});
