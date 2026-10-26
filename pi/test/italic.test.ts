import assert from "node:assert/strict";
import { test } from "node:test";
import { italic } from "../lib/italic.ts";

test("italic wraps the text in SGR 3 and SGR 23", () => {
  assert.equal(italic("x"), "\x1b[3mx\x1b[23m");
});

test("italic leaves the colour inside it untouched", () => {
  assert.equal(italic("\x1b[36mx\x1b[39m"), "\x1b[3m\x1b[36mx\x1b[39m\x1b[23m");
});
