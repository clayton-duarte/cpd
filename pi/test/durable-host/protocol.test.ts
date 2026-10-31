import assert from "node:assert/strict";
import { test } from "node:test";
import { encode, LIMITS, negotiate, parseLine } from "../../durable-host/protocol.ts";

const fail = (line: string) => {
  const r = parseLine(line);
  assert.equal(r.ok, false);
  return (r as { ok: false; error: { code: string; message: string; req?: string } }).error;
};

test("parseLine accepts a hello and returns its fields", () => {
  assert.deepEqual(parseLine('{"type":"hello","protocol":1,"min_protocol":1,"client":"cpd-chat","name":"me"}'), {
    ok: true,
    msg: { type: "hello", protocol: 1, min_protocol: 1, client: "cpd-chat", name: "me" },
  });
  assert.deepEqual(parseLine('{"type":"hello","protocol":2,"min_protocol":1,"client":"x"}'), {
    ok: true,
    msg: { type: "hello", protocol: 2, min_protocol: 1, client: "x" },
  });
});

test("parseLine accepts attach with and without a cursor", () => {
  assert.deepEqual(parseLine('{"type":"attach","req":"r1"}'), { ok: true, msg: { type: "attach", req: "r1" } });
  assert.deepEqual(parseLine('{"type":"attach","req":"r2","cursor":{"epoch":"e1","seq":7}}'), {
    ok: true,
    msg: { type: "attach", req: "r2", cursor: { epoch: "e1", seq: 7 } },
  });
});

test("parseLine defaults when_busy to followUp and refuses an unknown value with bad_args", () => {
  assert.deepEqual(parseLine('{"type":"submit","req":"r","text":"hi"}'), {
    ok: true,
    msg: { type: "submit", req: "r", text: "hi", when_busy: "followUp" },
  });
  for (const v of ["followUp", "steer", "reject"]) {
    assert.deepEqual(parseLine(`{"type":"submit","req":"r","text":"hi","when_busy":"${v}","request_id":"q"}`), {
      ok: true,
      msg: { type: "submit", req: "r", text: "hi", when_busy: v, request_id: "q" },
    });
  }
  const e = fail('{"type":"submit","req":"r","text":"hi","when_busy":"later"}');
  assert.equal(e.code, "bad_args");
  assert.equal(e.req, "r");
  assert.match(e.message, /when_busy/);
  assert.deepEqual(parseLine('{"type":"abort","req":"r"}'), { ok: true, msg: { type: "abort", req: "r", background: false } });
  assert.deepEqual(parseLine('{"type":"abort","req":"r","background":true}'), { ok: true, msg: { type: "abort", req: "r", background: true } });
});

test("parseLine requires req on attach, submit, abort, abort_submission and detach", () => {
  const lines = [
    '{"type":"attach"}',
    '{"type":"submit","text":"hi"}',
    '{"type":"abort"}',
    '{"type":"abort_submission","submission_id":3}',
    '{"type":"detach"}',
    '{"type":"detach","req":5}',
  ];
  for (const line of lines) {
    const e = fail(line);
    assert.equal(e.code, "bad_args", line);
    assert.match(e.message, /req/, line);
    assert.equal(e.req, undefined, line);
  }
  assert.deepEqual(parseLine('{"type":"abort_submission","req":"r","submission_id":3}'), {
    ok: true,
    msg: { type: "abort_submission", req: "r", submission_id: 3 },
  });
  assert.deepEqual(parseLine('{"type":"detach","req":"r"}'), { ok: true, msg: { type: "detach", req: "r" } });
});

test("parseLine accepts ping and hello without req", () => {
  assert.deepEqual(parseLine('{"type":"ping"}'), { ok: true, msg: { type: "ping" } });
  assert.deepEqual(parseLine('{"type":"ping","req":"p"}'), { ok: true, msg: { type: "ping", req: "p" } });
  assert.equal(parseLine('{"type":"hello","protocol":1,"min_protocol":1,"client":"c"}').ok, true);
});

test("parseLine ignores unknown fields", () => {
  assert.deepEqual(parseLine('{"type":"ping","req":"p","extra":1}'), { ok: true, msg: { type: "ping", req: "p" } });
  assert.deepEqual(parseLine('{"type":"detach","req":"r","zzz":{"a":1}}'), { ok: true, msg: { type: "detach", req: "r" } });
});

