import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import { buildClassifierRequest, COMPLETION_LABELS, COMPLETION_QUESTION, createCompletionClassifier, mapAnswer, type CompletionPayload } from "./completion-classify.ts";
import type { ChildBoundary } from "./plugin.ts";

const payload: CompletionPayload = {
  session: { id: "s1", key: "c:p", role: "lead", state: "idle", hasOpenDialog: false },
  plan: null,
  ledgerActions: ["cpd context decide x"],
  transcript: [{ kind: "user", text: "hi" }],
};

describe("buildClassifierRequest", () => {
  it("buildClassifierRequest carries the state the question and the five labels", () => {
    const r = buildClassifierRequest(payload);
    expect(r.state).toEqual(payload);
    expect(r.question).toBe(COMPLETION_QUESTION);
    expect(r.labels).toEqual(COMPLETION_LABELS);
    expect(r.labels.map((l) => l.name)).toEqual(["safe_to_archive", "unanswered_ask", "open_work", "unrecorded_decision", "unescalated_issue"]);
  });
});

const ok = (label: string, extra: Record<string, unknown> = {}) => ({ ok: true, label, model: "llama.cpp/kev-4b", ...extra });

describe("mapAnswer", () => {
  it("mapAnswer lists the other labels' probabilities sorted high to low", () => {
    const probabilities = { safe_to_archive: 0.46, unanswered_ask: 0.24, open_work: 0.1, unrecorded_decision: 0.11, unescalated_issue: 0.09 };
    const r = mapAnswer(ok("open_work", { probabilities }));
    expect(r?.others).toEqual([
      { label: "safe_to_archive", score: 46 },
      { label: "unanswered_ask", score: 24 },
      { label: "unrecorded_decision", score: 11 },
      { label: "unescalated_issue", score: 9 },
    ]);
    expect(r).toMatchObject({ label: "open_work", text: "Open work · 46%", score: 46, tone: "decision" });
  });
  it("mapAnswer omits others when the answer has no probabilities", () => {
    const a = mapAnswer(ok("safe_to_archive", { confidence: 0.8 }));
    expect(a).not.toBeNull();
    expect("others" in a!).toBe(false);
    expect(mapAnswer(ok("open_work", { probabilities: { safe_to_archive: 0.5, unanswered_ask: "x" } }))?.others).toEqual([{ label: "safe_to_archive", score: 50 }]);
    const b = mapAnswer(ok("safe_to_archive", { probabilities: { safe_to_archive: "x", open_work: null } }));
    expect(b).not.toBeNull();
    expect("others" in b!).toBe(false);
  });
  it("mapAnswer maps safe_to_archive with probabilities", () => {
    expect(mapAnswer(ok("safe_to_archive", { probabilities: { safe_to_archive: 0.93 } }))).toEqual({ label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" });
  });
  it("mapAnswer maps safe_to_archive below 80 to decision", () => {
    expect(mapAnswer(ok("safe_to_archive", { probabilities: { safe_to_archive: 0.6 } }))).toEqual({ label: "safe_to_archive", text: "Safe to archive · 60%", score: 60, tone: "decision" });
  });
  it("mapAnswer maps unanswered_ask", () => {
    expect(mapAnswer(ok("unanswered_ask", { probabilities: { safe_to_archive: 0.2 } }))).toEqual({ label: "unanswered_ask", text: "Unanswered ask · 20%", score: 20, tone: "decision" });
  });
  it("mapAnswer maps open_work below 40 to blocked", () => {
    expect(mapAnswer(ok("open_work", { probabilities: { safe_to_archive: 0.1 } }))).toEqual({ label: "open_work", text: "Open work · 10%", score: 10, tone: "blocked" });
  });
  it("mapAnswer maps open_work at 40 or more to decision", () => {
    expect(mapAnswer(ok("open_work", { probabilities: { safe_to_archive: 0.4 } }))).toEqual({ label: "open_work", text: "Open work · 40%", score: 40, tone: "decision" });
  });
  it("mapAnswer maps unrecorded_decision", () => {
    expect(mapAnswer(ok("unrecorded_decision", { probabilities: { safe_to_archive: 0.3 } }))).toEqual({ label: "unrecorded_decision", text: "Unrecorded decision · 30%", score: 30, tone: "decision" });
  });
  it("mapAnswer maps unescalated_issue", () => {
    expect(mapAnswer(ok("unescalated_issue", { probabilities: { safe_to_archive: 0.15 } }))).toEqual({ label: "unescalated_issue", text: "Unescalated bug · 15%", score: 15, tone: "decision" });
  });
  it("mapAnswer falls back to confidence without probabilities", () => {
    expect(mapAnswer(ok("unanswered_ask", { confidence: 0.7 }))?.score).toBe(30);
    expect(mapAnswer(ok("safe_to_archive", { confidence: 0.9 }))?.score).toBe(90);
    expect(mapAnswer(ok("safe_to_archive"))?.score).toBe(100);
    expect(mapAnswer(ok("open_work"))?.score).toBe(50);
  });
  it("mapAnswer returns null for ok false an unknown label or a malformed answer", () => {
    expect(mapAnswer(ok("safe_to_archive", { probabilities: { safe_to_archive: 0.9 } }))).not.toBeNull();
    expect(mapAnswer({ ok: false, error: "boom" })).toBeNull();
    expect(mapAnswer(ok("something_else"))).toBeNull();
    expect(mapAnswer({ ok: true, model: "m" })).toBeNull();
    expect(mapAnswer(null)).toBeNull();
    expect(mapAnswer("safe_to_archive")).toBeNull();
    expect(mapAnswer(42)).toBeNull();
  });
});

type Spawned = { file: string; args: string[]; env: NodeJS.ProcessEnv | undefined };
const fakeChild = () => {
  const spawned: Spawned[] = [];
  const stdin: string[] = [];
  let ended = 0;
  let killed = 0;
  let onData: (c: Buffer) => void = () => undefined;
  let onClose: (code: number | null) => void = () => undefined;
  let throwOnSpawn = false;
  let attempts = 0;
  const child: ChildBoundary = {
    execFile: () => {
      throw new Error("unexpected execFile");
    },
    spawn: (file, args, options) => {
      attempts += 1;
      if (throwOnSpawn) throw new Error("spawn failed");
      spawned.push({ file, args, env: options.env });
      return {
        stdin: { write: (c: string) => stdin.push(c), end: () => { ended += 1; }, on: () => undefined },
        stdout: { on: (_e, h) => { onData = h; } },
        stderr: { on: () => undefined },
        on: (_e, h) => { onClose = h; },
        kill: () => { killed += 1; },
      };
    },
  };
  return {
    child, spawned, stdin,
    ended: () => ended, killed: () => killed,
    out: (s: string) => onData(Buffer.from(s)),
    close: (code: number | null) => onClose(code),
    failSpawn: () => { throwOnSpawn = true; },
    attempts: () => attempts,
  };
};
const make = (f: ReturnType<typeof fakeChild>, script: string | null = "/x/classify.ts") => {
  const scheduler = new FakeScheduler();
  return { scheduler, classifier: createCompletionClassifier({ child: f.child, scheduler, script, agentDir: "/agent", env: { PATH: "/bin" } }) };
};
const line = (o: unknown) => `${JSON.stringify(o)}\n`;
const flush = () => Promise.resolve().then(() => Promise.resolve());

describe("classify", () => {
  it("classify spawns node with the request on stdin and returns the mapped completion", async () => {
    const f = fakeChild();
    const { classifier } = make(f);
    const p = classifier.classify(payload);
    await flush();
    expect(f.spawned).toHaveLength(1);
    expect(f.spawned[0]?.file).toBe("node");
    expect(f.spawned[0]?.args).toEqual(["/x/classify.ts"]);
    expect(f.spawned[0]?.env?.["PI_CODING_AGENT_DIR"]).toBe("/agent");
    expect(f.spawned[0]?.env?.["PATH"]).toBe("/bin");
    expect(JSON.parse(f.stdin.join(""))).toEqual(buildClassifierRequest(payload));
    expect(f.ended()).toBe(1);
    f.out(line(ok("safe_to_archive", { probabilities: { safe_to_archive: 0.93 } })));
    f.close(0);
    await expect(p).resolves.toEqual({ label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" });
  });
  it("classify takes the last JSON line of stdout", async () => {
    const f = fakeChild();
    const { classifier } = make(f);
    const p = classifier.classify(payload);
    await flush();
    f.out("warming up\n");
    f.out(line(ok("open_work", { probabilities: { safe_to_archive: 0.1 } })));
    f.out("\n\n");
    f.close(0);
    await expect(p).resolves.toMatchObject({ label: "open_work", score: 10 });
  });
  it("classify resolves null on a non-zero exit", async () => {
    const f = fakeChild();
    const { classifier } = make(f);
    const p = classifier.classify(payload);
    await flush();
    expect(f.spawned).toHaveLength(1);
    f.out(line(ok("safe_to_archive", { probabilities: { safe_to_archive: 0.9 } })));
    f.close(1);
    await expect(p).resolves.toBeNull();
  });
  it("classify resolves null on unparseable output", async () => {
    const f = fakeChild();
    const { classifier } = make(f);
    const p = classifier.classify(payload);
    await flush();
    expect(f.spawned).toHaveLength(1);
    f.out("not json\n");
    f.close(0);
    await expect(p).resolves.toBeNull();
  });
  it("classify resolves null when spawn throws", async () => {
    const f = fakeChild();
    f.failSpawn();
    const { classifier } = make(f);
    await expect(classifier.classify(payload)).resolves.toBeNull();
    expect(f.attempts()).toBe(1);
  });
  it("classify resolves null with no script and spawns nothing", async () => {
    const f = fakeChild();
    const { classifier } = make(f, null);
    await expect(classifier.classify(payload)).resolves.toBeNull();
    expect(f.spawned).toHaveLength(0);
    const g = fakeChild();
    void make(g).classifier.classify(payload);
    await flush();
    expect(g.spawned).toHaveLength(1); // control: with a script the same call spawns
  });
  it("classify kills the child and resolves null at the 30000 ms deadline", async () => {
    const f = fakeChild();
    const { classifier, scheduler } = make(f);
    const p = classifier.classify(payload);
    await flush();
    expect(scheduler.pending()).toBe(1);
    scheduler.advance(29_999);
    expect(f.killed()).toBe(0);
    scheduler.advance(1);
    expect(f.killed()).toBe(1);
    await expect(p).resolves.toBeNull();
  });
});
