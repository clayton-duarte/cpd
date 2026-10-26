import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { loadPi, main, PARALLEL, PI_MODULES, PI_NAMES, readTeamChannel, Refusal, resultText, type CallTool } from "../lib/slack-channel.ts";

const ME = "U04DB4MTUMR";
const CHANNEL = "C02JU6C2Z1D";
const here = dirname(fileURLToPath(import.meta.url));
const helper = join(here, "..", "lib", "slack-channel.ts");
const NOW = new Date(1790950000 * 1000 + 3600_000);

const block = (who: string, ts: string, body: string) => `=== Message from ${who} <x@missionlane.com> (U01) at 2026-10-02 09:00:00 EDT === \nMessage TS: ${ts}\n${body}\n\n`;
const pr = (n: number) => `<https://github.com/acme/pay/pull/${n}|github.com/acme/pay/pull/${n}>`;
const channelText =
  block("Ryan", "1790950000.000100", `review ${pr(812)}\nReactions: eyes (1)`) +
  block("Wes", "1790940000.000200", `please ${pr(813)}`) +
  block("Ann", "1790930000.000300", `<@U01HPPJ6DQT|Wes> ${pr(814)}`);

/** A fake channel: answers by tool and records every call. */
function fake(answers: Record<string, string | ((args: Record<string, unknown>) => string)>) {
  const calls: Array<{ name: string; args: Record<string, unknown>; timeoutMs: number }> = [];
  const call: CallTool = async (name, args, timeoutMs) => {
    calls.push({ name, args, timeoutMs });
    const answer = answers[name];
    if (answer === undefined) throw new Refusal("slack_failed", `${name} failed`);
    return typeof answer === "function" ? answer(args) : answer;
  };
  return { call, calls };
}

test("resultText joins the text blocks, as slack_fetch._result_text does", () => {
  assert.equal(resultText({ content: [{ type: "text", text: "a" }, { type: "image" }, { type: "text", text: "" }, { type: "text", text: "b" }] }), "a\nb");
});

test("reads the channel with {channel_id, limit: 200}, each thread, and the eyes reactors of claimed posts", async () => {
  const f = fake({
    slack_read_channel: channelText,
    slack_read_thread: (a) => (a.message_ts === "1790940000.000200" ? "From: Ryan (U01)\n<@U04DB4MTUMR|Clayton> look?" : "nothing"),
    slack_get_reactions: ":eyes: \u00d7 1 \u2014 clayton (U04DB4MTUMR)",
  });
  const out = await readTeamChannel(f.call, CHANNEL, ME, NOW);
  assert.deepEqual(f.calls[0], { name: "slack_read_channel", args: { channel_id: CHANNEL, limit: 200 }, timeoutMs: 45_000 });
  assert.equal(out.read, 3);
  assert.deepEqual(
    out.posts.map((p) => [p.number, p.claim?.reason ?? null, p.eyes_me, p.thread_tags_me]),
    [
      [812, "claimed with :eyes: in Slack", true, false],
      [813, null, null, true],
      [814, "assigned to someone in Slack", null, false],
    ],
  );
  assert.deepEqual(f.calls.filter((c) => c.name === "slack_read_thread").map((c) => c.args.message_ts).sort(), ["1790930000.000300", "1790940000.000200", "1790950000.000100"]);
  assert.deepEqual(f.calls.filter((c) => c.name === "slack_get_reactions").map((c) => c.args), [{ channel_id: CHANNEL, message_ts: "1790950000.000100" }], "only the post with a claim and an eyes reaction");
  assert.ok(f.calls.slice(1).every((c) => c.timeoutMs === 30_000));
});

test("a payload that does not parse into messages is slack_unparseable, whatever its size (the cache is kept)", async () => {
  for (const text of ["I could not read the channel, sorry.", JSON.stringify({ result: "Raw response above, delivered as returned." })]) {
    await assert.rejects(readTeamChannel(fake({ slack_read_channel: text }).call, CHANNEL, ME, NOW), (e: Refusal) => {
      assert.equal(e.code, "slack_unparseable");
      assert.equal(e.exit, 1);
      assert.equal(e.details.bytes, text.length);
      return true;
    });
  }
});

test("no text is slack_failed; a quiet channel with messages outside the window is not unparseable", async () => {
  await assert.rejects(readTeamChannel(fake({ slack_read_channel: "  \n" }).call, CHANNEL, ME, NOW), { code: "slack_failed" });
  const out = await readTeamChannel(fake({ slack_read_channel: block("Old", "1600000000.000000", pr(1)) }).call, CHANNEL, ME, NOW);
  assert.deepEqual([out.read, out.posts], [1, []]);
});

