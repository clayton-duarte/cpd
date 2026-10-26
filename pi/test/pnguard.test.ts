import assert from "node:assert/strict";
import { test } from "node:test";
import { pnVerdict } from "../lib/pnguard.ts";
import { REGISTRY_KEY } from "../lib/pntrack.ts";

// commands.md §15 "Every plannotator session is tracked": G1 (no live cpd run), G2 (no reachable link), G3 (a registry cpd cannot read).

type Verdict = { block?: boolean; reason?: string } | undefined;

const SESSION = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const goodEnv = { CPD_SESSION: SESSION, PLANNOTATOR_REMOTE: "1", PLANNOTATOR_URL_HOST: "192.168.3.3", PLANNOTATOR_PORT: "19437" };
/** The method names of the registry of @plannotator/pi-extension 0.28.17 (plannotator-tool-host.ts:190, REGISTRY_METHODS), under the key of :182. */
const goodRegistry = () => Object.fromEntries(["add", "remove", "openFor", "openAll", "listText", "close"].map((m) => [m, () => undefined]));

const G1 = "φ: plannotator waits for the cpd adapter to start";
const G2 = (cause: string) => `φ: plannotator needs a link the User can reach; refused: ${cause}`;
const G3 = "cpd cannot track plannotator: its review registry changed; plannotator was upgraded and its review registry changed: the pin in pi-config's settings.json and cpd's parser move together";

type Over = { env?: Record<string, string | undefined>; live?: boolean; store?: Record<string, unknown> };
const call = (toolName: string, input: unknown, over: Over = {}, parentToolCallId?: string): Verdict =>
  pnVerdict({ env: over.env ?? goodEnv, live: over.live ?? true, store: over.store ?? { [REGISTRY_KEY]: goodRegistry() } }, { toolName, input, ...(parentToolCallId ? { parentToolCallId } : {}) });

const OPENERS: [string, unknown][] = [
  ["plannotator", { action: "annotate", target: "a.md" }],
  ["plannotator", { action: "review" }],
  ["plannotator", { action: "last" }],
  ["plannotator_submit_plan", { filePath: "PLAN.md" }],
];

for (const [tool, input] of OPENERS) {
  const name = `${tool} ${(input as { action?: string }).action ?? ""}`.trim();

  test(`${name}: passes when the adapter is live, the env is right and the registry is readable`, () => {
    assert.equal(call(tool, input), undefined);
  });

  test(`${name}: G1 refuses with no live cpd run`, () => {
    assert.deepEqual(call(tool, input, { live: false }), { block: true, reason: G1 });
  });

  test(`${name}: G2 refuses a PLANNOTATOR_REMOTE that is not 1`, () => {
    for (const v of [undefined, "0", "true", ""]) assert.deepEqual(call(tool, input, { env: { ...goodEnv, PLANNOTATOR_REMOTE: v } }), { block: true, reason: G2("PLANNOTATOR_REMOTE is not 1") });
  });

  test(`${name}: G2 refuses a PLANNOTATOR_URL_HOST that is empty or loopback`, () => {
    for (const v of [undefined, "", "localhost", "127.0.0.1", "127.1.2.3", "::1", "[::1]", "0.0.0.0"]) {
      assert.deepEqual(call(tool, input, { env: { ...goodEnv, PLANNOTATOR_URL_HOST: v } }), { block: true, reason: G2("PLANNOTATOR_URL_HOST is empty or a loopback name or address") }, String(v));
    }
  });

  test(`${name}: G3 refuses an absent registry and one of the wrong shape, naming the cause and the fix`, () => {
    for (const store of [{}, { [REGISTRY_KEY]: {} }, { [REGISTRY_KEY]: "x" }, { [REGISTRY_KEY]: { ...goodRegistry(), openFor: 1 } }]) {
      assert.deepEqual(call(tool, input, { store }), { block: true, reason: G3 });
    }
  });

  test(`${name}: a nested call (parentToolCallId) is blocked too`, () => {
    assert.deepEqual(call(tool, input, { live: false }, "parent-1"), { block: true, reason: G1 });
    assert.deepEqual(call(tool, input, { env: { ...goodEnv, PLANNOTATOR_URL_HOST: "localhost" } }, "parent-1"), { block: true, reason: G2("PLANNOTATOR_URL_HOST is empty or a loopback name or address") });
  });

  test(`${name}: (1) with no CPD_SESSION the call passes, whatever else is wrong`, () => {
    assert.equal(call(tool, input, { env: {}, live: false, store: {} }), undefined);
    assert.equal(call(tool, input, { env: { ...goodEnv, CPD_SESSION: "" }, live: false, store: {} }), undefined);
  });
}

test("G2 passes a PLANNOTATOR_PORT that is a port range", () => {
  for (const [tool, input] of OPENERS) {
    for (const v of ["19437", "19432-19456", "20000-20000", "1-65535"]) {
      assert.equal(call(tool, input, { env: { ...goodEnv, PLANNOTATOR_PORT: v } }), undefined, `${tool} ${v}`);
    }
  }
});

test("G2 refuses a PLANNOTATOR_PORT that is neither a port nor a range of ports", () => {
  for (const [tool, input] of OPENERS) {
    for (const v of [undefined, "", "abc", "0", "70000", "19432,19433", "19441-19432", "0-5", "1-70000", "19432-", "-19441", "a-b", "19432-19441-19450", " 19432-19441"]) {
      assert.deepEqual(call(tool, input, { env: { ...goodEnv, PLANNOTATOR_PORT: v } }), { block: true, reason: G2("PLANNOTATOR_PORT is not a port or a range of ports") }, `${tool} ${String(v)}`);
    }
  }
});


test("G1 comes before G2, and G2 before G3", () => {
  const input = { action: "annotate" };
  assert.deepEqual(call("plannotator", input, { live: false, env: { ...goodEnv, PLANNOTATOR_REMOTE: "0" }, store: {} }), { block: true, reason: G1 });
  assert.deepEqual(call("plannotator", input, { env: { ...goodEnv, PLANNOTATOR_REMOTE: "0" }, store: {} }), { block: true, reason: G2("PLANNOTATOR_REMOTE is not 1") });
});

test("`list` and `close` pass, even when G1, G2 and G3 would refuse", () => {
  const bad: Over = { live: false, env: { ...goodEnv, PLANNOTATOR_REMOTE: "0" }, store: {} };
  assert.equal(call("plannotator", { action: "list" }, bad), undefined);
  assert.equal(call("plannotator", { action: "close", session: "all" }, bad), undefined);
  assert.equal(call("plannotator", { action: "close", session: "pn-1" }, bad, "parent-1"), undefined);
});

test("other tools pass: `plannotator_mark_done`, `bash`, `ask_user_question`", () => {
  const bad: Over = { live: false, env: {}, store: {} };
  for (const t of ["plannotator_mark_done", "bash", "ask_user_question", "cpd_plan_review"]) assert.equal(call(t, {}, bad), undefined, t);
  assert.equal(call("bash", { command: "ls" }, { live: false, env: goodEnv, store: {} }), undefined);
});
