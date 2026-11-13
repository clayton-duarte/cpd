# WebUI Session Busy State Display

## Contract: Stopped Sessions Never Show as Thinking

The WebUI busy indicator follows session state: busy requires the session not be stopped. A stopped session never shows as thinking, even when its transcript ends on a toolResult or assistant message with a toolUse stop reason; instead, a stopped session with such a transcript is displayed as stopped and awaiting resume.

### Rationale

When a pi Session running in tmux stops (the pi process exits and the pane dies), the session's state is recorded as `stopped` in the database. If the session's last message was an assistant message that called a tool (with stop reason `toolUse`), or if the last message role was `toolResult`, the session is awaiting the tool's result to be processed.

The WebUI backend's `isBusy()` function (`web/src/server/pi/backend.ts`) determines whether to show a busy indicator based on the transcript's last role and stop reason. However, the display must also respect the session's state: a stopped session should always show as stopped and awaiting user action (resume), never as actively thinking or waiting for a tool result.

This contract ensures the WebUI accurately represents session state to the user: stopped is a distinct, visible state separate from the busy/thinking indicator, giving the user clear context for why they cannot interact with the session (it is not running) versus why they cannot interrupt it (it is busy).

### References

- GitHub issue: #1207
- Implementation: `web/src/server/pi/backend.ts` lines 68-70 (isBusy function)
- WebUI contract: `web/CONTRACT.md` section 6.5 (backend.ts busy documentation)
