#!/usr/bin/env node
// CPD entry point; portable implementation and policy live outside the product.
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { cli, createGuard } from "../tools/repo-qa/scripts/qa.mjs";
export { github } from "../tools/repo-qa/scripts/qa.mjs";

const configFile = new URL("../qa.config.json", import.meta.url);
const guard = createGuard(JSON.parse(readFileSync(configFile, "utf8")));
export const { REPOS, MARKER, batchMarker, day } = guard;
export const ZONE = guard.config.timezone;
const batch = () => {
  if (process.env.QA_BATCH !== undefined && process.env.CPD_QA_BATCH !== undefined && process.env.QA_BATCH !== process.env.CPD_QA_BATCH) {
    throw new Error("Conflicting QA_BATCH and CPD_QA_BATCH; use one authorized batch");
  }
  return process.env.QA_BATCH ?? process.env.CPD_QA_BATCH ?? null;
};
export const status = (api, now, value = batch(), run = null) => guard.status(api, now, value, run);
export const run = (args, api, read, value = batch(), descriptor = null) => guard.run(args, api, read, value, descriptor);
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { cli(process.argv.slice(2), { configFile, batch: batch() }); }
  catch (error) {
    process.stderr.write(`${JSON.stringify({ error: error.message, creation_allowed: false })}\n`);
    process.exitCode = 1;
  }
}
