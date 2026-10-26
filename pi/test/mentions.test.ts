import assert from "node:assert/strict";
import { test } from "node:test";
import { isMention, paintLine, paintMentions } from "../lib/mentions.ts";

/** commands.md §15.14 (hs-d544), contract 6.1.1 (pro-d38): a mention is italic and magenta `ESC[3m ESC[35m`…`ESC[39m ESC[23m`, a whole token, in a Lead's or Manager's pane only. */
const UUID = "3f2a9c1e-5b7d-4e8a-9c1d-0a1b2c3d4e5f";
const PREFIX = "3f2a9c1e";
const ids: ReadonlySet<string> = new Set(["pi-theme", "cpd2", UUID, PREFIX]);
const mag = (s: string) => `\x1b[3m\x1b[35m${s}\x1b[39m\x1b[23m`;

test("a Plan id in assistant text is magenta", () => {
  assert.equal(paintMentions("see pi-theme for details", ids), `see ${mag("pi-theme")} for details`);
  assert.equal(isMention("pi-theme", ids), true);
});

test("a Session UUID and its 8-character prefix are magenta", () => {
  assert.equal(paintMentions(`Lead ${UUID} and ${PREFIX}.`, ids), `Lead ${mag(UUID)} and ${mag(PREFIX)}.`);
});

test("a Context id is magenta", () => {
  assert.equal(paintMentions("context cpd2: done", ids), `context ${mag("cpd2")}: done`);
});

test("only whole tokens are mentions", () => {
  assert.equal(paintMentions("cpd2-x and xcpd2 and pi-theme-tests", ids), "cpd2-x and xcpd2 and pi-theme-tests");
  assert.equal(paintMentions("(cpd2)", ids), `(${mag("cpd2")})`);
  assert.equal(isMention("cpd2-x", ids), false);
});

test("an unknown id is not a mention", () => {
  assert.equal(paintMentions("see other-plan", ids), "see other-plan");
  assert.equal(isMention("other-plan", ids), false);
  assert.equal(paintMentions("see other-plan and cpd2", ids), `see other-plan and ${mag("cpd2")}`);
});

test("a Worker pane draws no mentions", () => {
  assert.equal(paintMentions("see pi-theme", ids, "worker"), "see pi-theme");
  assert.equal(paintMentions("see pi-theme", ids, "manager"), `see ${mag("pi-theme")}`);
});

test("a mention is italic as well as magenta", () => {
  assert.equal(paintMentions("see pi-theme", ids), "see \x1b[3m\x1b[35mpi-theme\x1b[39m\x1b[23m");
});

test("a mention in a rendered line is italic as well as magenta, outside code", () => {
  const codeOpen = "\x1b[38;5;3m";
  const line = `see pi-theme ${codeOpen}pi-theme\x1b[39m`;
  assert.equal(paintLine(line, ids, [codeOpen]), `see \x1b[3m\x1b[35mpi-theme\x1b[39m\x1b[23m ${codeOpen}pi-theme\x1b[39m`);
});
