import assert from "node:assert/strict";
import { test } from "node:test";
import { clickable, foreground, type MeaningName, meaning, parseSpec, pill, spec } from "../lib/pill.ts";

const CAP_L = "\u{E0B6}";
const CAP_R = "\u{E0B4}";
const MEANINGS = { plain: 7, in_progress: 6, review: 3, done: 2, failed: 1, planning: 5, nothing: 8, focus: 4 };
const RAW = () => ({
  caps: { left: CAP_L, right: CAP_R },
  clickable: { add: 8, from: 1, to: 7 },
  meanings: { ...MEANINGS },
});

test("the shipped spec has the caps, meanings and clickable rule of the file", () => {
  assert.deepEqual(spec, RAW());
  for (const [name, index] of Object.entries(MEANINGS)) assert.equal(meaning(name as MeaningName), index);
});

test("parseSpec rejects an unknown key naming it", () => {
  assert.deepEqual(parseSpec(RAW()), RAW());
  assert.throws(() => parseSpec({ ...RAW(), bogus: 1 }), /bogus/);
  assert.throws(() => parseSpec({ ...RAW(), meanings: { ...MEANINGS, sparkle: 2 } }), /sparkle/);
});

test("parseSpec rejects a missing key naming it", () => {
  const { caps: _c, ...noCaps } = RAW();
  assert.throws(() => parseSpec(noCaps), /caps/);
  const { focus: _f, ...rest } = MEANINGS;
  assert.throws(() => parseSpec({ ...RAW(), meanings: rest }), /focus/);
});

test("parseSpec rejects index 0 and an index above 15", () => {
  for (const bad of [0, 16, 1.5, "3"]) {
    assert.throws(() => parseSpec({ ...RAW(), meanings: { ...MEANINGS, done: bad } }), /done/);
  }
  assert.doesNotThrow(() => parseSpec({ ...RAW(), meanings: { ...MEANINGS, done: 15 } }));
  assert.doesNotThrow(() => parseSpec({ ...RAW(), meanings: { ...MEANINGS, done: 1 } }));
});

test("clickable adds 8 to 1 through 7 and keeps the rest", () => {
  for (let i = 1; i <= 7; i++) assert.equal(clickable(i), i + 8);
  for (let i = 8; i <= 15; i++) assert.equal(clickable(i), i);
});

test("foreground prints 3N for 1-7 and 9N for 8-15", () => {
  assert.equal(foreground(12), "\x1b[94m");
  assert.equal(foreground(7), "\x1b[37m");
  for (let i = 1; i <= 7; i++) assert.equal(foreground(i), `\x1b[3${i}m`);
  for (let i = 8; i <= 15; i++) assert.equal(foreground(i), `\x1b[9${i - 8}m`);
});

function decode(s: string): { index: number; left: string; text: string; right: string } {
  const m = /^\x1b\[(3|9)(\d)m(.)\x1b\[7m(.*)\x1b\[27m\x1b\[\1\2m(.)\x1b\[39m$/su.exec(s);
  assert.ok(m, JSON.stringify(s));
  return { index: Number(m[2]) + (m[1] === "9" ? 8 : 0), left: m[3]!, text: m[4]!, right: m[5]! };
}

test("pill is a coloured cap, reversed text and a coloured cap for every meaning", () => {
  assert.equal(pill(2, "ok"), `\x1b[32m${CAP_L}\x1b[7mok\x1b[27m\x1b[32m${CAP_R}\x1b[39m`);
  const indices = Object.values(spec.meanings).flatMap((i) => [i, clickable(i)]);
  for (const index of indices) {
    for (const text of ["two words", "x", "wait…"]) {
      const d = decode(pill(index, text));
      assert.equal(d.index, index);
      assert.equal(d.left, spec.caps.left);
      assert.equal(d.right, spec.caps.right);
      assert.equal(d.text, text);
    }
  }
});

test("pill rejects an index outside 1-15", () => {
  for (const bad of [0, 16, -1, 1.5]) assert.throws(() => pill(bad, "x"));
  assert.doesNotThrow(() => pill(1, "x"));
  assert.doesNotThrow(() => pill(15, "x"));
});
