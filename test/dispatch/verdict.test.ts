import { test } from "node:test";
import assert from "node:assert/strict";
import { parseVerdict } from "../../src/dispatch/verdict.ts";

const block = (json: string) => "```json\n" + json + "\n```";
const approve = block('{"verdict":"approve","feedback":"all items met"}');
const reject = block('{"verdict":"reject","feedback":"missing test for the 400 case"}');

type Row = { name: string; text: string; want: { verdict: "approve" | "reject"; feedback: string } | "invalid" };

const rows: Row[] = [
  { name: "valid approve", text: `Reviewed everything.\n\n${approve}`, want: { verdict: "approve", feedback: "all items met" } },
  { name: "valid reject", text: reject, want: { verdict: "reject", feedback: "missing test for the 400 case" } },
  { name: "missing block", text: "Looks good to me, approve.", want: "invalid" },
  { name: "empty message", text: "", want: "invalid" },
  { name: "malformed JSON", text: block('{"verdict":"approve","feedback":"x",}'), want: "invalid" },
  { name: "extra text after the block is ignored (the verdict is unambiguous)", text: `${approve}\n\nLet me know if you need more.`, want: { verdict: "approve", feedback: "all items met" } },
  { name: "the last block wins when there are several", text: `${reject}\n\nOn second thought:\n\n${approve}`, want: { verdict: "approve", feedback: "all items met" } },
  { name: "an earlier non-verdict json block does not matter", text: `The config is\n${block('{"a":1}')}\nVerdict:\n${reject}`, want: { verdict: "reject", feedback: "missing test for the 400 case" } },
  { name: "a trailing non-verdict json block makes it invalid (the last block decides)", text: `${approve}\n\nFor reference:\n${block('{"a":1}')}`, want: "invalid" },
  { name: "unknown verdict value", text: block('{"verdict":"maybe","feedback":"x"}'), want: "invalid" },
  { name: "missing feedback", text: block('{"verdict":"approve"}'), want: "invalid" },
  { name: "blank feedback", text: block('{"verdict":"approve","feedback":"   "}'), want: "invalid" },
  { name: "extra keys are refused (strict schema)", text: block('{"verdict":"approve","feedback":"x","score":9}'), want: "invalid" },
  { name: "a fence that is not json is not a verdict", text: "```js\n" + '{"verdict":"approve","feedback":"x"}' + "\n```", want: "invalid" },
  { name: "an unclosed fence is not a block", text: '```json\n{"verdict":"approve","feedback":"x"}\n', want: "invalid" },
  { name: "CRLF line endings", text: '```json\r\n{"verdict":"approve","feedback":"x"}\r\n```', want: { verdict: "approve", feedback: "x" } },
  { name: "a JSON array is not a verdict", text: block('[{"verdict":"approve","feedback":"x"}]'), want: "invalid" },
];

for (const row of rows) {
  test(`parseVerdict: ${row.name}`, () => {
    const got = parseVerdict(row.text);
    if (row.want === "invalid") {
      assert.equal(got.verdict, "invalid");
      assert.ok("reason" in got && got.reason.length > 0, "an invalid verdict says why");
    } else {
      assert.deepEqual(got, row.want);
    }
  });
}
