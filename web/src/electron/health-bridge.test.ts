import { describe, it, expect } from 'vitest'
import { createHealthBridge } from './health-bridge.ts'

void describe

function fake() {
  const sent: unknown[][] = []
  return { sent, ipc: { send: (...a: unknown[]) => { sent.push(a) } } }
}

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
