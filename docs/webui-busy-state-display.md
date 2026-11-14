# WebUI Session Busy State and Resume Action Display

## Contract: Stopped Sessions Never Show as Thinking

The WebUI busy indicator follows session state: busy requires the session not be stopped. A stopped session never shows as thinking, even when its transcript ends on a toolResult or assistant message with a toolUse stop reason; instead, a stopped session with such a transcript is displayed as stopped and awaiting resume.

### Rationale

When a pi Session running in tmux stops (the pi process exits and the pane dies), the session's state is recorded as `stopped` in the database. When the engine's liveness check detects that a pi process's pane is dead or gone, it marks the dead pane state as stopped via `pane_gone()` (`core/src/session/state.rs:129-152`). If the session's last message was an assistant message that called a tool (with stop reason `toolUse`), or if the last message role was `toolResult`, the session is awaiting the tool's result to be processed.

The WebUI backend's `isBusy()` function (`web/src/server/pi/backend.ts`) determines whether to show a busy indicator based on the transcript's last role and stop reason. However, the display must also respect the session's state: a stopped session should always show as stopped and awaiting user action (resume), never as actively thinking or waiting for a tool result.

This contract ensures the WebUI accurately represents session state to the user: stopped is a distinct, visible state separate from the busy/thinking indicator, giving the user clear context for why they cannot interact with the session (it is not running) versus why they cannot interrupt it (it is busy).

## Contract: Resume Action for Stopped Sessions

In the WebUI chat pane, a stopped pi Session displays its state as 'stopped' and renders a Resume button (`ResumeButton`). The Resume button is displayed only when the session's state is `stopped`.

Clicking the Resume button triggers a server action that invokes `cpd session resume <key>` for the target session key:
- **Execution:** The server action executes `cpd session resume <key>` against the core CLI. The command restarts the stopped pi Session in the tmux cockpit (creating the cockpit session if absent, and preserving the session id and transcript).
- **Result and Error Display:** The outcome of the resume action is displayed directly in the WebUI:
  - On success, the result is reported and the session resumes execution, clearing the stopped display once running.
  - On refusal or failure (e.g., non-zero exit code or CLI error), the resulting error message is displayed to the user.

### Accepted Decision: Stopped State + Resume Action (No Auto-Restart)

Per User decision on bug b65 (issue #1207):
- **No auto-restart:** When a pi process terminates (clean exit or unexpected termination), the engine detects the dead pane and sets state `stopped` via `pane_gone()` (`core/src/session/state.rs:129-152`). The engine does not automatically restart the process.
- **Stopped state + Resume action:** The session remains stopped until the User explicitly clicks the Resume button in the chat pane (or runs `cpd session resume <key>`).
- **Upstream boundary:** pi 1.5.2 crashing on a codemode `AbortError` is an upstream concern and out of scope.

### References

- GitHub issue: #1207 (Fixes #1207, bug b65)
- Evidence: scouts at `$CPD_HOME/run/manager-death/explore/scout-{1,2,3}.md`
- CLI command: `cpd session resume <key>` (`docs/commands.md` §10.17)
- Engine state transition: `core/src/session/state.rs:129-152` (`pane_gone()`)
- Implementation:
  - Backend busy check: `web/src/server/pi/backend.ts` lines 68-70 (`isBusy` function)
  - UI component: `web/src/ui/chat/ResumeButton.tsx`
  - Tests: `web/src/ui/chat/ResumeButton.test.tsx`
- WebUI contract: `web/CONTRACT.md` section 6.5 (backend.ts busy documentation)

