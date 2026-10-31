import assert from "node:assert/strict";
import { test } from "node:test";
import { registerHostTools, type RegisterHostToolsWhere } from "../../durable-host/tools.ts";
import type { Dialogs } from "../../durable-host/dialogs.ts";
import type { HeldApi } from "../../durable-host/held.ts";

// Contract §6.15 `tools.ts`: the four tools are tested through `registerHostTools` with a fake `cpd(args)` recorder, a
// fake held and fake dialogs. `registerHostTools` is the stub, so every test fails on 'not implemented'; the fakes
// record what the implemented tool would call, and the assertions pin pi's texts and shapes.

type ToolLike = { name: string; replay?: string; execute(a: unknown, api: unknown, ctx: unknown): Promise<{ content: { text: string }[]; details: unknown }> };

type Records = {
  cpd: string[][];
  asked: { questions: unknown; opts?: unknown }[];
  selected: { title: string; options: readonly string[]; opts?: unknown }[];
};
const noHeld: HeldApi = {
  create: () => ({ id: "id-1", promise: new Promise<object | undefined>(() => {}) }),
  answer: () => ({ outcome: "answered" }),
  resolveByHost: () => {},
  resolveAll: () => {},
  heldCount: () => 0,
  heldList: () => [],
};

const baseDialogs = (records: Records): Dialogs => ({
  select: async (title, options, opts) => {
    records.selected.push({ title, options, opts });
    return "Allow once";
  },
  confirm: async () => true,
  input: async () => "note",
  editor: async () => "feedback text",
  questionnaire: async (questions, opts) => {
    records.asked.push({ questions, opts });
    return { answers: [{ questionIndex: 0, question: "Pick one?", kind: "option", answer: "a" }], cancelled: false };
  },
  notice: () => {},
  status: () => {},
});

const makeWhere = (records: Records): RegisterHostToolsWhere => ({
  role: "lead",
  plan: "p1",
  name: "s-lead",
  cpd: async (args) => {
    records.cpd.push([...args]);
    return { ok: true };
  },
  held: noHeld,
  dialogs: baseDialogs(records),
});
const empty = (): Records => ({ cpd: [], asked: [], selected: [] });

const toolOf = (ext: { tools?: readonly { name: string }[] }, name: string): ToolLike => {
  const tool = ext.tools?.find((t) => t.name === name);
  assert.ok(tool, `tool ${name} registered`);
  return tool as unknown as ToolLike;
};
const run = async (tool: ToolLike, args: unknown) => {
  const r = await tool.execute(args, {}, {});
  return r;
};

test("ask_user_question validates its input and asks a questionnaire with the questions verbatim", async () => {
  const records = empty();
  const tool = toolOf(registerHostTools(makeWhere(records)), "ask_user_question");
  const questions = [
    { question: "Which library?", header: "Library", options: [{ label: "pi-durable", description: "host" }, { label: "cpd", description: "cli" }] },
  ];
  await run(tool, { questions });
  assert.equal(records.asked.length, 1);
  assert.deepEqual(records.asked[0].questions, questions);
  assert.equal((records.asked[0].opts as { purpose: string }).purpose, "ask_user_question");
});

test("ask_user_question is replay safe and returns the pi envelope text and details", async () => {
  const tool = toolOf(registerHostTools(makeWhere(empty())), "ask_user_question");
  assert.equal(tool.replay, "safe");
  const res = await run(tool, {
    questions: [{ question: "Which?", header: "Lib", options: [{ label: "a", description: "d" }, { label: "b", description: "e" }] }],
  });
  assert.equal(res.content[0].text, "User has answered your questions: \"Which?\"=\"a\". You can now continue with the user's answers in mind.");
  assert.deepEqual(res.details as { cancelled: boolean }, { cancelled: false });
});

test("a cancelled questionnaire returns the pi cancelled text to the model", async () => {
  const records = empty();
  const dialogs: Dialogs = {
    ...baseDialogs(records),
    questionnaire: async () => ({ answers: [], cancelled: true }),
  };
  const tool = toolOf(registerHostTools({ ...makeWhere(records), dialogs }), "ask_user_question");
  const res = await run(tool, {
    questions: [{ question: "Which?", header: "Lib", options: [{ label: "a", description: "d" }, { label: "b", description: "e" }] }],
  });
  assert.equal(res.content[0].text, "User declined to answer questions");
});

test("cpd_secret_approve refutes open vaults, non lead or manager roles and bad action with the pi texts", async () => {
  const basic = empty();
  const tool = toolOf(registerHostTools(makeWhere(basic)), "cpd_secret_approve");
  const text = async (args: unknown) => (await run(tool, args)).content[0].text;
  const open = "these vaults need no approval; use cpd secret get|list|create|edit|delete (commands.md §24)";
  assert.equal(await text({ action: "get", vault: "cpd", reason: "why" }), open);
  assert.equal(await text({ action: "get", vault: "homeserver", reason: "why" }), open);
  // A role that is not lead or manager: pi's refusal text. The type names only the two host roles, so the caller forces it.
  const other = empty();
  const otherWhere = makeWhere(other);
  const otherTool = toolOf(registerHostTools({ ...otherWhere, role: "other" as unknown as "manager" }), "cpd_secret_approve");
  assert.equal((await run(otherTool, { action: "get", vault: "prod", reason: "why" })).content[0].text, "the tool cpd_secret_approve is for Lead and Manager Sessions only");
  assert.equal(await text({ vault: "prod", reason: "why" }), "action must be one of get, list, create, edit, delete");
});

test("cpd_secret_approve asks select Allow once Deny with the pi title and runs cpd secret approve on Allow once", async () => {
  const records = empty();
  const tool = toolOf(registerHostTools(makeWhere(records)), "cpd_secret_approve");
  const res = await run(tool, { action: "get", vault: "prod", item: "i", field: "f", reason: "need it" });
  assert.equal(records.selected.length, 1);
  assert.equal(records.selected[0].title, "s-lead asks to get in vault prod: item i, field f. Reason: need it");
  assert.deepEqual([...records.selected[0].options], ["Allow once", "Deny"]);
  assert.deepEqual(records.cpd, [["secret", "approve", "--action", "get", "--vault", "prod", "--item", "i", "--field", "f"]]);
  assert.equal(res.content[0].text, "Approved for one command in the next 10 minutes: run it now.");
});

test("cpd_host_approve is manager only and runs cpd host approve with the pi title and texts", async () => {
  const records = empty();
  const tool = toolOf(registerHostTools({ ...makeWhere(records), role: "manager" }), "cpd_host_approve");
  const res = await run(tool, { reason: "main moves" });
  assert.equal(records.selected.length, 1);
  assert.equal(records.selected[0].title, "s-lead asks to repair the cpd checkout. Reason: main moves. Allow once: approve the repair. It discards tracked changes of ~/projects/cpd and resets main to origin/main after saving the diff and the old HEAD.");
  assert.deepEqual(records.cpd, [["host", "approve"]]);
  assert.equal(res.content[0].text, "Approved: run cpd host repair now.");
});

test("cpd_plan_review calls cpd plan review with the title and returns the next text", async () => {
  const records = empty();
  const tool = toolOf(registerHostTools(makeWhere(records)), "cpd_plan_review");
  assert.equal(tool.replay, "safe");
  const res = await run(tool, { title: "draft the plan" });
  assert.deepEqual(records.cpd, [["plan", "review", "--plan", "p1", "--title", "draft the plan"]]);
  assert.equal(res.content[0].text, "The User reviews the plan summary in the chat. Stop and wait for the decision message.");
});