// The delta reads of the Slack helper (docs/architecture/swr-slack.md §3): the stdin `{version, known, terminal}` and the skip
// rules T1-T6 and E1-E4 (§3.4). A fake MCP client answers by tool and records every call: no network, no process, no pi.
// The channel text is the shape of slack-parse.test.ts's recorded fixture (`Reactions: name (n)`, `Thread: n replies (latest: …)`).
// Unverified (§7): that the real `slack_read_channel` carries those two lines; the implementation Worker checks it with `cpd mcp attach slack`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { main, readTeamChannel, Refusal, type CallTool, type Input, type Known } from "../lib/slack-channel.ts";

const ME = "U04DB4MTUMR";
const CHANNEL = "C02JU6C2Z1D";
const NOW = new Date(1790950000 * 1000 + 3600_000);

const OTHER_TS = "1790950000.000100"; // Ryan's post of acme/pay#812
const OWN_TS = "1790940000.000200"; // the User's post of acme/pay#813

const msg = (who: string, id: string, ts: string, body: string) => `=== Message from ${who} <x@missionlane.com> (${id}) at 2026-10-02 09:00:00 EDT === \nMessage TS: ${ts}\n${body}\n\n`;
const url = (n: number) => `<https://github.com/acme/pay/pull/${n}|github.com/acme/pay/pull/${n}>`;

/** Ryan's post (eyes ×`eyes`, `replies` replies) then the User's own post. */
const channel = (o: { eyes?: number; replies?: number; latest?: string } = {}) =>
  msg("Ryan", "U01", OTHER_TS, `review ${url(812)}\nReactions: eyes (${o.eyes ?? 2})\nThread: ${o.replies ?? 3} replies (latest: ${o.latest ?? "2026-10-02 09:20:00 EDT"})`) +
  msg("Clayton", ME, OWN_TS, `please ${url(813)}\nReactions: eyes (1)\nThread: 1 replies (latest: 2026-10-02 09:30:00 EDT)`);

const known = (ts: string, number: number, over: Partial<Known> = {}): Known => ({
  ts,
  repo: "acme/pay",
  number,
  author_is_me: false,
  reply_count: 3,
  latest_reply: "2026-10-02 09:20:00 EDT",
  reactions: { eyes: 2 },
  eyes_me: false,
  thread_tags_me: false,
  ...over,
});
const input = (k: Known[], terminal: string[] = []): Input => ({ version: 1, known: k, terminal });

function fake(text: string) {
  const calls: Array<{ name: string; args: Record<string, unknown> }> = [];
  const call: CallTool = async (name, args) => {
    calls.push({ name, args });
    if (name === "slack_read_channel") return text;
    if (name === "slack_read_thread") return `<@${ME}|Clayton> look`;
    if (name === "slack_get_reactions") return `:eyes: \u00d7 1 \u2014 clayton (${ME})`;
    throw new Refusal("slack_failed", `${name} failed`);
  };
  const reads = (name: string) => calls.filter((c) => c.name === name).map((c) => c.args.message_ts);
  return { call, reads };
}

const post = (out: Awaited<ReturnType<typeof readTeamChannel>>, number: number) => out.posts.find((p) => p.number === number) as unknown as Record<string, unknown>;

test("slack_own_post_skips_thread_and_reaction_reads", async () => {
  const f = fake(channel());
  const out = await readTeamChannel(f.call, CHANNEL, ME, NOW, input([known(OWN_TS, 813, { author_is_me: true, reactions: { eyes: 1 }, reply_count: 1, thread_tags_me: true, eyes_me: true }), known(OTHER_TS, 812)]));
  assert.ok(!f.reads("slack_read_thread").includes(OWN_TS), "T1: no thread read of the User's own message");
  assert.ok(!f.reads("slack_get_reactions").includes(OWN_TS), "E1: no reaction read of the User's own message");
  assert.equal(post(out, 813).author_is_me, true);
  assert.equal(post(out, 813).thread_tags_me, true, "the known value is echoed");
  assert.equal(post(out, 813).eyes_me, true, "the known value is echoed");
});

test("slack_terminal_pr_post_skips_thread_and_reaction_reads", async () => {
  const f = fake(channel({ eyes: 5, replies: 9 })); // both counts moved, yet the pull request is merged or closed
  const out = await readTeamChannel(f.call, CHANNEL, ME, NOW, input([known(OTHER_TS, 812, { thread_tags_me: true, eyes_me: true }), known(OWN_TS, 813, { author_is_me: true })], ["acme/pay#812"]));
  assert.ok(!f.reads("slack_read_thread").includes(OTHER_TS), "T2: no thread read when every pull request of the message is terminal");
  assert.ok(!f.reads("slack_get_reactions").includes(OTHER_TS), "E1: no reaction read either");
  assert.equal(post(out, 812).thread_tags_me, true);
  assert.equal(post(out, 812).eyes_me, true);
});

