import assert from "node:assert/strict";
import { test } from "node:test";
import { createTracker, REGISTRY_KEY } from "../lib/pntrack.ts";

// commands.md §15 "Every plannotator session is tracked": the tracker mirrors plannotator's review registry into `cpd review`.

const SESSION = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

/**
 * An open review as @plannotator/pi-extension 0.28.17 stores it: `PiOpenReview` at plannotator-tool-host.ts:38-62 (`id` `pn-` + 6 hex, `kind`, `subject`,
 * `url`, `startedAt`, `owner` = the Pi session id, `closedByAgent`), in the registry `createPiReviewRegistry` returns at plannotator-tool-host.ts:80-175 (methods
 * `add remove openFor openAll listText close`, listed at :190), kept on `globalThis` under the versioned key of :182.
 */
const review = (id: string, over: Record<string, unknown> = {}) => ({
  id, kind: "annotate", subject: `${id}.md`, url: "http://localhost:19432", startedAt: 1, owner: SESSION, closedByAgent: false, ...over,
});
type Review = ReturnType<typeof review>;

/** A registry of the 0.28.17 shape over a mutable list. */
function registry(open: Review[]) {
  const openAll = () => open.filter((r) => !r.closedByAgent);
  return {
    add: () => { throw new Error("not used"); },
    remove: (r: Review) => { open.splice(open.indexOf(r), 1); },
    openFor: (owner: string | undefined) => openAll().filter((r) => r.owner === owner),
    openAll,
    listText: () => "",
    close: () => ({ ok: true, text: "" }),
  };
}

/** A fake `cpd`: records argv and answers `review list` with `listed` (the open rows). */
function harness(open: Review[], listed: string[] = [], session: string | null = SESSION, store: Record<string, unknown> = { [REGISTRY_KEY]: registry(open) }) {
  const argvs: string[][] = [];
  const reports: string[] = [];
  const tracker = createTracker({
    session: session ?? undefined,
    store,
    cpd: async (argv: string[]) => {
      argvs.push(argv);
      return argv[1] === "list" ? JSON.stringify({ ok: true, reviews: listed.map((pn_id) => ({ pn_id })) }) : JSON.stringify({ ok: true });
    },
    report: (m: string) => void reports.push(m),
  });
  return { argvs, reports, tracker, store };
}

const opened = (pn: string, kind: string, subject: string, port: string) => ["review", "open", "--session", SESSION, "--pn", pn, "--kind", kind, "--subject", subject, "--port", port];
const closed = (pn: string, ended: string) => ["review", "close", "--session", SESSION, "--pn", pn, "--ended", ended];

test("a review of this Session in the registry runs `cpd review open` with its kind, subject and the port of its url", async () => {
  const open = [review("pn-1", { kind: "review", subject: "the diff", url: "http://localhost:19437/" })];
  const h = harness(open);
  await h.tracker.poll();
  assert.deepEqual(h.argvs, [opened("pn-1", "review", "the diff", "19437")]);
});

test("a review of another owner is not mirrored", async () => {
  const h = harness([review("pn-1", { owner: "someone-else" })]);
  await h.tracker.poll();
  assert.deepEqual(h.argvs, []);
});

test("a review that leaves the registry runs `cpd review close` (decided)", async () => {
  const open = [review("pn-1")];
  const h = harness(open);
  await h.tracker.poll();
  open.length = 0;
  await h.tracker.poll();
  assert.deepEqual(h.argvs, [opened("pn-1", "annotate", "pn-1.md", "19432"), closed("pn-1", "decided")]);
});

test("a review added later is opened, and the ones already known are not opened again", async () => {
  const open = [review("pn-1")];
  const h = harness(open);
  await h.tracker.poll();
  open.push(review("pn-2", { kind: "plan", subject: "PLAN.md" }));
  await h.tracker.poll();
  assert.deepEqual(h.argvs, [opened("pn-1", "annotate", "pn-1.md", "19432"), opened("pn-2", "plan", "PLAN.md", "19432")]);
});

test("(3) two polls of an unchanged registry run cpd once", async () => {
  const h = harness([review("pn-1")]);
  await h.tracker.poll();
  await h.tracker.poll();
  await h.tracker.poll();
  assert.equal(h.argvs.length, 1);
});

test("(3) an empty registry never runs cpd", async () => {
  const h = harness([]);
  await h.tracker.poll();
  await h.tracker.poll();
  assert.deepEqual(h.argvs, []);
});

test("at session_start a row `cpd review list` returns that the registry lacks is closed `lost`, and one it has is kept", async () => {
  const h = harness([review("pn-1")], ["pn-1", "pn-2"]);
  await h.tracker.start();
  assert.deepEqual(h.argvs[0], ["review", "list", "--session", SESSION]);
  assert.deepEqual(h.argvs.filter((a) => a[1] === "close"), [closed("pn-2", "lost")]);
});

test("at session_start the registry is read afterwards: the review already open is not run again as new if cpd listed it", async () => {
  const h = harness([review("pn-1")], ["pn-1"]);
  await h.tracker.start();
  await h.tracker.poll();
  assert.deepEqual(h.argvs.filter((a) => a[1] === "open"), []);
});

test("a registry of the wrong shape refuses (G3) and is reported once", async () => {
  const wrong = { openFor: () => [] }; // methods missing
  const h = harness([], [], SESSION, { [REGISTRY_KEY]: wrong });
  await h.tracker.poll();
  await h.tracker.poll();
  await h.tracker.start();
  assert.deepEqual(h.argvs.filter((a) => a[1] !== "list"), []);
  assert.equal(h.reports.length, 1);
  assert.match(h.reports[0]!, /review registry changed/);
});

test("an absent registry is not a refusal while nothing was ever opened: no cpd call, no report", async () => {
  const h = harness([], [], SESSION, {});
  await h.tracker.poll();
  assert.deepEqual(h.argvs, []);
  assert.deepEqual(h.reports, []);
});

test("(1) with no CPD_SESSION the tracker runs no `cpd review`, at start or at a poll", async () => {
  const h = harness([review("pn-1")], ["pn-2"], null);
  await h.tracker.start();
  await h.tracker.poll();
  assert.deepEqual(h.argvs, []);
  assert.deepEqual(h.reports, []);
});

test("(2) the fixture is a registry the parser accepts, and a registry missing any one method is refused", async () => {
  for (const method of ["add", "remove", "openFor", "openAll", "listText", "close"]) {
    const broken: Record<string, unknown> = { ...registry([review("pn-1")]) };
    delete broken[method];
    const h = harness([], [], SESSION, { [REGISTRY_KEY]: broken });
    await h.tracker.poll();
    assert.equal(h.reports.length, 1, `without ${method}`);
  }
  const ok = harness([review("pn-1")]);
  await ok.tracker.poll();
  assert.equal(ok.reports.length, 0);
});

test("(2) a registry entry of the wrong shape (no id of the form pn-xxxxxx, an unknown kind) is refused, not mirrored", async () => {
  for (const bad of [review("x-1"), review("pn-1", { kind: "other" }), review("pn-1", { url: "not a url" }), review("pn-1", { subject: "" })]) {
    const h = harness([bad]);
    await h.tracker.poll();
    assert.deepEqual(h.argvs, [], JSON.stringify(bad));
    assert.equal(h.reports.length, 1);
  }
});
