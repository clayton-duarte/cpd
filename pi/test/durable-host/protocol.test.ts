import assert from "node:assert/strict";
import { test } from "node:test";
import { encode, HISTORY_MAX, IMAGE_MAX_COUNT, IMAGE_MAX_TOTAL, IMAGE_MIME, LIMITS, negotiate, parseLine } from "../../durable-host/protocol.ts";

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

test("parseLine accepts reload with req and rejects a foreign body with bad_args", () => {
  assert.deepEqual(parseLine('{"type":"reload","req":"r1"}'), { ok: true, msg: { type: "reload", req: "r1" } });
  // unknown fields are ignored
  assert.deepEqual(parseLine('{"type":"reload","req":"r2","extra":1}'), { ok: true, msg: { type: "reload", req: "r2" } });
  for (const line of ['{"type":"reload"}', '{"type":"reload","req":5}', '{"type":"reload","req":null}']) {
    const e = fail(line);
    assert.equal(e.code, "bad_args", line);
    assert.match(e.message, /req/, line);
  }
});

test("parseLine answers a type of a later PR with unknown_type", () => {
  for (const type of ["mcp", "rename"]) {
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
  assert.equal(negotiate(h(2, 2)), 2);
  assert.equal(negotiate(h(3, 1)), 2);
  assert.equal(negotiate(h(2, 0)), 2);
});

test("negotiate returns undefined when no version is common", () => {
  const h = (protocol: number, min_protocol: number) => ({ type: "hello" as const, protocol, min_protocol, client: "c" });
  assert.equal(negotiate(h(1, 1)), undefined);
  assert.equal(negotiate(h(0, 0)), undefined);
  assert.equal(negotiate(h(4, 3)), undefined);
});

test("LIMITS holds the numbers of contract 6.14", () => {
  assert.deepEqual(
    { ...LIMITS },
    { clientLine: 1048576, hostLine: 67108864, slowClient: 8388608, helloMs: 5000, rebaseAtRunEnd: 256, rebaseMax: 4096 },
  );
  // the stub must not make this pass alone: the limits are used by shouldRebase and the wire
  assert.equal(encode({ type: "pong" }), '{"type":"pong"}\n');
});

test("parseLine accepts compact with optional instructions", () => {
  assert.deepEqual(parseLine('{"type":"compact","req":"r","instructions":"summarize"}'), {
    ok: true,
    msg: { type: "compact", req: "r", instructions: "summarize" },
  });
  assert.deepEqual(parseLine('{"type":"compact","req":"r"}'), { ok: true, msg: { type: "compact", req: "r" } });
  const e = fail('{"type":"compact","req":"r","instructions":5}');
  assert.equal(e.code, "bad_args");
  assert.equal(e.req, "r");
  assert.match(e.message, /instructions/);
});

test("parseLine accepts configure with a model or a thinking_level and rejects both or none with bad_args", () => {
  assert.deepEqual(parseLine('{"type":"configure","req":"r","model":"anthropic/sonnet"}'), {
    ok: true,
    msg: { type: "configure", req: "r", model: "anthropic/sonnet" },
  });
  assert.deepEqual(parseLine('{"type":"configure","req":"r","thinking_level":"high"}'), {
    ok: true,
    msg: { type: "configure", req: "r", thinking_level: "high" },
  });
  assert.deepEqual(parseLine('{"type":"configure","req":"r","thinking_level":null}'), {
    ok: true,
    msg: { type: "configure", req: "r", thinking_level: null },
  });
  const none = fail('{"type":"configure","req":"r"}');
  assert.equal(none.code, "bad_args");
  assert.equal(none.req, "r");
  assert.match(none.message, /model|thinking_level/);
  const both = fail('{"type":"configure","req":"r","model":"anthropic/sonnet","thinking_level":"high"}');
  assert.equal(both.code, "bad_args");
  assert.equal(both.req, "r");
  assert.match(both.message, /model|thinking_level/);
  const badModel = fail('{"type":"configure","req":"r","model":5}');
  assert.equal(badModel.code, "bad_args");
  assert.match(badModel.message, /model/);
  const badLevel = fail('{"type":"configure","req":"r","thinking_level":5}');
  assert.equal(badLevel.code, "bad_args");
  assert.match(badLevel.message, /thinking_level/);
});

test("parseLine accepts list_models and fork with ownership and rejects an unknown ownership with bad_args", () => {
  assert.deepEqual(parseLine('{"type":"list_models","req":"r"}'), { ok: true, msg: { type: "list_models", req: "r" } });
  assert.deepEqual(parseLine('{"type":"fork","req":"r","at":7}'), { ok: true, msg: { type: "fork", req: "r", at: 7 } });
  for (const ownership of ["lead", "manager"] as const) {
    assert.deepEqual(parseLine(`{"type":"fork","req":"r","at":7,"ownership":"${ownership}"}`), {
      ok: true,
      msg: { type: "fork", req: "r", at: 7, ownership },
    });
  }
  const bad = fail('{"type":"fork","req":"r","at":7,"ownership":"worker"}');
  assert.equal(bad.code, "bad_args");
  assert.equal(bad.req, "r");
  assert.match(bad.message, /ownership/);
  const noAt = fail('{"type":"fork","req":"r"}');
  assert.equal(noAt.code, "bad_args");
  assert.match(noAt.message, /at/);
  const badAt = fail('{"type":"fork","req":"r","at":"7"}');
  assert.equal(badAt.code, "bad_args");
  assert.match(badAt.message, /at/);
});

test("parseLine accepts history with before and limit and rejects an over-cap limit with bad_args", () => {
  assert.deepEqual(parseLine('{"type":"history","req":"r"}'), { ok: true, msg: { type: "history", req: "r" } });
  assert.deepEqual(parseLine('{"type":"history","req":"r","before":"Y3Vyc29y","limit":10}'), {
    ok: true,
    msg: { type: "history", req: "r", before: "Y3Vyc29y", limit: 10 },
  });
  const over = fail(`{"type":"history","req":"r","limit":${HISTORY_MAX + 1}}`);
  assert.equal(over.code, "bad_args");
  assert.equal(over.req, "r");
  assert.match(over.message, /limit/);
  assert.match(over.message, new RegExp(String(HISTORY_MAX)));
  for (const limit of [0, -1, 1.5, "10"]) {
    const e = fail(`{"type":"history","req":"r","limit":${JSON.stringify(limit)}}`);
    assert.equal(e.code, "bad_args", String(limit));
    assert.match(e.message, /limit/, String(limit));
  }
  const badBefore = fail('{"type":"history","req":"r","before":5}');
  assert.equal(badBefore.code, "bad_args");
  assert.match(badBefore.message, /before/);
});

test("parseLine accepts submit with images and rejects an over-limit or a bad mime or a non-base64 data with bad_args", () => {
  const image = { mime_type: "image/png", data: "aGk=" };
  assert.deepEqual(parseLine(JSON.stringify({ type: "submit", req: "r", text: "hi", images: [image] })), {
    ok: true,
    msg: { type: "submit", req: "r", text: "hi", when_busy: "followUp", images: [image] },
  });
  for (const mime_type of IMAGE_MIME) {
    assert.deepEqual(parseLine(JSON.stringify({ type: "submit", req: "r", text: "hi", images: [{ mime_type, data: "aGk=" }] })), {
      ok: true,
      msg: { type: "submit", req: "r", text: "hi", when_busy: "followUp", images: [{ mime_type, data: "aGk=" }] },
    });
  }
  // an empty array is the absent one
  assert.deepEqual(parseLine('{"type":"submit","req":"r","text":"hi","images":[]}'), {
    ok: true,
    msg: { type: "submit", req: "r", text: "hi", when_busy: "followUp" },
  });
  // more than IMAGE_MAX_COUNT images
  const tooMany = Array.from({ length: IMAGE_MAX_COUNT + 1 }, () => ({ mime_type: "image/png", data: "aGk=" }));
  const count = fail(JSON.stringify({ type: "submit", req: "r", text: "hi", images: tooMany }));
  assert.equal(count.code, "bad_args");
  assert.equal(count.req, "r");
  assert.match(count.message, /images/);
  // more than IMAGE_MAX_TOTAL bytes of base64 data together
  const big = Array.from({ length: 2 }, () => ({ mime_type: "image/png", data: "A".repeat(Math.ceil(IMAGE_MAX_TOTAL / 2) + 4) }));
  const total = fail(JSON.stringify({ type: "submit", req: "r", text: "hi", images: big }));
  assert.equal(total.code, "bad_args");
  assert.match(total.message, /images/);
  // a mime outside IMAGE_MIME
  const mime = fail(JSON.stringify({ type: "submit", req: "r", text: "hi", images: [{ mime_type: "image/tiff", data: "aGk=" }] }));
  assert.equal(mime.code, "bad_args");
  assert.match(mime.message, /mime_type/);
  // data that is not RFC 4648 base64 with its padding
  for (const data of ["not base64!!", "aGk", "====", "aGk=extra"]) {
    const e = fail(JSON.stringify({ type: "submit", req: "r", text: "hi", images: [{ mime_type: "image/png", data }] }));
    assert.equal(e.code, "bad_args", data);
    assert.match(e.message, /data/, data);
  }
});