test("slack_reactions_read_only_when_eyes_count_changed_or_uncached", async () => {
  const same = fake(channel({ eyes: 2 }));
  const kept = await readTeamChannel(same.call, CHANNEL, ME, NOW, input([known(OTHER_TS, 812, { eyes_me: true }), known(OWN_TS, 813, { author_is_me: true })]));
  assert.deepEqual(same.reads("slack_get_reactions"), [], "E4: the eyes count is the known one and eyes_me is known");
  assert.equal(post(kept, 812).eyes_me, true, "the known eyes_me is echoed");

  const moved = fake(channel({ eyes: 3 }));
  const changed = await readTeamChannel(moved.call, CHANNEL, ME, NOW, input([known(OTHER_TS, 812, { eyes_me: false }), known(OWN_TS, 813, { author_is_me: true })]));
  assert.deepEqual(moved.reads("slack_get_reactions"), [OTHER_TS], "E3: the count changed");
  assert.equal(post(changed, 812).eyes_me, true, "the read answer replaces the known value");

  const unknownMe = fake(channel({ eyes: 2 }));
  await readTeamChannel(unknownMe.call, CHANNEL, ME, NOW, input([known(OTHER_TS, 812, { eyes_me: null }), known(OWN_TS, 813, { author_is_me: true })]));
  assert.deepEqual(unknownMe.reads("slack_get_reactions"), [OTHER_TS], "E3: eyes_me was never read");

  const uncached = fake(channel({ eyes: 2 }));
  await readTeamChannel(uncached.call, CHANNEL, ME, NOW, input([]));
  assert.deepEqual(uncached.reads("slack_get_reactions").sort(), [OTHER_TS, OWN_TS].sort(), "E3/T3: no known entry, so read");
});

test("slack_thread_read_only_when_reply_count_or_latest_reply_changed", async () => {
  const skipped = fake(channel());
  const base = [known(OTHER_TS, 812), known(OWN_TS, 813, { author_is_me: true })];
  const out = await readTeamChannel(skipped.call, CHANNEL, ME, NOW, input(base));
  assert.deepEqual(skipped.reads("slack_read_thread"), [], "T6: nothing changed and thread_tags_me is known");
  assert.equal(post(out, 812).thread_tags_me, false, "the known value is echoed");
  assert.equal(post(out, 812).reply_count, 3);

  const count = fake(channel({ replies: 4 }));
  const counted = await readTeamChannel(count.call, CHANNEL, ME, NOW, input(base));
  assert.deepEqual(count.reads("slack_read_thread"), [OTHER_TS], "T4: reply_count changed");
  assert.equal(post(counted, 812).thread_tags_me, true, "the thread read says the User is tagged");
  assert.equal(post(counted, 812).reply_count, 4);

  const latest = fake(channel({ latest: "2026-10-02 09:55:00 EDT" }));
  await readTeamChannel(latest.call, CHANNEL, ME, NOW, input(base));
  assert.deepEqual(latest.reads("slack_read_thread"), [OTHER_TS], "T4: latest_reply changed");

  const never = fake(channel());
  await readTeamChannel(never.call, CHANNEL, ME, NOW, input([known(OTHER_TS, 812, { thread_tags_me: null }), known(OWN_TS, 813, { author_is_me: true })]));
  assert.deepEqual(never.reads("slack_read_thread"), [OTHER_TS], "T5: thread_tags_me was never read");

  const fresh = fake(channel());
  await readTeamChannel(fresh.call, CHANNEL, ME, NOW, input([known(OWN_TS, 813, { author_is_me: true })]));
  assert.deepEqual(fresh.reads("slack_read_thread"), [OTHER_TS], "T3: no known entry for the message");
});

test("a malformed stdin, or one without version 1, is usage and reads nothing from Slack", async () => {
  for (const stdin of ["not json", "{}", JSON.stringify({ version: 2, known: [], terminal: [] })]) {
    const { value, exit } = await main(["--coding-agent", "/nonexistent/dist/index.js", "--channel", CHANNEL, "--me", ME], { PI_CODING_AGENT_DIR: "/nonexistent" }, "/", NOW, stdin);
    assert.equal(exit, 2, stdin);
    assert.equal((value as { error: { code: string } }).error.code, "usage", stdin);
  }
});
