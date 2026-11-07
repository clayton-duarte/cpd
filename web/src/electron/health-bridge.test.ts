import { describe, it, expect } from 'vitest'
import { createHealthBridge } from './health-bridge.ts'
import type { HealthBridge } from './health-bridge.ts'

void describe

function fake() {
  const sent: unknown[][] = []
  return { sent, ipc: { send: (...a: unknown[]) => { sent.push(a) } } }
}

// D2: health:ok is gated on the renderer's startup IPC. `reportHealthy` takes that startup
// promise and only sends once it resolves; a rejection reports health:crash instead.
type StartupGatedBridge = HealthBridge & { reportHealthy(startup: Promise<unknown>): void }

it('createHealthBridge reportHealthy sends health:ok', () => {
  const f = fake()
  createHealthBridge(f.ipc).reportHealthy()
  expect(f.sent).toEqual([['health:ok']])
})

it('createHealthBridge reportCrash sends health:crash with the payload', () => {
  const f = fake()
  const info = { source: 'window-error' as const, error: 'boom', stack: 's' }
  createHealthBridge(f.ipc).reportCrash(info)
  expect(f.sent).toEqual([['health:crash', info]])
})

it('sends health:ok only after the startup ipc resolves', async () => {
  const f = fake()
  let resolveStartup!: () => void
  const startup = new Promise<void>((resolve) => { resolveStartup = resolve })
  const bridge: StartupGatedBridge = createHealthBridge(f.ipc)
  bridge.reportHealthy(startup)
  await Promise.resolve()
  expect(f.sent).toEqual([])
  resolveStartup()
  await startup
  await Promise.resolve()
  expect(f.sent).toEqual([['health:ok']])
})

it('sends health:crash when the startup ipc rejects', async () => {
  const f = fake()
  const startup = Promise.reject(new Error('no handler registered for pty:hosts'))
  const bridge: StartupGatedBridge = createHealthBridge(f.ipc)
  bridge.reportHealthy(startup)
  await startup.catch(() => {})
  await Promise.resolve()
  expect(f.sent).toEqual([['health:crash', expect.objectContaining({ error: 'no handler registered for pty:hosts' })]])
})
