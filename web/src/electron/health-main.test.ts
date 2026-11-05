import { describe, it, expect, vi } from 'vitest'
import { attachHealth } from './health-main.ts'
import type { HealthRecorder } from './health.ts'

void describe

type L = (...a: any[]) => void
function emitter() {
  const m = new Map<string, L[]>()
  return {
    on(e: string, l: L) { m.set(e, [...(m.get(e) ?? []), l]) },
    emit(e: string, ...a: unknown[]) { for (const l of m.get(e) ?? []) l(...a) },
  }
}

function setup() {
  const calls: unknown[][] = []
  const rec = (name: string) => async (...a: unknown[]) => { calls.push([name, ...a]) }
  const recorder: HealthRecorder = {
    init: rec('init'), begin: rec('begin'), healthy: rec('healthy'),
    crashed: rec('crashed'), exited: rec('exited'),
  }
  const webContents = emitter(), ipcMain = emitter(), proc = emitter(), app = emitter()
  attachHealth({ recorder, webContents, ipcMain, process: proc, app })
  return { calls, webContents, ipcMain, proc, app }
}

// ipc handlers receive (event, payload)
const ev = {}

it('attachHealth begins loading at attach', () => {
  expect(setup().calls).toEqual([['begin']])
})

it('attachHealth records loading on did-start-loading and SIGUSR1', () => {
  const s = setup()
  s.webContents.emit('did-start-loading')
  s.proc.emit('SIGUSR1')
  expect(s.calls).toEqual([['begin'], ['begin'], ['begin']])
})

it('attachHealth records healthy on health:ok', () => {
  const s = setup()
  s.ipcMain.emit('health:ok', ev)
  expect(s.calls).toEqual([['begin'], ['healthy']])
})

it('attachHealth records a crash from health:crash', () => {
  const s = setup()
  s.ipcMain.emit('health:crash', ev, { source: 'error-boundary', error: 'x', stack: 'st' })
  expect(s.calls).toEqual([['begin'], ['crashed', 'error-boundary', 'x', 'st']])
})

it('attachHealth ignores a malformed health:crash payload', () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {})
  const s = setup()
  s.ipcMain.emit('health:crash', ev, { source: 'bogus', error: 1 })
  s.ipcMain.emit('health:crash', ev, null)
  expect(s.calls).toEqual([['begin']])
  expect(err).toHaveBeenCalled()
  err.mockRestore()
})

it('attachHealth records render-process-gone except clean-exit', () => {
  const s = setup()
  s.webContents.emit('render-process-gone', ev, { reason: 'crashed' })
  s.webContents.emit('render-process-gone', ev, { reason: 'clean-exit' })
  expect(s.calls).toEqual([['begin'], ['crashed', 'render-process-gone', 'crashed']])
})

it('attachHealth records did-fail-load of the main frame except aborted', () => {
  const s = setup()
  s.webContents.emit('did-fail-load', ev, -105, 'NAME_NOT_RESOLVED', 'http://x/', true)
  s.webContents.emit('did-fail-load', ev, -3, 'ABORTED', 'http://x/', true)
  s.webContents.emit('did-fail-load', ev, -105, 'NAME_NOT_RESOLVED', 'http://x/', false)
  expect(s.calls).toEqual([['begin'], ['crashed', 'did-fail-load', 'NAME_NOT_RESOLVED (-105) http://x/']])
})

it('attachHealth records exited on before-quit', () => {
  const s = setup()
  s.app.emit('before-quit')
  expect(s.calls).toEqual([['begin'], ['exited']])
})
