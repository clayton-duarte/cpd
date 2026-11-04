import { describe, expect, it } from "vitest";
import { CPD_IDENTITY_VARS, cpdChildEnv, realBoundaries, type ChildBoundary } from "./plugin.ts";

const leaky: NodeJS.ProcessEnv = {
  CPD_SESSION: "s1",
  PI_SESSION_ID: "s2",
  CPD_SESSION_ID: "s3",
  CPD_ATTEMPT_ID: "a1",
  PATH: "/bin",
  CPD_HOME: "/h",
};

const noIdentity = (e: NodeJS.ProcessEnv | undefined) => {
  expect(e).toBeDefined();
  for (const k of CPD_IDENTITY_VARS) expect(e).not.toHaveProperty(k);
  expect(e?.["PATH"]).toBe("/bin");
};

it("cpdChildEnv removes the cpd and pi session identity variables", () => {
  noIdentity(cpdChildEnv(leaky));
});

it("cpdChildEnv keeps every other variable and does not mutate its input", () => {
  const input = { ...leaky };
  const out = cpdChildEnv(input);
  expect(out).toEqual({ PATH: "/bin", CPD_HOME: "/h" });
  expect(input).toEqual(leaky);
});

it("realBoundaries cpdExec runs cpd without the session identity variables", async () => {
  let seen: NodeJS.ProcessEnv | undefined;
  const child: ChildBoundary = {
    execFile: (_f, _a, options, cb) => {
      seen = options.env;
      cb(null, "", "");
      return undefined;
    },
    spawn: () => {
      throw new Error("unexpected spawn");
    },
  };
  await realBoundaries(leaky, child).cpdExec.run(["status"]);
  noIdentity(seen);
});

it("realBoundaries follow spawns cpd without the session identity variables", () => {
  let seen: NodeJS.ProcessEnv | undefined;
  const stream = { on: () => undefined };
  const child: ChildBoundary = {
    execFile: () => {
      throw new Error("unexpected execFile");
    },
    spawn: (_f, _a, options) => {
      seen = options.env;
      return { stdout: stream, stderr: stream, on: () => undefined, kill: () => undefined };
    },
  };
  realBoundaries(leaky, child).follow.start(["events", "follow"]);
  noIdentity(seen);
});
