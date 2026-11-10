import assert from "node:assert/strict";
import { test } from "node:test";
import { proseTime, zoneLine, zoneTime } from "../lib/time.ts";

const TO = "America/Toronto";
const KOL = "Asia/Kolkata";

test("proseTime names the zone abbreviation", () => {
  assert.equal(proseTime("2026-10-25T22:29:00.000Z", TO), "2026-10-25 18:29 EDT");
  assert.equal(proseTime("2026-01-15T15:00:00.000Z", TO), "2026-01-15 10:00 EST");
  assert.equal(proseTime("2026-10-26T03:03:00.000Z", TO), "2026-10-25 23:03 EDT");
});

test("proseTime uses the numeric offset when the zone has no letters", () => {
  assert.equal(proseTime("2026-10-25T22:29:00.000Z", KOL), "2026-10-26 03:59 +05:30");
});

test("zoneTime is the time of day and the zone", () => {
  assert.equal(zoneTime("2026-10-25T22:29:00.000Z", TO), "18:29 EDT");
  assert.equal(zoneTime("2026-10-25T22:29:00.000Z", KOL), "03:59 +05:30");
});

test("zoneLine names the IANA zone, its label and an example", () => {
  const tail = (ex: string) => `State every time to a person in this zone with the zone named, as ${ex}, never in UTC.`;
  assert.equal(zoneLine(new Date("2026-10-26T03:03:00.000Z"), TO), `Time zone: America/Toronto (EDT). ${tail("2026-10-25 23:03 EDT")}`);
  assert.equal(zoneLine(new Date("2026-01-15T15:00:00.000Z"), TO), `Time zone: America/Toronto (EST). ${tail("2026-01-15 10:00 EST")}`);
  assert.equal(zoneLine(new Date("2026-10-25T22:29:00.000Z"), KOL), `Time zone: Asia/Kolkata (+05:30). ${tail("2026-10-26 03:59 +05:30")}`);
});

test("zoneLine produces byte-stable prompt output regardless of current time", () => {
  const d1 = new Date("2026-10-25T22:29:00.000Z");
  const d2 = new Date("2026-10-25T22:31:00.000Z");
  assert.equal(zoneLine(d1, TO), zoneLine(d2, TO));
  assert.equal(zoneLine(d1, KOL), zoneLine(d2, KOL));
  assert.equal(zoneLine(d1), zoneLine(d2));
});

