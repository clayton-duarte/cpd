import { WebError } from "../protocol/domain.ts";
import { createElectronTransport } from "./transport.ts";

function fakeBridge() {
  const sent: unknown[] = [];
  const subs = new Set<(raw: unknown) => void>();
  return {
    sent,
    subs,
    bridge: {
      sendWeb: (raw: unknown) => {
        sent.push(raw);
      },
      onWebMessage: (cb: (raw: unknown) => void) => {
        subs.add(cb);
        return () => {
          subs.delete(cb);
        };
      },
    },
    emit: (raw: unknown) => {
      for (const cb of [...subs]) cb(raw);
    },
  };
}

async function opened() {
  const f = fakeBridge();
  const t = createElectronTransport(f.bridge);
  const statuses: string[] = [];
  t.onStatus((s) => statuses.push(s));
  t.connect();
  await Promise.resolve();
  return { f, t, statuses };
}

test("createElectronTransport reports open after connect", async () => {
  const f = fakeBridge();
  const t = createElectronTransport(f.bridge);
  const statuses: string[] = [];
  t.onStatus((s) => statuses.push(s));
  t.connect();
  expect(statuses).toEqual([]);
  await Promise.resolve();
  expect(statuses).toEqual(["open"]);
});

test("createElectronTransport send throws WebError internal before open", () => {
  const f = fakeBridge();
  const t = createElectronTransport(f.bridge);
  let error: unknown;
  try {
    t.send({ a: 1 });
  } catch (e) {
    error = e;
  }
  expect(error).toBeInstanceOf(WebError);
  expect((error as WebError).code).toBe("internal");
  expect((error as WebError).message).toBe("transport is not open");
  expect(f.sent).toEqual([]);
});

test("createElectronTransport send forwards raw frames to the bridge once open", async () => {
  const { f, t } = await opened();
  const frame = { a: 1 };
  t.send(frame);
  expect(f.sent).toEqual([frame]);
});

test("createElectronTransport delivers bridge messages to message handlers", async () => {
  const { f, t } = await opened();
  const got: unknown[] = [];
  t.onMessage((raw) => got.push(raw));
  f.emit({ n: 1 });
  expect(got).toEqual([{ n: 1 }]);
});

test("createElectronTransport gives disposed handlers nothing", async () => {
  const { f, t } = await opened();
  const got: unknown[] = [];
  const sub = t.onMessage((raw) => got.push(raw));
  sub.dispose();
  f.emit({ n: 1 });
  expect(got).toEqual([]);
});

test("createElectronTransport close is idempotent and silences events", async () => {
  const { f, t, statuses } = await opened();
  const got: unknown[] = [];
  t.onMessage((raw) => got.push(raw));
  t.close();
  t.close();
  f.emit({ n: 1 });
  t.connect();
  await Promise.resolve();
  expect(got).toEqual([]);
  expect(statuses).toEqual(["open"]);
  expect(() => t.send({ a: 1 })).toThrow("transport is not open");
  expect(f.sent).toEqual([]);
});
