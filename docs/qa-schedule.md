# Daily QA wake on homeserver

The User explicitly requested cron waking the **existing OpenCode chat session**
at 3 a.m. Toronto, and corrected weekly approval as a policy bug: the authorized
cron and available live allowance suffice. This is not a second agent, a
subagent, a session fork, an automatic merge or a product deployment.

## Policy and source dependency

CPD selects the canonical toolkit's opt-in `standing_authorization`. Weekly
resets refresh the live budget, not permission. Optional immutable run records
and official windows remain useful audit data, not approval gates. Other
consumers retain descriptor-bound authority by default.

The wrapper only delivers a prompt. The agent must obtain fresh official usage,
run budget/upkeep checks, then repeat QA iterations only while permitted. Missing
telemetry, exhausted budget, unavailable credentials or tool permissions pause
work. Stopping or disabling the schedule revokes scheduled continuation; the
agent must honor this, and standing config must be removed when authority ends.

Merge the canonical source and the CPD consumer before installing this schedule
from a clean current-main checkout. Never activate an unreviewed policy branch
or grant blanket `--auto` permissions to bypass a blocker.

## Private host settings

Outside Git, create a mode-0600 JSON file with these fields, using absolute paths:

```json
{
  "session_id": "ses_REPLACE_WITH_VERIFIED_EXISTING_SESSION",
  "directory": "/absolute/path/to/cpd",
  "opencode": "/absolute/path/to/opencode",
  "state_dir": "/private/local/state/cpd-qa-wake",
  "prompt_file": "/absolute/path/to/cpd/docs/qa-scheduled-prompt.md"
}
```

Do not commit host settings or credentials. The directory must match the target
session's actual location; moving or archiving it is a blocker, not permission
to revive or replace it. The existing OpenCode service, provider login, GitHub
access and separate private cached Codex telemetry login must be usable without
interactive sign-in. The wrapper uses `opencode api` discovery/authentication,
not a hand-built unauthenticated HTTP request. No public listener is needed.
Use an explicit cron PATH containing the verified Node, Codex, Git and GitHub
CLI binaries in addition to the configured absolute OpenCode executable.

Safe read-only delivery preflight (never sends a prompt):

```sh
python3 scripts/qa-wake.py --config /private/path/wake.json --check
```

Busy sessions return `skipped`; missing/moved/archived/unverifiable sessions fail.
Do not test real delivery by prompting the current session during setup.

## Cron

Use an hourly minute-zero cron entry; the wrapper checks the Toronto hour with
Python's IANA zone data and admits work **only at 3 a.m. Toronto**. This avoids
assuming `CRON_TZ` support or changing the host's timezone (observed homeserver
zone: America/New_York). The other hourly invocations do not contact OpenCode or
invoke a model. Three a.m. exists exactly once across Toronto DST transitions.

```cron
0 * * * * /usr/bin/python3 /absolute/path/to/cpd/scripts/qa-wake.py --config /private/path/wake.json >> /private/path/wake.log 2>&1
```

Keep unrelated crontab entries intact. Back up and review the existing crontab
before adding one uniquely marked QA entry. Never replace the user's whole
crontab with only this example. Bound/rotate the private log using the host's
existing retention mechanism. No catch-up execution after downtime is promised.

## Duplicate handling and limitations

- A local POSIX file lock serializes wake wrappers. Busy target sessions or
  pending inbox work are skipped; nothing interrupts active work.
- A private per-Toronto-date receipt is written and synced **before** delivery.
  Timeouts/failures remain attempted: inspect the session inbox/messages before
  manually clearing a receipt. Never blindly resend a possibly admitted prompt.
- The wrapper sends to an exact existing ID via the session prompt API; it never
  creates/forks a session, changes permissions, or starts a separate agent.
- Admission is not completion. Review session results, failed cron invocations
  and handoffs. If the service is unavailable, delivery fails closed.
- The busy check and admission are not an atomic idle-only API operation. A user
  message racing the check may cause the wake to queue behind it. OpenCode still
  serializes that session's execution; all QA work remains with its one operator.
  The wake lock does not coordinate independent operators in other sessions;
  shared-group ownership must still be exclusive.
- No model output can guarantee an exact reserve landing with rounded telemetry,
  in-flight work or unrelated account usage. Recheck throughout each loop.

## Sources and verification

OpenCode V2 [API](https://opencode.ai/docs/api) and
[OpenAPI](https://opencode.ai/docs/openapi.json), consulted 2026-11-16:
`GET /api/session/{sessionID}`, `GET /api/session/active`,
`GET /api/session/{sessionID}/inbox`, and
`POST /api/session/{sessionID}/prompt` (`text`, `delivery: queue`, `resume: true`).
Session prompts durably admit work and schedule execution; no model call is
needed to discover the target or read its state.

`node --test scripts/qa-wake.test.mjs` executes isolated Python tests with a fake
API. Tests cover same-session delivery, Toronto standard/daylight time, private
receipts, busy/pending/archived/moved targets, concurrent invocations, duplicate
suppression, ambiguous POST failure and non-delivering preflight.
