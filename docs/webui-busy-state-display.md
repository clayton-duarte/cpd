# WebUI Session Busy State and Resume

## Stopped state is not busy

The WebUI busy indicator must respect session state. A stopped Session is never shown as thinking or waiting for a tool result, even when its transcript ends with a tool result or an assistant tool-use stop reason. It is shown as stopped and awaiting an explicit user action.

The engine does not automatically restart a Lead or Manager Session when its pi process exits, whether cleanly or unexpectedly (User, 2026-11-14). The Session remains stopped until the User explicitly resumes it. Job retries and desired-awake durable-host supervision follow their separate rules; neither is an implicit resume of the stopped pi Session.

## Explicit Resume for the selected stopped Session

The chat pane offers Resume only for the selected Session when its state is `stopped`. The action is sent through the cockpit action protocol as `session.resume` with the selected Session's canonical ID (`view.id`); it must not identify the target by a display name, tree position, or a potentially stale selection key. Tree updates synchronize the attached Session's state, so the stopped-only action disappears when that Session is no longer stopped.

A successful action reports its result. A refusal or failure is shown to the user rather than swallowed. In particular, if a live pi pane is still tagged with the stopped Session, Resume refuses with the running-session error; it must not kill or replace that pane. A stopped marker alone does not prove that the tagged pi wrapper has exited.

### Durable Sessions

Explicitly resuming an eligible stopped durable Session wakes that existing durable host for supervision. Wake clears its sleep state; when the host-crash-loop latch is set, the existing durable wake policy also clears that latch and resets its restart bound. Resume does not start a pane, socket, host, or `session run`, and does not directly change the Session's stopped state. The successful result therefore remains `stopped` with no pane: being awake makes the durable Session eligible for engine supervision, but is not proof that its host is already running. An eligible already-awake durable Session needs no wake write.

### Loop-guard blocks

A blocked durable Session stopped by the loop guard is not the stopped-session wake case. Explicit loop-guard resume retains its existing guarded transaction and emits its scoped `session-resumed` signal, while preserving the blocked state and `φ: loop guard` label; if the durable Session is asleep, it may be woken in that same transaction. It does not start a pane or process. Other blocked durable Sessions remain refused. Do not emit `session-resumed` for a stopped Session resume: that signal denotes an explicit loop-guard resume.

## References

- Cockpit action protocol: `session.resume`, carrying the canonical selected Session ID.
- CLI command: `cpd session resume <key>` (`docs/commands.md` §10.17).
- Busy-state implementation: `web/src/server/pi/backend.ts` (`isBusy`).
- Durable wake policy: `core/src/session/moves.rs` (`wake_durable`); host supervision eligibility: `engine/src/watch/hosts.rs`.
- Resume safety and state transitions: `core/src/session/resume.rs`; pane-gone state transition: `core/src/session/state.rs` (`pane_gone`).