test("a thread or reaction read that fails is soft (false, null); a lost sign-in is not", async () => {
  const f = fake({ slack_read_channel: channelText });
  const out = await readTeamChannel(f.call, CHANNEL, ME, NOW);
  assert.deepEqual(out.posts.map((p) => [p.thread_tags_me, p.eyes_me]), [[false, null], [false, null], [false, null]]);
  const auth = fake({
    slack_read_channel: channelText,
    slack_read_thread: () => {
      throw new Refusal("slack_auth", "sign in");
    },
  });
  await assert.rejects(readTeamChannel(auth.call, CHANNEL, ME, NOW), { code: "slack_auth", exit: 3 });
});

test("thread reads run at most eight at a time", async () => {
  const many = Array.from({ length: 30 }, (_, i) => block("A", `${1790940000 + i}.000000`, pr(900 + i))).join("");
  let running = 0;
  let peak = 0;
  const call: CallTool = async (name) => {
    if (name === "slack_read_channel") return many;
    running++;
    peak = Math.max(peak, running);
    await new Promise((r) => setTimeout(r, 2));
    running--;
    return "";
  };
  await readTeamChannel(call, CHANNEL, ME, NOW);
  assert.equal(peak, PARALLEL);
});

// --- pi's modules, by file URL --------------------------------------------------------------------------------------

/** A directory shaped like a pi release's `dist/`, whose MCP modules are the given sources. */
function fakePi(config: string, runtime: string) {
  const root = mkdtempSync(join(tmpdir(), "cpd-slack-"));
  const dist = join(root, "dist");
  mkdirSync(join(dist, "extensions/mcp"), { recursive: true });
  writeFileSync(join(dist, "index.js"), "");
  writeFileSync(join(dist, PI_MODULES.config), config);
  writeFileSync(join(dist, PI_MODULES.runtime), runtime);
  return { root, entry: join(dist, "index.js"), done: () => rmSync(root, { recursive: true, force: true }) };
}

const FAKE_CONFIG = `export function loadMcpConfig({ agentDir, projectTrusted }) {
  if (projectTrusted !== false) throw new Error("a project file must never be read");
  return { servers: process.env.FAKE_NO_SLACK ? [] : [{ name: "slack", config: { url: "https://fake.invalid/mcp" } }], errors: [] };
}`;
const FAKE_RUNTIME = `
import { readFileSync } from "node:fs";
export function createDefaultTransport() {}
export class McpOAuthCredentialStore {}
export class McpServerConnection {
  constructor(o) { this.o = o; this.state = "connecting"; }
  async callTool(name, args, options) {
    if (!options.timeoutMs || !options.signal) throw new Error("no timeout");
    if (process.env.FAKE_AUTH) { this.state = "needs-auth"; throw new Error('MCP server "slack" requires sign-in. Run /mcp to sign in.'); }
    const answers = JSON.parse(readFileSync(process.env.FAKE_ANSWERS, "utf8"));
    return { content: [{ type: "text", text: answers[name] ?? "" }], isError: false };
  }
  async close() { this.state = "closed"; }
}`;

test("the helper reads through pi's own connection, loaded by file URL", async (t) => {
  const pi = fakePi(FAKE_CONFIG, FAKE_RUNTIME);
  t.after(pi.done);
  const answers = join(pi.root, "answers.json");
  writeFileSync(answers, JSON.stringify({ slack_read_channel: channelText, slack_read_thread: "", slack_get_reactions: "" }));
  const run = (env: Record<string, string>, extra: string[] = []) =>
    spawnSync(process.execPath, [helper, "--coding-agent", pi.entry, "--channel", CHANNEL, "--me", ME, ...extra], {
      env: { ...process.env, PI_CODING_AGENT_DIR: pi.root, FAKE_ANSWERS: answers, ...env },
      encoding: "utf8",
      timeout: 30_000,
    });

  const ok = run({});
  assert.equal(ok.status, 0, ok.stderr);
  const out = JSON.parse(ok.stdout);
  assert.deepEqual([out.ok, out.channel, out.read, out.posts.length], [true, CHANNEL, 3, 0], "the posts of 2026-10 are older than 7 days now");

  const auth = run({ FAKE_AUTH: "1" });
  assert.equal(auth.status, 3);
  assert.equal(JSON.parse(auth.stdout).error.code, "slack_auth");
  assert.match(JSON.parse(auth.stdout).error.message, /pi mcp login slack/);

  const none = run({ FAKE_NO_SLACK: "1" });
  assert.equal(none.status, 3);
  assert.equal(JSON.parse(none.stdout).error.code, "slack_not_configured");
  assert.match(JSON.parse(none.stdout).error.message, /pi mcp add slack/);

  const usage = run({}, ["--bogus", "x"]);
  assert.equal(usage.status, 2);
  assert.equal(JSON.parse(usage.stdout).error.code, "usage");
});

