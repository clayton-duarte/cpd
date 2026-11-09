# Blocked-send feedback: evidence and remaining contract

The reported Preview message at 2026-11-08 19:32 EST has no attributable Event/input ID, rejection, queue record, or consumption evidence. Its target and runner remain unverified. The screenshot shows `S2 / deepseek-v4.1-flash / high` and a partly obscured error toast; its full error text and message history are unreadable. Session metadata instead reported `openai/gpt-6.1-terra` and a loop-guard block. Neither image nor records establish a wrong selection, the selector's source, or delivery. Do not reload, resend, resume, or modify that live state.

## Source findings

- The plan row chooses its lead session ID; `connection.submit` uses `selectedSessionId`. The hub requires the same attached ID and routes to its runner. The composer selector displays `view.footer`, not a separate target selector. An attached session's `ref` is not refreshed by tree patches, so its status may be stale; the tree separately reflects subsequent patches.
- For pi, `backend.ts` prefers the latest transcript `model_change` over `cpd session show` for the footer. The mocked `backend.test.ts` footer case proves this can produce different values (`p/later` versus the fake show `bifrost/m1`); it does not establish the screenshot's actual path or cause. For durable, footer model comes from host agent state or transcript facts. The actual reported runner is unknown.
- Source comparison confirmed by engine-defects `scout-webui-guard`: pi TUI's human `messageType=user` clears a tripped loop guard; WebUI pi `session.send --origin webui` produces a `session-message` Event, delivered as a custom message and held for next turn while tripped. Origin is a display tag, not a human-input classification. Durable socket `cpd-chat` with `reply:true` clears an already tripped guard; senderless Events do not. This is a source mismatch, not proof of the Preview message's disposition.

## Unmet acceptance and correlation

The engine-owned human-input parity and correlated disposition are not implemented or verified here. `session.submitAck` correlates only session and client ID; pi drops the CLI's Event ID, and durable backend drops the host status. A failure can follow an Event commit; durable held ID `-1` is not accepted or consumed; changing attachments can suppress an old-generation completion. A receipt or queued flag is not consumption, guard reset, or agent recovery. The needed engine contract links session + client/request ID to Event/input ID and consumed/held/rejected and guard-clear disposition. Until then this change retains the draft, calls the outcome unconfirmed or unknown, and does not auto-replay.

Unresolved attachments and feedback survive session changes within the mounted chat; they are in memory and do **not** survive a page reload. Text uses the existing local draft store. This is partial UI work, not live mitigation or incident resolution. The original Preview incident remains open.
