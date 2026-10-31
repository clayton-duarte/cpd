import assert from "node:assert/strict";
import { test } from "node:test";
import { createDialogs, type DialogPort } from "../../durable-host/dialogs.ts";
import type { CreateOptions, Held } from "../../durable-host/held.ts";

// Contract §6.15 `dialogs.ts`: pure tests over a fake `DialogPort` (a fake held map, broadcast and stateChanged), no
// socket, no timer. Each test drives the stub through `createDialogs`, which throws 'not implemented'.

type Created = {
  method: Held["method"];
  purpose: Held["purpose"];
  fields: object;
  opts: CreateOptions;
  id: string;
  promise: Promise<object | undefined>;
  resolve(v?: object): void;
};

const makePort = () => {
  const created: Created[] = [];
  const broadcasted: Record<string, unknown>[] = [];
  const stateChanges: number[] = [];
  let n = 0;
  const port: DialogPort = {
    held: {
      create(method, fields, purpose, opts) {
        n++;
        let resolve!: (v?: object) => void;
        const promise = new Promise<object | undefined>((r) => {
          resolve = r;
        });
        created.push({ method, purpose, fields, opts, id: `id-${n}`, promise, resolve });
        return { id: `id-${n}`, promise };
      },
      answer: () => ({ outcome: "answered" }),
      resolveByHost: () => {},
      resolveAll: () => {},
      heldCount: () => created.length,
      heldList: () =>
        created.map((c) => ({
          id: c.id,
          method: c.method,
          purpose: c.purpose,
          created_at: 0,
          fields: c.fields,
          resolve: c.resolve,
          reject: () => {},
        })),
    },
    broadcast: (m) => {
      broadcasted.push(m);
    },
    stateChanged: () => {
      stateChanges.push(1);
    },
  };
  return { created, broadcasted, stateChanges, port };
};

const tick = () => new Promise<void>((r) => setImmediate(r));

test("select creates one held request and resolves with the chosen value", async () => {
  const { created, port } = makePort();
  const dialogs = createDialogs(port);
  const p = dialogs.select("Pick", ["Allow once", "Deny"], { purpose: "secret_approve" });
  assert.equal(created.length, 1);
  assert.equal(created[0].method, "select");
  assert.equal(created[0].purpose, "secret_approve");
  assert.deepEqual(created[0].fields, { title: "Pick", options: ["Allow once", "Deny"] });
  created[0].resolve("Allow once" as unknown as object);
  assert.equal(await p, "Allow once");
});

test("a cancelled select resolves the request as cancelled and returns undefined", async () => {
  const { created, port } = makePort();
  const dialogs = createDialogs(port);
  const p = dialogs.select("Pick", ["Allow once", "Deny"]);
  created[0].resolve(undefined);
  assert.equal(await p, undefined);
  assert.equal(created[0].purpose, "other");
});

test("confirm returns false on cancel and the boolean on confirm", async () => {
  const { created, port } = makePort();
  const dialogs = createDialogs(port);
  const confirmed = dialogs.confirm("Sure?", "message", { purpose: "other" });
  const declined = dialogs.confirm("Sure?", "message", { purpose: "other" });
  created[0].resolve(true as unknown as object);
  created[1].resolve(undefined);
  assert.equal(await confirmed, true);
  assert.equal(await declined, false);
});

test("questionnaire returns the answers object on answer and cancelled with answers empty on cancel", async () => {
  const { created, port } = makePort();
  const dialogs = createDialogs(port);
  const questions = [{ question: "Pick one?", header: "Choice", options: [{ label: "a", description: "d" }, { label: "b", description: "e" }] }];
  const p = dialogs.questionnaire(questions, { purpose: "ask_user_question" });
  created[0].resolve({ answers: [{ questionIndex: 0, question: "Pick one?", kind: "option", answer: "a" }], cancelled: false });
  const r = await p;
  assert.equal(r.cancelled, false);
  assert.equal(r.answers.length, 1);
  const c = makePort();
  const pc = createDialogs(c.port);
  const q = pc.questionnaire(questions);
  c.created[0].resolve(undefined);
  const rc = await q;
  assert.deepEqual(rc, { answers: [], cancelled: true });
});

test("notice and status broadcast their messages and create no held request", () => {
  const { created, broadcasted, port } = makePort();
  const dialogs = createDialogs(port);
  dialogs.notice("warning", "careful");
  dialogs.status("cpd", "working");
  dialogs.status("cpd", null);
  assert.deepEqual(broadcasted, [
    { type: "notice", level: "warning", message: "careful" },
    { type: "status", key: "cpd", text: "working" },
    { type: "status", key: "cpd", text: null },
  ]);
  assert.equal(created.length, 0);
});

test("every dialog broadcast happens once and changes the state via stateChanged", async () => {
  const { created, broadcasted, stateChanges, port } = makePort();
  const dialogs = createDialogs(port);
  void dialogs.select("a", ["x", "y"], { purpose: "other" });
  void dialogs.confirm("b", "m", { purpose: "other" });
  void dialogs.editor("c", "", { purpose: "other" });
  const prev = created.length;
  assert.equal(prev, 3);
  assert.equal(broadcasted.filter((m) => m.type === "ui_request").length, 3);
  assert.equal(stateChanges.length, 3);
  created[0].resolve("x" as unknown as object);
  created[1].resolve(true as unknown as object);
  await tick();
});