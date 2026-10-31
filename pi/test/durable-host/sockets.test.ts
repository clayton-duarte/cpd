import assert from "node:assert/strict";
import { test } from "node:test";
import { dirVerdict, LineSplitter, staleVerdict } from "../../durable-host/sockets.ts";

test("LineSplitter returns the complete lines of a chunk and keeps the rest", () => {
  const s = new LineSplitter();
  assert.deepEqual(s.push("a\nb\nc"), { lines: ["a", "b"] });
  assert.deepEqual(s.push("\n"), { lines: ["c"] });
});

test("LineSplitter joins a line split over two chunks", () => {
  const s = new LineSplitter();
  assert.deepEqual(s.push(Buffer.from('{"type":"pi')), { lines: [] });
  assert.deepEqual(s.push(Buffer.from('ng"}\n')), { lines: ['{"type":"ping"}'] });
});

test("LineSplitter keeps a multi-byte character split over two chunks", () => {
  const s = new LineSplitter();
  const bytes = Buffer.from("a€b\n");
  assert.deepEqual(s.push(bytes.subarray(0, 2)), { lines: [] });
  assert.deepEqual(s.push(bytes.subarray(2)), { lines: ["a€b"] });
});

test("LineSplitter emits the pending text as a line as soon as it exceeds the limit", () => {
  const s = new LineSplitter(4);
  const out = s.push("abcdefg");
  assert.deepEqual(out.lines, []);
  assert.ok(out.overLimit !== undefined && out.overLimit.length >= 5 && "abcdefg".startsWith(out.overLimit), "overLimit holds at least limit + 1 bytes of the line");
  // the rest of the long line is discarded; the next line is intact
  assert.deepEqual(s.push("hij\nok\n"), { lines: ["ok"] });
});

test("dirVerdict accepts a directory of the User with mode 0700", () => {
  assert.equal(dirVerdict({ exists: true, isDir: true, uid: 1000, mode: 0o40700 }, 1000), "ok");
  assert.equal(dirVerdict({ exists: false }, 1000), "create");
});

test("dirVerdict asks to chmod a directory of the User with group or other bits", () => {
  for (const mode of [0o40755, 0o40770, 0o40707, 0o40500]) assert.equal(dirVerdict({ exists: true, isDir: true, uid: 1000, mode }, 1000), "chmod", mode.toString(8));
});

test("dirVerdict refuses a directory owned by another user", () => {
  assert.equal(dirVerdict({ exists: true, isDir: true, uid: 0, mode: 0o40700 }, 1000), "refuse");
  assert.equal(dirVerdict({ exists: true, isDir: false, uid: 1000, mode: 0o100600 }, 1000), "refuse");
});

test("staleVerdict removes the file when the connect is refused or the file is absent", () => {
  assert.equal(staleVerdict("absent"), "listen");
  assert.equal(staleVerdict("refused"), "remove");
  assert.equal(staleVerdict({ error: "EACCES" }), "fail");
});

test("staleVerdict refuses with host_running when the connect succeeds", () => {
  assert.equal(staleVerdict("connected"), "host_running");
});
