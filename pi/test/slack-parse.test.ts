// Transcript shapes are taken from hermes-config `plugins/cockpit/tests/` (hermes-config @ 57b7dbe): `test_classify.py`
// (the automation footer, `Reactions:`, co-tagged mentions, claims) and `test_backfill_slack_ts.py` (`FIXTURE_TRANSCRIPT`).
import assert from "node:assert/strict";
import { test } from "node:test";
import { claimedByMe, countMessages, extractPrs, mentionsMe, parseMessages, parseReactionCounts, parseReactions, parseThread, reactionUsers, slackClaim, unescapeTranscript, unwrap } from "../lib/slack-parse.ts";

const ME = "U04DB4MTUMR";

/** test_classify.py `test_automation_post_with_no_human_tagged_is_not_claimed`. */
const automation =
  "Michael Lancaster <michael.lancaster@missionlane.com> (U04EKRHPLHF) at 2026-09-08 15:52:16 EDT === \n" +
  "Message TS: 1788897136.047209\n" +
  "Refresh home queries after settlement enrollment\n\n" +
  ":git-pr-open: <https://github.com/missionlane/card-dashboard/pull/3531|github.com/missionlane/card-dashboard/pull/3531>\n" +
  "*Sent using* <@U0AT379PE6A|Claude>\n" +
  "Thread: 1 replies (latest: 2026-09-08 16:54:34 EDT)\n";

const claim = (block: string) => slackClaim(block, ME);

test("an automation footer is the app naming itself, not a person asked (by shape, not by id)", () => {
  assert.equal(claim(automation), null);
  assert.equal(claim(automation.replace("U0AT379PE6A|Claude", "U09ZZZZZZZZ|Some Other Bot")), null);
});

test("the footer excuses only its own mention", () => {
  const block = automation.replace("Refresh home", "<@U04EKRHPLHF|Michael Lancaster> can you take first pass?\nRefresh home");
  assert.deepEqual(claim(block), { kind: "claimed", reason: "assigned to someone in Slack" });
});

test("a mention of the User withdraws the claim, even beside a teammate", () => {
  const block = "Michael <m@x.com> (U04EKRHPLHF) at t === \nMessage TS: 1789079219.182249\n<@U01HPPJ6DQT|Wes Hirsheimer> <@U04DB4MTUMR|Clayton Duarte> <https://github.com/missionlane/card-dashboard/pull/3537|x> before I rebase\n";
  assert.equal(claim(block), null);
});

test("a mention of only teammates claims; a hand-typed mention with no footer too", () => {
  const block = "Chris <c@x.com> (U04CLNY2MCY) at t === \nMessage TS: 1788897559.716789\n<@U04EKRHPLHF|Michael Lancaster> - may need to drag and drop\n";
  assert.deepEqual(claim(block), { kind: "claimed", reason: "assigned to someone in Slack" });
});

test("a reaction eyes claims whatever the footer says; a merged reaction is not state", () => {
  assert.deepEqual(claim(automation + "Reactions: eyes (1)\n"), { kind: "claimed", reason: "claimed with :eyes: in Slack" });
  assert.equal(claim(automation + "Reactions: git-merged (1)\n"), null);
});

test("parseReactions reads the names without counts", () => {
  assert.deepEqual(parseReactions("x\nReactions: eyes (1), github-check-mark (2)\n"), ["eyes", "github-check-mark"]);
  assert.deepEqual(parseReactions("no reactions here"), []);
});

test("extractPrs finds every URL of the Slack link wrapper and strips trailing punctuation from the repo", () => {
  assert.deepEqual(extractPrs("<https://github.com/acme/pay/pull/812|github.com/acme/pay/pull/812> and github.com/acme/ui./pull/9"), [
    ["acme/pay", 812],
    ["acme/pay", 812],
    ["acme/ui", 9],
  ]);
});

test("mentionsMe: the thread rule ignores the app's footer", () => {
  assert.equal(mentionsMe("From: Michael (U04EKRHPLHF)\n<@U04DB4MTUMR|Clayton> look?", ME), true);
  assert.equal(mentionsMe("<@U01HPPJ6DQT|Wes> can you take this one?", ME), false);
  assert.equal(mentionsMe("done\n*Sent using* <@U04DB4MTUMR|Me>\n", ME), false);
});

/** test_backfill_slack_ts.py `FIXTURE_TRANSCRIPT`, a bump and a stacked post added. */
const transcript =
  "=== Message from Clayton Duarte <clayton.duarte@missionlane.com> (U04DB4MTUMR) at 2026-09-30 10:05:28 EDT === \n" +
  "Message TS: 1790777128.873339\n" +
  ":git-pr-open: fix: something\n" +
  "<https://github.com/missionlane/card-dashboard/pull/3627|github.com/missionlane/card-dashboard/pull/3627>\n" +
  "*Sent using* <@U0AT379PE6A|Claude>\n\n" +
  "=== Message from Angel Rodriguez <angel@missionlane.com> (U01CF85EF8B) at 2026-10-01 10:56:53 EDT === \n" +
  "Message TS: 1790866613.749329\n" +
  ":git-pr-open: feat: stacked <https://github.com/missionlane/card-dashboard/pull/3653|x> on https://github.com/missionlane/card-dashboard/pull/3627\n" +
  "Reactions: eyes (1)\n\n" +
  "=== Message from Wes Hirsheimer <wes@missionlane.com> (U01HPPJ6DQT) at 2026-10-02 09:00:00 EDT === \n" +
  "Message TS: 1790950000.000100\n" +
  "bump <https://github.com/missionlane/card-dashboard/pull/3627|x>\n\n" +
  "=== Message from Nobody (U0) at t === \nno timestamp https://github.com/a/b/pull/1\n\n" +
  "=== Message from Old (U1) at t === \nMessage TS: 1700000000.000000\nhttps://github.com/a/b/pull/2\n";