test("parseLine refuses a line that is not a JSON object with bad_frame", () => {
  for (const line of ["", "not json", "[1]", "42", '"s"', "null", "{"]) {
    const r = parseLine(line);
    assert.equal(r.ok, false, line);
    assert.equal((r as { ok: false; error: { code: string } }).error.code, "bad_frame", line);
  }
});

test("parseLine refuses an object without a string type with bad_frame", () => {
  for (const line of ["{}", '{"type":1}', '{"type":null}', '{"req":"r"}']) {
    assert.equal(fail(line).code, "bad_frame", line);
  }
});

test("parseLine answers an unknown type with unknown_type and keeps its req", () => {
  const e = fail('{"type":"frobnicate","req":"r9"}');
  assert.equal(e.code, "unknown_type");
  assert.equal(e.req, "r9");
  const n = fail('{"type":"frobnicate"}');
  assert.equal(n.code, "unknown_type");
  assert.equal(n.req, undefined);
  assert.equal(fail('{"type":"frobnicate","req":5}').req, undefined);
});

test("parseLine answers a type of a later PR with unknown_type", () => {
  for (const type of ["compact", "configure", "list_models", "reload", "mcp", "fork", "history", "rename"]) {
    const e = fail(`{"type":"${type}","req":"r"}`);
    assert.equal(e.code, "unknown_type", type);
    assert.equal(e.req, "r", type);
  }
});

test("parseLine accepts ui_response with value and rejects none, two or unknown shapes with bad_args", () => {
  assert.deepEqual(parseLine('{"type":"ui_response","req":"r13","id":"b3b2e6f0","value":"Allow once"}'), {
    ok: true,
    msg: { type: "ui_response", req: "r13", id: "b3b2e6f0", value: "Allow once" },
  });
  // none of the four answer fields: bad_args naming the fields, req carried.
  const none = fail('{"type":"ui_response","req":"r","id":"i"}');
  assert.equal(none.code, "bad_args");
  assert.equal(none.req, "r");
  assert.match(none.message, /value|confirmed|answer|cancelled/);
  const other = fail('{"type":"ui_response","req":"r","id":"i","declined":true}');
  assert.equal(other.code, "bad_args");
  // two or more of the four: bad_args naming the fields it saw.
  for (const body of ['"value":"x","confirmed":true', '"answer":{},"cancelled":true', '"value":"x","confirmed":true,"answer":{},"cancelled":true']) {
    const two = fail(`{"type":"ui_response","req":"r","id":"i",${body}}`);
    assert.equal(two.code, "bad_args", body);
    assert.equal(two.req, "r", body);
    assert.match(two.message, /value|confirmed|answer|cancelled/, body);
  }
  // a mistyped answer field: bad_args naming that field.
  for (const [field, value] of [
    ["value", 5],
    ["value", true],
    ["confirmed", "yes"],
    ["answer", "not an object"],
    ["answer", [1]],
    ["cancelled", 1],
  ] as const) {
    const e = fail(`{"type":"ui_response","req":"r","id":"i","${field}":${JSON.stringify(value)}}`);
    assert.equal(e.code, "bad_args", `${field}=${JSON.stringify(value)}`);
    assert.match(e.message, new RegExp(field), `${field}=${JSON.stringify(value)}`);
    assert.equal(e.req, "r", `${field}=${JSON.stringify(value)}`);
  }
});

test("parseLine accepts ui_response with confirmed, answer and cancelled", () => {
  assert.deepEqual(parseLine('{"type":"ui_response","req":"r","id":"i","confirmed":true}'), {
    ok: true,
    msg: { type: "ui_response", req: "r", id: "i", confirmed: true },
  });
  const q = { questionIndex: 0, question: "color", kind: "option", answer: "blue" };
  assert.deepEqual(parseLine(`{"type":"ui_response","req":"r","id":"i","answer":${JSON.stringify(q)}}`), {
    ok: true,
    msg: { type: "ui_response", req: "r", id: "i", answer: q },
  });
  assert.deepEqual(parseLine('{"type":"ui_response","req":"r","id":"i","cancelled":true}'), {
    ok: true,
    msg: { type: "ui_response", req: "r", id: "i", cancelled: true },
  });
});

