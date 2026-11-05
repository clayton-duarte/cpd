import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMessages } from "../lib/slack-parse.ts";

const transcript =
  "=== Message from Clayton Duarte <clayton.duarte@missionlane.com> (U04DB4MTUMR) at 2026-09-30 10:05:28 EDT === \n" +
  "Message TS: 1790777128.873339\n" +
  "<@U01HPPJ6DQT|Wes Hirsheimer> https://github.com/missionlane/card-dashboard/pull/3537\n\n" +
  "=== Message from Angel Rodriguez <angel@missionlane.com> (U01CF85EF8B) at 2026-10-01 10:56:53 EDT === \n" +
  "Message TS: 1790866613.749329\n" +
  "https://github.com/missionlane/card-dashboard/pull/3538\n";

test("parseMessages keeps the author's Slack id from the message header, not a mentioned user's", () => {
  const posts = parseMessages(transcript, 0, "U04DB4MTUMR");
  const byNumber = new Map(posts.map((p) => [p.number, p]));
  assert.equal(byNumber.get(3537)?.author_id, "U04DB4MTUMR");
  assert.equal(byNumber.get(3537)?.author, "Clayton Duarte");
  assert.equal(byNumber.get(3538)?.author_id, "U01CF85EF8B");
});