test("parseMessages: window, one row per PR, the newest posting wins, the primary is the first URL, newest first", () => {
  const posts = parseMessages(transcript, 1790000000, ME);
  assert.deepEqual(
    posts.map((p) => [p.repo, p.number, p.ts_raw, p.author, p.primary]),
    [
      ["missionlane/card-dashboard", 3627, "1790950000.000100", "Wes Hirsheimer", true], // the bump wins
      ["missionlane/card-dashboard", 3653, "1790866613.749329", "Angel Rodriguez", true],
    ],
  );
  const stacked = parseMessages(transcript.replace(/=== Message from Wes[^]*?\n\n/, ""), 1790000000, ME);
  const by = (n: number) => stacked.find((p) => p.number === n)!;
  assert.deepEqual([by(3653).primary, by(3627).primary], [true, false], "the post is about 3653; 3627 is only mentioned");
  assert.equal(by(3653).ts_raw, by(3627).ts_raw, "both share the message's ts");
  assert.deepEqual(by(3653).reactions, { eyes: 1 });
  assert.deepEqual(by(3653).claim, { kind: "claimed", reason: "claimed with :eyes: in Slack" });
  assert.equal(by(3653).eyes_me, null);
  assert.equal(by(3653).thread_tags_me, null);
});

test("a bump by someone with no reaction re-opens a PR an older :eyes: had claimed (the winning post's claim rides along)", () => {
  const posts = parseMessages(transcript, 1790000000, ME);
  assert.equal(posts[0]!.claim, null);
  assert.deepEqual(posts[0]!.reactions, {});
});

test("ts_raw is Slack's string, kept exact", () => {
  assert.equal(parseMessages(transcript, 0, ME).find((p) => p.number === 2)?.ts_raw, "1700000000.000000");
});

test("countMessages counts the blocks", () => assert.equal(countMessages(transcript), 5));

// refresh.unwrap: the envelope arrives in several shapes and is sometimes truncated mid-JSON.
test("unwrap: a plain transcript, a {result} envelope, a doubly wrapped one and a {messages} one", () => {
  assert.equal(unwrap(transcript), unescapeTranscript(transcript));
  assert.equal(unwrap(JSON.stringify({ result: transcript })), transcript);
  assert.equal(unwrap(JSON.stringify({ result: JSON.stringify({ messages: transcript }) })), transcript);
  assert.equal(unwrap(JSON.stringify({ messages: transcript })), transcript);
});

test("unwrap: a truncated envelope is unescaped, so the headers are on their own lines and the URLs match", () => {
  const cut = JSON.stringify({ result: transcript }).slice(0, -40);
  const text = unwrap(cut);
  assert.ok(countMessages(text) >= 4, text.slice(0, 200));
  assert.deepEqual(extractPrs(text)[0], ["missionlane/card-dashboard", 3627], "no escaped slash left in the URL");
});

test("unwrap: a payload with no message is none, so a caller can refuse it", () => {
  assert.equal(countMessages(unwrap("I could not read the channel.")), 0);
  assert.equal(countMessages(unwrap(JSON.stringify({ result: "Raw response above, delivered as returned." }))), 0);
  assert.equal(countMessages(unwrap("[1,2]")), 0);
});

test("reactionUsers reads :eyes: by Slack user id, through the envelope, and claimedByMe judges them", () => {
  const raw = JSON.stringify({ result: ":eyes: \u00d7 2 \u2014 Ryan Case (U039194B7D2), clayton (U04DB4MTUMR)\n:+1: \u00d7 1 \u2014 Wes (U01HPPJ6DQT)" });
  const users = reactionUsers(raw, "eyes");
  assert.deepEqual(users, [{ name: "Ryan Case", id: "U039194B7D2" }, { name: "clayton", id: "U04DB4MTUMR" }]);
  assert.equal(claimedByMe(users, ME), false, "a peer's claim wins even when the User also reacted");
  assert.equal(claimedByMe([{ id: ME }], ME), true);
  assert.equal(claimedByMe([], ME), null, "unattributable");
  assert.deepEqual(reactionUsers(":eyes: \u00d7 1 \u2014 Wes (U01HPPJ6DQT)", "eyes"), [{ name: "Wes", id: "U01HPPJ6DQT" }], "raw text without an envelope");
  assert.deepEqual(reactionUsers("nothing", "eyes"), []);
});

test("parseReactionCounts reads the counts of the Reactions line", () => {
  assert.deepEqual(parseReactionCounts("x\nReactions: eyes (2), github-check-mark (1)\n"), { eyes: 2, "github-check-mark": 1 });
  assert.deepEqual(parseReactionCounts("no reactions here"), {});
});

test("parseThread reads reply_count and the latest reply verbatim (a local-zone string); no line is no thread", () => {
  assert.deepEqual(parseThread(automation), { reply_count: 1, latest_reply: "2026-09-08 16:54:34 EDT" });
  assert.deepEqual(parseThread("Message TS: 1.1\nplain\n"), { reply_count: null, latest_reply: null });
});