test("parseLine requires req and id on ui_response and answers an unknown type ui_response variant with unknown_type", () => {
  const noReq = fail('{"type":"ui_response","id":"i","value":"x"}');
  assert.equal(noReq.code, "bad_args");
  assert.match(noReq.message, /req/);
  assert.equal(noReq.req, undefined);
  const noId = fail('{"type":"ui_response","req":"r","value":"x"}');
  assert.equal(noId.code, "bad_args");
  assert.match(noId.message, /id/);
  assert.equal(noId.req, "r");
  const badId = fail('{"type":"ui_response","req":"r","id":5,"value":"x"}');
  assert.equal(badId.code, "bad_args");
  assert.match(badId.message, /id/);
  assert.equal(badId.req, "r");
  const badReq = fail('{"type":"ui_response","req":5,"id":"i","value":"x"}');
  assert.equal(badReq.code, "bad_args");
  assert.match(badReq.message, /req/);
  // The wire's host-to-client UI messages are not client-to-host kinds: unknown_type.
  for (const line of [
    '{"type":"ui_request","id":"i","method":"select","title":"t","options":[],"purpose":"other","created_at":1}',
    '{"type":"ui_resolved","id":"i","outcome":"answered"}',
    '{"type":"notice","level":"info","message":"hi"}',
    '{"type":"status","key":"k","text":"v"}',
  ]) {
    const e = fail(line);
    assert.equal(e.code, "unknown_type", line);
    assert.equal(e.req, undefined, line);
  }
});

test("parseLine refuses a mistyped field with bad_args naming the field", () => {
  const cases: [string, string, string | undefined][] = [
    ['{"type":"abort_submission","req":"r","submission_id":"3"}', "submission_id", "r"],
    ['{"type":"abort_submission","req":"r"}', "submission_id", "r"],
    ['{"type":"attach","req":"r","cursor":{"epoch":"e","seq":"1"}}', "seq", "r"],
    ['{"type":"attach","req":"r","cursor":{"epoch":5,"seq":1}}', "epoch", "r"],
    ['{"type":"submit","req":"r","text":5}', "text", "r"],
    ['{"type":"submit","req":"r"}', "text", "r"],
    ['{"type":"abort","req":"r","background":"yes"}', "background", "r"],
    ['{"type":"hello","protocol":"1","min_protocol":1,"client":"c"}', "protocol", undefined],
    ['{"type":"hello","protocol":1,"min_protocol":1}', "client", undefined],
  ];
  for (const [line, field, req] of cases) {
    const e = fail(line);
    assert.equal(e.code, "bad_args", line);
    assert.match(e.message, new RegExp(field), line);
    assert.equal(e.req, req, line);
  }
});

test("encode ends the line with one newline and never holds a raw newline", () => {
  const line = encode({ type: "result", req: "r", data: { text: "a\nb\r\nc" } });
  assert.equal(line, '{"type":"result","req":"r","data":{"text":"a\\nb\\r\\nc"}}\n');
  assert.equal(line.indexOf("\n"), line.length - 1);
  assert.equal(encode({ type: "pong" }), '{"type":"pong"}\n');
});

test("negotiate picks the highest common protocol", () => {
  const h = (protocol: number, min_protocol: number) => ({ type: "hello" as const, protocol, min_protocol, client: "c" });
  assert.equal(negotiate(h(1, 1)), 1);
  assert.equal(negotiate(h(3, 1)), 1);
  assert.equal(negotiate(h(1, 0)), 1);
});

test("negotiate returns undefined when no version is common", () => {
  const h = (protocol: number, min_protocol: number) => ({ type: "hello" as const, protocol, min_protocol, client: "c" });
  assert.equal(negotiate(h(2, 2)), undefined);
  assert.equal(negotiate(h(0, 0)), undefined);
});

test("LIMITS holds the numbers of contract 6.14", () => {
  assert.deepEqual(
    { ...LIMITS },
    { clientLine: 1048576, hostLine: 67108864, slowClient: 8388608, helloMs: 5000, rebaseAtRunEnd: 256, rebaseMax: 4096 },
  );
  // the stub must not make this pass alone: the limits are used by shouldRebase and the wire
  assert.equal(encode({ type: "pong" }), '{"type":"pong"}\n');
});
