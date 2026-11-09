import { describe, it, expect } from 'vitest'
import { createHealthRecorder, detectBreak, HealthParseError, parseHealth } from './health.ts'
import type { CockpitHealth, HealthLimits } from './health.ts'
import type { ContextNode, ContextTreeModel } from '../protocol/cockpit.ts'

void describe

const FILE = '/run/preview-health.json'
const LOG = '/run/preview-crash.log'

function setup(initial: Record<string, string> = {}, pid = 42) {
  const files = new Map(Object.entries(initial))
  const ops: string[] = []
  let t = new Date('2026-01-01T00:00:00.000Z')
  const fs = {
    async readFile(f: string) {
      const v = files.get(f)
      if (v === undefined) throw new Error('ENOENT')
      return v
    },
    async writeFile(f: string, d: string) { ops.push(`write ${f}`); files.set(f, d) },
    async rename(a: string, b: string) {
      ops.push(`rename ${a} ${b}`)
      const v = files.get(a)
      if (v === undefined) throw new Error('ENOENT')
      files.delete(a)
      files.set(b, v)
    },
    async appendFile(f: string, d: string) { files.set(f, (files.get(f) ?? '') + d) },
    async mkdir() {},
  }
  const recorder = createHealthRecorder({ file: FILE, crashLog: LOG, pid, now: () => t, fs })
  return {
    files, ops, recorder,
    setNow(d: string) { t = new Date(d) },
    rec: () => JSON.parse(files.get(FILE) ?? 'null'),
  }
}

it('parseHealth accepts each of the four statuses', () => {
  const base = { reloadEpoch: 1, pid: 7, startedAt: '2026-01-01T00:00:00.000Z' }
  const recs = [
    { status: 'loading', ...base },
    { status: 'healthy', ...base, healthyAt: '2026-01-01T00:00:01.000Z' },
    { status: 'crashed', ...base, crashedAt: '2026-01-01T00:00:01.000Z', source: 'error-boundary', error: 'e', stack: 's' },
    { status: 'exited', ...base, exitedAt: '2026-01-01T00:00:01.000Z' },
  ]
  for (const r of recs) expect(parseHealth(JSON.stringify(r))).toEqual(r)
})

it('parseHealth rejects malformed json and unknown status', () => {
  const base = { reloadEpoch: 1, pid: 7, startedAt: 'x' }
  expect(() => parseHealth('{nope')).toThrow(HealthParseError)
  expect(() => parseHealth(JSON.stringify({ ...base, status: 'weird' }))).toThrow(HealthParseError)
  expect(() => parseHealth(JSON.stringify({ ...base, status: 'loading', pid: 'x' }))).toThrow(HealthParseError)
})

it('recorder begin writes loading with epoch 1 on a missing file', async () => {
  const s = setup()
  s.setNow('2026-02-02T03:04:05.000Z')
  await s.recorder.init()
  await s.recorder.begin()
  expect(s.rec()).toEqual({ status: 'loading', reloadEpoch: 1, pid: 42, startedAt: '2026-02-02T03:04:05.000Z' })
})

it('recorder begin continues the epoch of the previous file', async () => {
  const prev = { status: 'exited', reloadEpoch: 4, pid: 9, startedAt: 'a', exitedAt: 'b' }
  const s = setup({ [FILE]: JSON.stringify(prev) })
  await s.recorder.init()
  await s.recorder.begin()
  expect(s.rec()).toMatchObject({ status: 'loading', reloadEpoch: 5, pid: 42 })
})

it('recorder begin while loading is a no-op', async () => {
  const s = setup()
  await s.recorder.init()
  await s.recorder.begin()
  const before = s.files.get(FILE)
  s.setNow('2026-03-03T00:00:00.000Z')
  await s.recorder.begin()
  expect(s.files.get(FILE)).toBe(before)
  expect(s.rec().reloadEpoch).toBe(1)
})

it('recorder healthy writes healthy with healthyAt', async () => {
  const s = setup()
  await s.recorder.init()
  await s.recorder.begin()
  s.setNow('2026-01-01T00:00:09.000Z')
  await s.recorder.healthy()
  expect(s.rec()).toEqual({
    status: 'healthy', reloadEpoch: 1, pid: 42,
    startedAt: '2026-01-01T00:00:00.000Z', healthyAt: '2026-01-01T00:00:09.000Z',
  })
})

it('recorder healthy is ignored after a crash in the same epoch', async () => {
  const s = setup()
  await s.recorder.init()
  await s.recorder.begin()
  await s.recorder.crashed('window-error', 'boom')
  await s.recorder.healthy()
  expect(s.rec()).toMatchObject({ status: 'crashed', reloadEpoch: 1 })
})

