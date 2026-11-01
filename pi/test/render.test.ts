import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, mock } from "node:test";
import { type Component } from "@earendil-works/pi-tui";
import { errorMessage as rawError, eventMessage as rawEvent, sessionMessage as rawSession } from "../lib/delivery.ts";
import type { CpdEvent, EventOf } from "../lib/event.ts";
import type { Message, MessageRenderer, RenderedMessage, Theme } from "../lib/pi.ts";
import { loadStyle, RENDERED_TYPES, Rendering, type Style } from "../lib/render.ts";
import { contextUpdate, type ContextRecord, type PendingPlan, type PlanEvent } from "../lib/updates.ts";
import { ev, Fixture, ID, OTHER } from "./fake.ts";

// Pin the process timezone so the expected local times are deterministic and prove the rendering is local, not UTC.
process.env.TZ = "Asia/Kolkata";

/** hs-d637: the one known instant every surface renders; in Asia/Kolkata (UTC+05:30) it is 03:11 the next day. */
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
const TIME = "03:11";

/** The test message constructors (pi/test/): carry pi's `CustomMessage.timestamp`, the source of the fold's local time. */
const eventMessage = (...args: Parameters<typeof rawEvent>) => ({ ...rawEvent(...args), timestamp: TIME_MS });
const sessionMessage = (...args: Parameters<typeof rawSession>) => ({ ...rawSession(...args), timestamp: TIME_MS });
const errorMessage = (...args: Parameters<typeof rawError>) => {
  const sent = rawError(...args);
  return { ...sent, message: { ...sent.message, timestamp: TIME_MS } };
};
const timed = <T extends object>(message: T): T & { timestamp: number } => ({ ...message, timestamp: TIME_MS });

/** commands.md §15.9 (hs-d286): every cpd message is a collapsed row like pi-compact-tools' tool rows; the model's `content` is untouched. */