test("a helper reached through a symlink to its script or to its directory still runs main", async (t) => {
  const pi = fakePi(FAKE_CONFIG, FAKE_RUNTIME);
  const root = realpathSync(mkdtempSync(join(tmpdir(), "slack-link-")));
  t.after(() => {
    pi.done();
    rmSync(root, { recursive: true, force: true });
  });
  const answers = join(pi.root, "answers.json");
  writeFileSync(answers, JSON.stringify({ slack_read_channel: channelText, slack_read_thread: "", slack_get_reactions: "" }));
  symlinkSync(realpathSync(helper), join(root, "script.ts"));
  symlinkSync(realpathSync(join(here, "..")), join(root, "pi"));
  for (const script of [join(root, "script.ts"), join(root, "pi", "lib", "slack-channel.ts")]) {
    const r = spawnSync(process.execPath, [script, "--coding-agent", pi.entry, "--channel", CHANNEL, "--me", ME], {
      env: { ...process.env, PI_CODING_AGENT_DIR: pi.root, FAKE_ANSWERS: answers },
      encoding: "utf8",
      timeout: 30_000,
    });
    assert.equal(r.status, 0, `${script}: ${r.stderr}`);
    assert.notEqual(r.stdout, "", `${script} printed nothing`);
    const out = JSON.parse(r.stdout);
    assert.deepEqual([out.ok, out.channel, out.read], [true, CHANNEL, 3], script);
  }
});

test("a module that only imports the helper does not run main", (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "slack-import-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const script = join(root, "importer.ts");
  writeFileSync(script, `import ${JSON.stringify(pathToFileURL(realpathSync(helper)).href)};\n`);
  const r = spawnSync(process.execPath, [script], { encoding: "utf8", timeout: 30_000 });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, "");
});

test("main parses and answers without a process: the posts with ts, author, primary, reactions, claim, thread tags", async (t) => {
  const pi = fakePi(FAKE_CONFIG, FAKE_RUNTIME);
  t.after(pi.done);
  const answers = join(pi.root, "answers.json");
  writeFileSync(answers, JSON.stringify({ slack_read_channel: channelText, slack_read_thread: "<@U04DB4MTUMR|C> hi" }));
  process.env.FAKE_ANSWERS = answers;
  const { value, exit } = await main(["--coding-agent", pi.entry, "--channel", CHANNEL, "--me", ME], { PI_CODING_AGENT_DIR: pi.root }, pi.root, NOW);
  assert.equal(exit, 0);
  const v = value as { posts: Array<Record<string, unknown>> };
  assert.deepEqual(v.posts[0], {
    repo: "acme/pay",
    number: 812,
    ts: 1790950000.0001,
    ts_raw: "1790950000.000100",
    author: "Ryan",
    primary: true,
    reactions: ["eyes"],
    claim: { kind: "claimed", reason: "claimed with :eyes: in Slack" },
    eyes_me: null,
    thread_tags_me: true,
  });
});

test("a pi whose modules lack a name, or fail to load, is slack_helper_failed with the thrown message", async (t) => {
  const noName = fakePi(FAKE_CONFIG, "export function createDefaultTransport() {}\nexport class McpServerConnection {}");
  const broken = fakePi("throw new Error('boom from pi');", FAKE_RUNTIME);
  t.after(() => [noName, broken].forEach((p) => p.done()));
  await assert.rejects(loadPi(noName.entry), (e: Refusal) => e.code === "slack_helper_failed" && /does not export McpOAuthCredentialStore/.test(e.message));
  await assert.rejects(loadPi(broken.entry), (e: Refusal) => e.code === "slack_helper_failed" && e.details.message === "boom from pi");
  await assert.rejects(loadPi("/nonexistent/dist/index.js"), { code: "slack_helper_failed", exit: 1 });
});

// --- the import guard (docs/research/team-review.md §5) -----------------------------------------------------------

test("the four names of pi's own modules exist in the pinned devDependency, by file URL", async () => {
  const pkg = join(here, "..", "..", "node_modules", "@earendil-works", "pi-coding-agent");
  const pinned = JSON.parse((await import("node:fs")).readFileSync(join(here, "..", "..", "package.json"), "utf8")).devDependencies["@earendil-works/pi-coding-agent"];
  assert.match(pinned, /^\d+\.\d+\.\d+$/, "an exact pin, so a bump fails here before an install does");
  assert.equal(JSON.parse((await import("node:fs")).readFileSync(join(pkg, "package.json"), "utf8")).version, pinned);
  const loaded = await loadPi(join(pkg, "dist", "index.js"));
  for (const name of [...PI_NAMES.config, ...PI_NAMES.runtime]) assert.equal(typeof loaded[name], "function", name);
});
