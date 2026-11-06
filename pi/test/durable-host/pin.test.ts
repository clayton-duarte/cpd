import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { PI_DURABLE } from "../../durable-host/run.ts";

const pkg = JSON.parse(
  readFileSync(new URL("../../../package.json", import.meta.url), "utf8"),
) as {
  dependencies: Record<string, string>;
  devDependencies: Record<string, string>;
};

test("package.json pins the earendil-works packages at 1.5.0", () => {
  for (const name of ["chord", "pi-coding-agent", "pi-ai", "pi-codemode", "pi-durable"]) {
    assert.equal(pkg.dependencies[`@earendil-works/${name}`], "1.5.0", name);
  }
  assert.equal(pkg.dependencies["@earendil-works/pi-tui"], "^1.5.0");
});

test("the host's PI_DURABLE equals the pi-durable pin of package.json", () => {
  const pin = pkg.dependencies["@earendil-works/pi-durable"];
  assert.equal(pin, "1.5.0");
  assert.equal(PI_DURABLE, pin);
});