// A fake theme that paints real SGR codes, so widths and wrapping behave, and that `tagged` reads back as `<colour>…</>` and `<b>…</b>`.
const COLORS: string[] = [];
const theme: Theme = {
  fg: (color, text) => {
    if (!COLORS.includes(color)) COLORS.push(color);
    return `\x1b[38;5;${COLORS.indexOf(color)}m${text}\x1b[39m`;
  },
  bg: (_color, text) => `\x1b[48;5;0m${text}\x1b[49m`,
  bold: (text) => `\x1b[1m${text}\x1b[22m`,
};
const tagged = (line: string): string =>
  line
    .replace(/\x1b\[48;5;0m|\x1b\[49m/g, "")
    .replace(/\x1b\[38;5;(\d+)m/g, (_m, n) => `<${COLORS[Number(n)]}>`)
    .replace(/\x1b\[39m/g, "</>")
    .replace(/\x1b\[90m/g, "<c8>")
    .replace(/\x1b\[35m/g, "<c5>")
        .replace(/\x1b\[37m/g, "<t7>")
    .replace(/\x1b\[7m/g, "<inv>")
    .replace(/\x1b\[27m/g, "</inv>")
    .replace(/\x1b\[1m/g, "<b>")
    .replace(/\x1b\[22m/g, "</b>")
    .trimEnd();

const WIDTH = 200;
const draw = (c: Component | undefined, width = WIDTH): string[] => c!.render(width).map(tagged);
const rendering = (style: Style = "compact") => Object.assign(new Rendering(), { style });
const show = (message: RenderedMessage, expanded: boolean, opts: { style?: Style; width?: number; outputPad?: number } = {}) =>
  draw(rendering(opts.style).render(message, { expanded, outputPad: opts.outputPad ?? 1 }, theme), opts.width);
const plain = (lines: string[]) => lines.map((l) => l.replace(/<\/?[a-zA-Z0-9]*>/g, ""));

const CAP_L = "\u{E0B6}";
const CAP_R = "\u{E0B4}";
const DOT = "\u{2981}";
// hs-d534, hs-d537, hs-d708: `╭─` 8; the pill by explicit SGR (caps colour 5, inverted text), the bold φ and the first sentence; no dot.
const first = (summary: string): string => {
  const at = summary.search(/[.:]/);
  const s = at >= 0 && at < summary.length - 1 ? summary.slice(0, at).trim() : summary;
  return s.length > 60 ? `${s.slice(0, 60)}…` : s;
};
const HEAD = (summary: string) => `<c8>╭─</><c5>${CAP_L}<inv><b>φ</b></inv><c5>${CAP_R}</> <t7>${first(summary)}</>`;
const PLAIN_HEAD = (summary: string) => `╭─${CAP_L}φ${CAP_R} ${first(summary)}`;
const TERM = `<c8>╰\u{F0B2A}</>`;
const rail = (line: string) => `<c8>│ </>${line === "" ? "" : `<t7>${line}</>`}`;
const FOLDED = "\u{F0B26}";
const OPEN = "\u{F0B2C}";
const closing = (word: string) => `<c8>╰</><t7>${word === "expand" ? FOLDED : OPEN} ${word} · ${TIME}</>`;

const sessionEvent = (over: Record<string, unknown>): EventOf<"session-message"> => ev(7, "session-message", over) as unknown as EventOf<"session-message">;
const event = (kind: string, over: Record<string, unknown> = {}): CpdEvent => ev(7, kind, over) as unknown as CpdEvent;
const deep = <T>(m: T): T => structuredClone(m);

// ---- Event ----

test("the cpd message pill reads a bold phi", () => {
  const m = eventMessage(event("job-merged", { text: "Job api-1 merged." }), ID);
  const row = draw(rendering().render(m, { expanded: false, outputPad: 1 }, theme))[0]!;
  assert.ok(row.includes(`<c5>${CAP_L}<inv><b>φ</b></inv><c5>${CAP_R}</>`), `the pill holds the bold φ: ${JSON.stringify(row)}`);
  assert.ok(!row.includes("cpd"), `the pill no longer reads cpd: ${JSON.stringify(row)}`);
});

test("an Event, collapsed: ╭⦁, the bold title, a muted ╰󰬦 expand", () => {
  const m = eventMessage(event("job-merged", { text: "Job api-1 merged.\nsecond line" }), ID);
  assert.deepEqual(show(m, false), [HEAD("Job api-1 merged."), closing("expand")]);
});

test("an Event, expanded: the title stays, the full content behind a muted │ , then a muted ╰󰬬 collapse", () => {
  const m = eventMessage(event("job-merged", { text: "Job api-1 merged.\nsecond line" }), ID);
  assert.deepEqual(show(m, true), [HEAD("Job api-1 merged."), rail("Job api-1 merged."), rail("second line"), closing("collapse")]);
});

test("a long summary is cut to 60 characters and `…`", () => {
  const m = eventMessage(event("escalation", { text: `${"word ".repeat(60)}` }), ID);
  const summary = plain(show(m, false))[0]!.slice(`╭─${CAP_L}φ${CAP_R} `.length);
  assert.equal(summary.length, 61);
  assert.ok(summary.endsWith("…"));
});

test("a head wider than the pane wraps under the title, it is never cut", () => {
  const m = eventMessage(event("job-merged", { text: "alpha beta gamma delta epsilon zeta" }), ID);
  const lines = plain(show(m, false, { width: 24 }));
  assert.ok(lines[0]!.startsWith(`╭─${CAP_L}φ${CAP_R} alpha`), lines[0]);
  assert.ok(lines.length >= 3 && lines.slice(1, -1).every((l) => l.startsWith("│ ")), "continuation rows are aligned like a tool row's");
  assert.ok(lines.slice(0, -1).join("").replace(/[\s│╭─]|φ/gu, "").includes("zeta"), "never cut");
  assert.equal(lines.at(-1), `╰${FOLDED} expand · ${TIME}`);
});

test("an empty content shows no body", () => {
  const m = eventMessage(event("job-merged", { text: "x" }), ID, "");
  assert.deepEqual(show(m, true), [HEAD("x"), TERM]);
});

// ---- Session messages ----

test("a message from the User: `Message from the User`", () => {
  const m = sessionMessage(sessionEvent({ data: { from: null, text: "hi there", wake: false } }), ID, null);
  assert.deepEqual(show(m, false), [HEAD("Message from the User"), closing("expand")]);
  assert.deepEqual(show(m, true), [HEAD("Message from the User"), rail("Message from the User (03:11 +05:30): hi there"), closing("collapse")]);
});

test("a message from a Session: its name, or its id when it has none", () => {
  const named = sessionMessage(sessionEvent({ data: { from: OTHER, text: "hello", wake: false } }), ID, { id: OTHER, name: "cross-repo" });
  assert.deepEqual(show(named, false), [HEAD("Message from cross-repo"), closing("expand")]);
  assert.deepEqual(show(named, true).slice(1, 2), [rail(`Message from cross-repo (${OTHER}) (03:11 +05:30): hello`)]);
  const unnamed = sessionMessage(sessionEvent({ data: { from: OTHER, text: "hello", wake: false } }), ID, { id: OTHER, name: null });
  assert.deepEqual(show(unnamed, false)[0], HEAD(`Message from ${OTHER}`));
  assert.deepEqual(show(unnamed, true).slice(1, 2), [rail(`Message from ${OTHER} (03:11 +05:30): hello`)]);
});

// ---- the adapter's errors ----

test("an adapter error: `adapter: <what>`", () => {
  const { message } = errorMessage("session show", "bad_output", "cpd session show: the session object lacks role", { x: 1 });
  assert.deepEqual(show(message, false), [HEAD("adapter: session show"), closing("expand")]);
  assert.deepEqual(show(message, true)[1], rail("<b>φ</b> adapter: session show: bad_output: cpd session show: the session object lacks role"));
});

test("an `error` Event (it has an id) is an Event: its text", () => {
  const m = eventMessage(event("error", { text: "Something broke." }), ID);
  assert.equal(show(m, false)[0], HEAD("Something broke."));
});

// ---- cpd-context ----

const record = (n: number): ContextRecord => ({ seq: 40 + n, id: `hs-n${n}`, kind: "note", at: "2026-10-17T09:00:00.000Z", by: OTHER, text: `note ${n}` });
const planEvent = (n: number): PlanEvent => ({ event: 300 + n, at: "2026-10-18T09:00:00Z", plan: "pay-4521", text: `Plan phase ${n}.`, kind: "plan-phase", data: {} });
const pending: PendingPlan[] = [{ id: "pay-4521", phase: "dispatch", jobs: [], prs: [{ key: "api", status: "dispatch", number: 212, jobs: [{ key: "j1", status: "queued", title: "T" }] }] }];
const context = () => timed(contextUpdate("ctx", ID, [1, 2].map((n) => ({ event: 200 + n, record: `hs-n${n}`, seq: 40 + n })), [record(1), record(2)], [planEvent(1), planEvent(2), planEvent(3)], pending)!);
const FULL = [
  "Context ctx: 2 new records since your last turn.",
  "- hs-n1 note (2026-10-17 UTC): note 1",
  "- hs-n2 note (2026-10-17 UTC): note 2",
  "",
  "Plans:",
  "- pay-4521: 3 phase changes",
  "",
  "Pending work:",
  "- pay-4521 (dispatch): PR api 1 queued",
];

test("cpd-context, collapsed: its counts line is the summary", () => {
  assert.deepEqual(show(context(), false), [HEAD("cpd-context: 2 records · 3 Plan Events · 1 Plan pending"), closing("expand")]);
});

test("cpd-context, expanded: the full text", () => {
  assert.deepEqual(show(context(), true), [HEAD("cpd-context: 2 records · 3 Plan Events · 1 Plan pending"), ...FULL.map((l) => rail(l)), closing("collapse")]);
});

test("cpd-context whose details lack the counts (an older adapter's): always open, no closing row", () => {
  const old = { ...context(), details: { event: 1, delivered_to: ID, records: ["hs-n1"] } };
  const expected = [HEAD("cpd-context"), ...FULL.map((l) => rail(l)), TERM];
  assert.deepEqual(show(old, false), expected);
  assert.deepEqual(show(old, true), expected);
  const component = rendering().render(old, { expanded: false, outputPad: 1 }, theme) as Component & { handleMouse?: unknown };
  assert.equal(component.handleMouse, undefined, "no toggle");
});

// ---- cpd-plan-decision ----

const decision = (approved: boolean): Message & { timestamp: number } => ({
  customType: "cpd-plan-decision",
  content: `Plan review rv-3 was ${approved ? "APPROVED" : "REJECTED"}.\nNext: cpd plan lock`,
  display: true,
  details: { plan: "p1", review: "rv-3", approved, current: true },
  timestamp: TIME_MS,
});

test("cpd-plan-decision: `Plan review <id> approved|rejected`", () => {
  assert.deepEqual(show(decision(true), false), [HEAD("Plan review rv-3 approved"), closing("expand")]);
  assert.deepEqual(show(decision(false), false)[0], HEAD("Plan review rv-3 rejected"));
  assert.deepEqual(show(decision(true), true), [HEAD("Plan review rv-3 approved"), rail("Plan review rv-3 was APPROVED."), rail("Next: cpd plan lock"), closing("collapse")]);
});

test("a review that is gone (no verdict) shows the first line of its content", () => {
  const lost: Message & { timestamp: number } = { customType: "cpd-plan-decision", content: "Plan review rv-3 is gone.\nCall cpd_plan_review again.", display: true, details: { review: "rv-3", lost: true }, timestamp: TIME_MS };
  assert.equal(show(lost, false)[0], HEAD("Plan review rv-3 is gone."));
});

test("a customType the adapter starts to display later: the first line of its content", () => {
  const m: Message & { timestamp: number } = { customType: "cpd-later", content: "first\nsecond", display: true, details: undefined, timestamp: TIME_MS };
  assert.equal(show(m, false)[0], HEAD("first"));
});

// ---- cpd-plan-summary ----

test("cpd-plan-summary: `Plan review <id> summary`, always open with its Markdown content in full, no closing row and no toggle", () => {
  const content = "# Review title\nPlan p1: Plan · review r1\n\nGoal: Show a review in chat\n\nApprove | Change";
  const m: Message & { timestamp: number } = { customType: "cpd-plan-summary", content, display: true, details: { plan: "p1", review: "r1" }, timestamp: TIME_MS };
  const expected = [HEAD("Plan review r1 summary"), rail("# Review title"), rail("Plan p1: Plan · review r1"), rail(""), rail("Goal: Show a review in chat"), rail(""), rail("Approve | Change"), TERM];
  assert.deepEqual(show(m, false), expected);
  assert.deepEqual(show(m, true), expected);
  const component = rendering().render(m, { expanded: false, outputPad: 1 }, theme) as Component & { handleMouse?: unknown };
  assert.equal(component.handleMouse, undefined, "no toggle");
});

// ---- display only ----

test("the message handed to the model is unchanged, whatever the display", () => {
  const messages = [eventMessage(event("job-merged"), ID), context(), decision(true), errorMessage("x", "c", "m").message];
  for (const m of messages) {
    const before = deep(m);
    Object.freeze(m);
    show(m, false);
    show(m, true);
    assert.deepEqual(m, before);
    assert.equal(m.display, true);
  }
  assert.equal(messages[1]!.content, FULL.join("\n"));
});

// ---- expanding ----

const click = (component: Component | undefined, button = "left") => (component as unknown as { handleMouse(e: unknown): unknown }).handleMouse({ type: "click", button, x: 0, y: 0, screenX: 0, screenY: 0, width: 80, height: 2, shift: false, alt: false, ctrl: false });

test("a left click toggles that one message; another button or event does nothing", () => {
  const r = rendering();
  const a = eventMessage(event("job-merged", { text: "A" }), ID);
  const b = eventMessage(event("job-merged", { text: "B" }), ID);
  const ca = r.render(a, { expanded: false, outputPad: 1 }, theme);
  const cb = r.render(b, { expanded: false, outputPad: 1 }, theme);
  assert.equal(click(ca, "right"), undefined);
  assert.deepEqual(click(ca), { handled: true });
  assert.deepEqual(draw(ca), [HEAD("A"), rail("A"), closing("collapse")]);
  assert.deepEqual(draw(cb), [HEAD("B"), closing("expand")]);
  click(ca);
  assert.deepEqual(draw(ca), [HEAD("A"), closing("expand")]);
});

test("Ctrl+O (options.expanded) opens and closes the message both ways, a click in between included", () => {
  const r = rendering();
  const m = eventMessage(event("job-merged", { text: "A" }), ID);
  const render = (expanded: boolean) => draw(r.render(m, { expanded, outputPad: 1 }, theme));
  assert.deepEqual(render(false), [HEAD("A"), closing("expand")]);
  click(r.render(m, { expanded: false, outputPad: 1 }, theme)); // a click opens this one
  assert.equal(render(false).length, 3, "a rebuild with the host still collapsed keeps the click");
  assert.equal(render(true).length, 3);
  assert.deepEqual(render(false), [HEAD("A"), closing("expand")], "Ctrl+O again closes it");
});

// ---- the other styles and the pane ----

test("Click to expand is gone; only the closing ╰ row names the toggle, muted", () => {
  const m = eventMessage(event("job-merged", { text: "A\nB" }), ID);
  for (const style of ["compact", "claude", "codex"] as const) {
    for (const expanded of [false, true]) {
      const lines = show(m, expanded, { style });
      assert.ok(!plain(lines).join("\n").includes("Click to"));
      assert.deepEqual(lines, expanded ? [HEAD("A"), rail("A"), rail("B"), closing("collapse")] : [HEAD("A"), closing("expand")]);
    }
  }
});

test("outputPad 0 gives the margin column back", () => {
  const m = eventMessage(event("job-merged", { text: "A" }), ID);
  assert.deepEqual(plain(show(m, false, { outputPad: 0 })), [`╭─${CAP_L}φ${CAP_R} A`, `╰${FOLDED} expand · ${TIME}`]);
});

test("style off: no component, so pi draws its own box", () => {
  assert.equal(rendering("off").render(eventMessage(event("job-merged"), ID), { expanded: false, outputPad: 1 }, theme), undefined);
});

// ---- reading the extension's config, once ----

function scratch<T>(fn: (dirs: { agent: string; cwd: string; config: (dir: string, text: string) => string }) => T): T {
  const root = mkdtempSync(join(tmpdir(), "render-test-"));
  try {
    const agent = join(root, "agent");
    const cwd = join(root, "project");
    mkdirSync(agent);
    mkdirSync(join(cwd, ".pi"), { recursive: true });
    const config = (dir: string, text: string) => {
      writeFileSync(join(dir, "compact-tools.json"), text);
      return join(dir, "compact-tools.json");
    };
    // pi's getAgentDir() reads PI_CODING_AGENT_DIR (as pi/test/fake.ts sets it).
    const before = process.env.PI_CODING_AGENT_DIR;
    process.env.PI_CODING_AGENT_DIR = agent;
    try {
      return fn({ agent, cwd, config });
    } finally {
      if (before === undefined) delete process.env.PI_CODING_AGENT_DIR;
      else process.env.PI_CODING_AGENT_DIR = before;
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("no compact-tools.json: the extension's default, compact", () => {
  scratch(({ cwd }) => assert.equal(loadStyle({ cwd, trusted: true }), "compact"));
});

test("the agent directory's style, then a trusted project's over it; an untrusted project's is not read", () => {
  scratch(({ agent, cwd, config }) => {
    config(agent, '{"style": "claude", "mode": "silent"}');
    assert.equal(loadStyle({ cwd, trusted: false }), "claude");
    config(join(cwd, ".pi"), '{"style": "codex"}');
    assert.equal(loadStyle({ cwd, trusted: false }), "claude");
    assert.equal(loadStyle({ cwd, trusted: true }), "codex");
    config(join(cwd, ".pi"), '{"tools": []}');
    assert.equal(loadStyle({ cwd, trusted: true }), "claude", "a file without `style` keeps the one before it");
  });
});

test("a file that does not parse, and a style that is not one of the four, are reported as the extension reports them and the value before kept", () => {
  scratch(({ agent, cwd, config }) => {
    const errors = mock.method(console, "error", () => {});
    try {
      const bad = config(agent, "{nope");
      assert.equal(loadStyle({ cwd, trusted: false }), "compact");
      assert.equal(errors.mock.callCount(), 1);
      assert.match(String(errors.mock.calls[0]!.arguments[0]), new RegExp(`^\\[compact-tools\\] ${bad.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}: `));
      config(agent, '{"style": "fancy"}');
      assert.equal(loadStyle({ cwd, trusted: false }), "compact");
      assert.equal(errors.mock.calls[1]!.arguments[0], `[compact-tools] ${join(agent, "compact-tools.json")}: style must be one of compact, claude, codex, off; using previous value`);
      config(agent, "[1]");
      assert.equal(loadStyle({ cwd, trusted: false }), "compact");
      assert.equal(errors.mock.calls[2]!.arguments[0], `[compact-tools] ${join(agent, "compact-tools.json")}: expected a JSON object; using previous values`);
    } finally {
      errors.mock.restore();
    }
  });
});

// ---- registered by the adapter ----

test("the adapter registers one renderer per displayed customType once at load, for every role, and reads the style at session_start", async () => {
  const f = new Fixture();
  try {
    f.load();
    assert.deepEqual([...f.pi.renderers.keys()], ["cpd", "cpd-context", "cpd-plan-decision", "cpd-plan-summary", "cpd-skills"]);
    assert.deepEqual([...f.pi.renderers.keys()], [...RENDERED_TYPES]);
    mkdirSync(f.agentDir);
    f.file("agent/compact-tools.json", '{"style": "codex"}');
    const m = eventMessage(event("job-merged", { text: "Job done." }), ID);
    // The fake stores pi's own renderer type; call it as the adapter's narrow MessageRenderer, which it is.
    const renderer = f.pi.renderers.get("cpd") as unknown as MessageRenderer;
    const render = () => draw(renderer(m, { expanded: false, outputPad: 1 }, theme));
    assert.equal(render()[0], HEAD("Job done."), "before session_start");
    await f.pi.fire("session_start", { reason: "startup" });
    assert.deepEqual(render(), [HEAD("Job done."), closing("expand")], "every style but off draws the one fold");
  } finally {
    await f.close();
  }
});

test("the title row is ╭─, the title as a pill between U+E0B6 and U+E0B4, no U+2981, folded and expanded alike", () => {
  const m = eventMessage(event("job-merged", { text: "Job api-1 merged." }), ID);
  for (const expanded of [false, true]) {
    const lines = show(m, expanded);
    assert.equal(lines[0], HEAD("Job api-1 merged."));
    assert.equal(plain(lines)[0], PLAIN_HEAD("Job api-1 merged."));
    assert.ok(!lines.join("\n").includes(DOT), "no dot glyph");
  }
});
