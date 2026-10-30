/** hs-d738, hs-d739: the adapter's refusal text, tool description and skills hold no plannotator. */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { LIMIT, tooLong } from "../lib/askguard.ts";
import { limitSentence, fieldNote } from "../ask-user-question/limit-text.ts";

const read = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");

test("drop-plannotator the limit refusal names a file outside the repo and no plannotator", () => {
	const n = LIMIT + 1;
	const reason = tooLong({ questions: [{ question: "x".repeat(n), header: "", options: [] }] });
	assert.ok(reason !== undefined, "a question over the limit is refused");
	assert.ok(
		reason.includes(`ask_user_question is for a few short choices; write anything larger to a file outside the repo and point to it in chat; refused: ${n} characters (limit ${LIMIT})`),
		reason,
	);
	assert.doesNotMatch(reason, /plannotator/i);
});

test("drop-plannotator the tool description limit sentence names a file outside the repo and no plannotator", () => {
	assert.ok(limitSentence.endsWith("Put larger content in a file outside the repo and point to it in chat."), limitSentence);
	assert.doesNotMatch(limitSentence, /plannotator/i);
	assert.doesNotMatch(fieldNote, /plannotator/i);
});

test("drop-plannotator the lead and manager skills and their question rules name no plannotator", () => {
	for (const p of ["skills/cpd-lead/SKILL.md", "skills/cpd-manager/SKILL.md", "skills/cpd-lead/references/questions.md", "package.json"]) {
		const text = read(p);
		assert.doesNotMatch(text, /plannotator/i, p);
		assert.doesNotMatch(text, /Open full plan/, p);
	}
});