it('recorder crashed writes the crash and appends a log line', async () => {
  const s = setup()
  await s.recorder.init()
  await s.recorder.begin()
  s.setNow('2026-01-01T00:00:05.000Z')
  await s.recorder.crashed('error-boundary', 'boom', 'stk')
  expect(s.rec()).toEqual({
    status: 'crashed', reloadEpoch: 1, pid: 42, startedAt: '2026-01-01T00:00:00.000Z',
    crashedAt: '2026-01-01T00:00:05.000Z', source: 'error-boundary', error: 'boom', stack: 'stk',
  })
  const lines = (s.files.get(LOG) ?? '').trim().split('\n').map((l) => JSON.parse(l))
  expect(lines).toEqual([{
    at: '2026-01-01T00:00:05.000Z', reloadEpoch: 1, pid: 42,
    source: 'error-boundary', error: 'boom', stack: 'stk',
  }])
})

it('recorder exited writes exited', async () => {
  const s = setup()
  await s.recorder.init()
  await s.recorder.begin()
  s.setNow('2026-01-01T00:01:00.000Z')
  await s.recorder.exited()
  expect(s.rec()).toEqual({
    status: 'exited', reloadEpoch: 1, pid: 42,
    startedAt: '2026-01-01T00:00:00.000Z', exitedAt: '2026-01-01T00:01:00.000Z',
  })
})

it('recorder writes atomically through a tmp file and rename', async () => {
  const s = setup()
  await s.recorder.init()
  await s.recorder.begin()
  const w = s.ops.findIndex((o) => o.startsWith('write ') && !o.endsWith(FILE))
  expect(w).toBeGreaterThanOrEqual(0)
  const tmp = s.ops[w]!.slice('write '.length)
  expect(s.ops).toContain(`rename ${tmp} ${FILE}`)
  expect(s.ops).not.toContain(`write ${FILE}`)
  expect(s.files.has(tmp)).toBe(false)
})

it('recorder treats an unreadable previous file as epoch 0', async () => {
  const s = setup({ [FILE]: '{garbage' })
  await s.recorder.init()
  await s.recorder.begin()
  expect(s.rec()).toMatchObject({ status: 'loading', reloadEpoch: 1 })
})

const T0 = Date.parse('2026-01-01T00:00:00.000Z')
const LIMITS: HealthLimits = { snapshotGraceMs: 10000, heartbeatTimeoutMs: 5000 }

const ctx = (id: string): ContextNode => ({ id, title: id, archived: false, records: 0, openQuestions: 0, sessions: [], plans: [] })
const tree = (contexts: readonly ContextNode[]): ContextTreeModel => ({ contexts, cursor: 0 })

it('an empty cockpit tree is a break', () => {
  const empty: CockpitHealth = { epoch: 1, startedAt: T0, tree: null }
  expect(detectBreak(empty, T0, LIMITS)).toBeNull() // inside the loading grace
  expect(detectBreak(empty, T0 + 9999, LIMITS)).toBeNull()
  const break_ = detectBreak(empty, T0 + 10000, LIMITS)
  expect(break_).toMatchObject({ kind: 'empty-data' })
  expect(typeof break_?.detail).toBe('string')
  expect(break_?.detail.length).toBeGreaterThan(0)
  // a painted cockpit that received data and keeps its heartbeat is not a break
  const live: CockpitHealth = { epoch: 1, startedAt: T0, tree: tree([ctx('c1')]), snapshotAt: T0, heartbeatAt: T0 + 10000 }
  expect(detectBreak(live, T0 + 10000, LIMITS)).toBeNull()
})

it('a stalled WebSocket is a break', () => {
  const live: CockpitHealth = { epoch: 1, startedAt: T0, tree: tree([ctx('c1')]), snapshotAt: T0, heartbeatAt: T0 }
  expect(detectBreak(live, T0 + 4999, LIMITS)).toBeNull()
  const break_ = detectBreak(live, T0 + 5000, LIMITS)
  expect(break_).toMatchObject({ kind: 'websocket-stalled' })
  expect(typeof break_?.detail).toBe('string')
  expect(break_?.detail.length).toBeGreaterThan(0)
  // no heartbeat reported yet is not a stall: there is nothing to measure
  const { heartbeatAt: _heartbeatAt, ...silent } = live
  expect(detectBreak(silent, T0 + 50000, LIMITS)).toBeNull()
})
