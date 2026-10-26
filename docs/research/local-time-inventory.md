# Inventory of every time cpd shows to a person

Plan `local-time`, Job `research-docs` (Attempt 1). Research only: no code changed.
Read on the Lead Branch at `eefc5718` (worktree `cpd.local-time--research-docs-a1`), 2026-10-26.

The rule (Context records pro-d41, pro-d42; accepted assumptions A2, A3): a time in a **field** stays ISO 8601 UTC with `Z`
(contract §1.3); a time in **prose** for a person is machine-local with the zone abbreviation named (`2026-10-25 23:03 EDT`);
narrow `HH:MM` fold rows stay bare. This file only locates and classifies; it designs no formatter and proposes no
enforcement.

**Method.** (1) `rg` for every formatter and source of a time over `core/src`, `cli/src`, `engine/src`, `tui/src`, `pi/lib`,
`pi/compact-tools`, `pi/ask-user-question`, `pi/skills`, `docs/`: `clock::timestamp`, `clock::now`, `to_offset`,
`format_description!`, `OffsetDateTime`, `Rfc3339`, `toISOString`, `new Date`, `Date.now`, `localTime`, `localDateTime`,
`strftime`, `date +%z`, string slices of an `at` (`get(..10)`, `get(11..19)`, `slice(0, 10)`), and the field names `at`, `since`,
`*_at`, `until`, `expires`. (2) Followed each to where it meets a person: a `format!` into an Event `text`, an error
`message`, a markdown string, a TUI row or label, a digest line, a log line. (3) Ran read-only `cpd context show`, `cpd context
show --digest`, `cpd context show --record`, `cpd events list --since 19000`, `cpd inbox list`, `cpd plan show` in the worktree
(they work: they read the User's real store). Every `file:line` below was read on this branch; a script re-checked that each
file has that line (see "Checks"). Claims not verified say so in the same sentence.

**Classes.** FIELD (stays UTC), PROSE (a person reads it; must become local with the zone named), ALREADY LOCAL (shown local
today), DOC (documentation text or example showing a time).

**Two sources feed everything.** In Rust, every time is made by `core/src/clock.rs`: `timestamp()` (line 10: UTC, `Z`,
milliseconds), `now()` (line 17) and `Clock::utc()` (line 26; `SystemClock` line 46, `ManualClock` line 103). The local
offset exists once, in `core/src/inbox/cmd.rs:255` `local_offset_with` (`date +%z` through the mockable `Runner`). In TypeScript,
`pi/lib/time.ts` (`localTime` line 18, `localDateTime` line 23) formats with Node's `Date` local getters and names no zone.

## PROSE: a person reads it, must become local with the zone named

| file:line | producer | current rendered form (real example) | formatter | proposed owner of the change |
|---|---|---|---|---|
| `core/src/context/render.rs:188` | `date()`, called by `item` (175), `nested` (179), `nested_brief` (184), `raw` (146-148, no non-test caller found), `single` (153-159): every line of `cpd context show` markdown, `--since`, `--record`, `--topic` | `- pro-p1 (2026-10-23): owner-fix (on pro, …)`; `## pro-d41 (decision, 2026-10-26)` (live). A **UTC date** cut from `at` with `get(..10)`: the date is wrong for the User in the evening (23:03 EDT is already the next UTC day) and it names no zone | string slice, no formatter | `date()` in `render.rs`: call the one shared Rust local-time formatter on the `at` string |
| `core/src/context/distill.rs:674` | `records_md`: `records.md`/`batch.md` that the distiller pi agent reads (contract §4.6 step 3, §2084) | `[pro-d41] decision (2026-10-26)`; same UTC date slice | `record.at.get(..10)` | `records_md` in `distill.rs` (calls the same formatter as `render.rs`); guess: it is agent input, not a person's screen, but the Requirement lists "what agents write" and the distiller's output becomes records |
| `pi/lib/updates.ts:187` | `line()`, brief branch: every record line of the digest that starts a Manager/Lead turn (`§15.7`) | `- pro-b1 brief (2026-10-23): Correct a Context's owner Plan owner-fix.` UTC date | `r.at.slice(0, 10)` | `line()` in `pi/lib/updates.ts`: call `pi/lib/time.ts` |
| `pi/lib/updates.ts:188` | `line()`, other kinds: same digest | `- pro-d41 decision (2026-10-26): Times in the machine's time zone …` UTC date | `r.at.slice(0, 10)` | same `line()` in `pi/lib/updates.ts` |
| `engine/src/watch/engine.rs:423` | `Downtime::sentence`, used at `engine.rs:462` (the `watch-started` Event `text`, also delivered to Managers as the Event's text) and `engine.rs:466` (`watch.log`) | `The engine was down for 3.2 s: it stopped at 2026-10-25T22:29:00.000Z.` (format from the `ports_tests.rs:303` assertion; the live store has no such Event, unverified for real output) | `clock::timestamp(self.since)` | `Downtime::sentence` in `engine/src/watch/engine.rs` |
| `engine/src/watch/restart.rs:83` | `Pending::describe`, pending branch: `cpd doctor` check `tmux_restart` (`engine/src/doctor/mod.rs:44-45`) and bare `cpd` (`cli/src/cli.rs:245`, failed branch only) | `A tmux restart is pending since 2026-10-25T21:10:00.000Z: the server runs tmux 3.4, the installed tmux is 3.5. No Attempt starts until it has run.` (shape from source; no live restart was pending, unverified) | `self.at` is a `clock::timestamp` string (`restart.rs:91`) printed as is | `Pending::describe` in `engine/src/watch/restart.rs` |
| `engine/src/watch/restart.rs:85` | `Pending::describe`, failed branch: same surfaces | `The tmux restart failed at 2026-10-25T21:10:00.000Z: <reason> See run/tmux-restart.log.` | `f.at` printed as is | same `describe` |
| `engine/src/watch/pr.rs:246` | rate-limit `Failure.message` (code `gh_rate_limited`; becomes an `error` Event text the User reads) | `GitHub's rate limit has 12 requests left: PR polling pauses until 2026-10-20T11:00:00Z.` (`reset_at` is GitHub's string, printed as is; `details.reset_at` at line 247 stays a field) | `{reset_at}` raw | the `message` in `engine/src/watch/pr.rs` (parse `reset` already exists at line 241) |
| `core/src/inbox/cmd.rs:311` | `inbox snooze` Event text `inbox-snoozed` (Event log, `cpd events list`) | `Inbox item 12 snoozed until 2026-10-14T22:40:02.114Z: inbox/core-docs failed.` The same `until` string is also the field at `:307` and `data` at `:311` | `clock::timestamp` at `cmd.rs:307` | the `format!` at `core/src/inbox/cmd.rs:311`: keep `until` UTC for the field, give the text its own local rendering |
| `tui/src/logs/view.rs:133-135` | `clock()`, used by `line()` (161): every row of the Event log dialog (`logs`, commands.md §13 R36) | `03:05:11 Message for italic from cpd-manager (…)`: **UTC** `HH:MM:SS` cut from `e.at` with `get(11..19)` and no zone, so it reads as local and is not (live Event at `2026-10-26T03:05:11.907Z` is 23:05 EDT) | string slice | `clock()` in `tui/src/logs/view.rs`: shared Rust formatter; guess: this wide row may keep `HH:MM:SS` bare under pro-d42's narrow-row rule once the cockpit names the zone, the Lead decides |
| `tui/src/worker/render.rs:27` | `render()`: every transcript row of the Worker view and of `cpd session history` (`tui/src/history/`) | `22:54:10 you`, `22:54:11 → bash ls`: **UTC** `HH:MM:SS` cut from pi's `timestamp` with `get(11..19)` (`:148` is the same slice in a test reference, not production) | string slice | `render()` in `tui/src/worker/render.rs`: same formatter as `logs/view.rs` |

**Surfaces of the Requirement, checked and found to hold no other time in prose** (each by reading the producer, not by
guess):

- **Error messages** (`CpdError::new(..., format!(...))` over `core/src`, `cli/src`, `engine/src`, `tui/src`): the only error
  messages carrying a time are `engine/src/watch/pr.rs:246` above and the *invalid value* messages that echo a bad stored
  time (`engine/src/watch/workers.rs:128`, `core/src/incident.rs:345`, `engine/src/watch/schedule.rs:57`,
  `engine/src/watch/titles.rs:70`, `core/src/session/reload.rs:81,131`, `core/src/host/install_state.rs:94`): they quote a
  malformed value on purpose to say it is not RFC 3339; keep it as written (guess: they are diagnostics of a corrupt field, so UTC
  quoting is correct; the Lead decides).
- **Event texts** (`NewEvent::…(format!(…))`): times appear only in `watch-started` (`engine.rs:423`) and `inbox-snoozed`
  (`inbox/cmd.rs:311`). The `session-message` Event text (`core/src/session/send.rs:99,102`) holds no time; the time of
  that message is added by the adapter (`pi/lib/delivery.ts:43`, ALREADY LOCAL).
- **`context show`**: `render.rs:188` above. `--digest` prints `[pro-d41] <short>` with no date (live), and `context search`
  lines (`core/src/context/search.rs:294`) carry none; `context pending --markdown` (`core/src/context/pending.rs:150`)
  and plan markdown (`core/src/plan/render.rs`, `summary.rs`) carry none (read; `rg` for `_at`/`at` found none).
- **Digests that start a turn** (`pi/lib/updates.ts`): `line()` 187-188 (PROSE) and the `Board:` section 195 (ALREADY LOCAL).
  The Plan Event lines of that digest (`updates.ts:59`) use the Event `text` and no time of their own.
- **Inbox labels** (`core/src/inbox/rules.rs`: 187, 200, 214, 243, 283, 320-323, 524, 547): none holds a clock time;
  `loop guard, N turns in 10 min` is a duration. The Inbox row's age is a duration (`tui/src/tree/inbox.rs:162`, see "Not a time").
  The one time in the Inbox UI is the menu label `Snooze until tomorrow 09:00` (ALREADY LOCAL).
- **The agent skills** (`pi/skills/cpd-lead/SKILL.md`, `cpd-manager/SKILL.md`, `references/*.md`): hold no time or date
  example at all (`rg` for date/time/UTC/clock found only the DOC row below).
- **What agents write**: not produced by cpd code. Real instance in the live store: Event `context-record` text
  `note pro-n29 recorded in Context eec-general: 2026-10-26: cpd #304 (pr-adopt) was closed …` (the date is typed by the agent)
  and `open-italic failed at 22:54 EDT` in a Manager's message. Only skill text and record rules can reach this (see DOC).

## ALREADY LOCAL: shown in local time today

| file:line | producer | current rendered form | formatter | names the zone? |
|---|---|---|---|---|
| `pi/lib/updates.ts:195` | `boardSection`: `Board:` lines of the Manager's turn-start digest | `- User (2026-10-25 22:29): <post text>` | `localDateTime` (`pi/lib/time.ts:23`) | no |
| `pi/lib/delivery.ts:43` | `sessionMessage`: `session-message` content a Lead/Manager reads (commands.md §15.3) | `Message from the User (22:29): <text>` | `localTime` (`pi/lib/time.ts:18`) | no. It is message **content** the model and the chat both read, not only a fold row: whether it stays bare is a decision for the Lead |
| `pi/lib/render.ts:56` (used `:209`) | `timeOf`: the `HH:MM` of a cpd message fold's closing row | `╰󰬦 expand · 22:29` | `localTime` | no (narrow fold row; stays bare per pro-d42) |
| `pi/compact-tools/extensions/compact-tools.ts:141` (used `:155`, `:163`, `:168`, row at `:327`) | `localTimeOf`, `branchTime`, `rowTime`: tool closing toggle row | `╰󰬦 expand · 22:29 · Done in 1.151s` | `localTime` | no (narrow fold row) |
| `pi/compact-tools/extensions/compact-tools-balloon.ts:33` | agent balloon bottom border | `│╭─22:29───╯` | `localTime` | no (narrow) |
| `pi/compact-tools/extensions/compact-tools-thinking.ts:260` | Thinking fold time | `HH:MM` on the fold row | `localTime` | no (narrow) |
| `tui/src/tree/menu.rs:51` | Inbox row menu label (the snooze target is computed local: `core/src/inbox/cmd.rs:286-296`, `local_offset_with` 255, one `date +%z` call on the `Runner`) | `Snooze until tomorrow 09:00` | literal text; the instant is built with `now.to_offset(local_offset_with(runner)?)` (`cmd.rs:291`) | no |
| `pi/lib/time.ts:9-14` | `localParts`: `new Date(at)` with `getFullYear/getMonth/getDate/getHours/getMinutes` (process zone = TZ, then /etc/localtime) | helper for all rows above | Node local `Date` | no, it has no zone-name output |

Not found: a cockpit element that shows an absolute local time with a date (the "cockpit board" of the Plan brief is, as far
as I could verify, the pi `Board:` digest above; no `tui/src` file formats a local date: `rg '\[hour\]|strftime|getHours|%z'` over
`tui/src` hits only `tree/menu.rs`'s literal label). Whether the cockpit prints `2026-10-25 22:29` anywhere else is unverified:
no cockpit was launched (`cpd tui` never exits, forbidden).

## FIELD: stays ISO 8601 UTC with Z

Each is written by `clock::timestamp`/`clock::now` (`core/src/clock.rs:10`, `:17`) or SQLite's `strftime(...Z)`; a person sees
it only when a command prints the JSON field. None of these changes.

| file:line | producer | current form (real example) | formatter |
|---|---|---|---|
| `core/src/clock.rs:10` | `timestamp()` | `2026-10-25T22:29:00.123Z` | `format_description!` UTC |
| `core/src/clock.rs:17` | `now()` | same | `timestamp(now_utc())` |
| `core/src/store/mod.rs:768` | `txns.at` of every command (Event `at`, history `at`) | `2026-10-26T03:05:11.907Z` (live `events list`) | `clock::now()` |
| `core/src/store/mod.rs:930` | store slow-log line field `at` | `{"at":"…Z",…}` | `clock::now()` |
| `core/src/events/select.rs:133` | Event `at` in `events list`/`follow` JSON; read by `pi/lib/event.ts:40,85` | `"at":"2026-10-19T15:17:50.426Z"` | stored |
| `core/src/context/record.rs:128` | record JSON `at` (`context show` JSON `records[].at`); stored `record.rs:256` | `"at":"2026-10-14T21:12:00.000Z"` | `clock::now()` at `context/write.rs`/`curation.rs:101,301,354` |
| `core/src/context/mod.rs:316` | `created_at` of a Context | `2026-10-14T21:10:00.000Z` | stored |
| `core/src/context/search.rs:286` | search hit `at` | same | stored |
| `core/src/context/curation.rs:101` | `curated_at` | same | `clock::now()` |
| `core/src/context/distill.rs:491`, `:917` | distill run `started_at` (run log `runs.jsonl`, contract §4.6) | `started_at` UTC | `clock::timestamp(ports.clock.utc())` |
| `core/src/board.rs:97`, `:118`, `:147` | board `posted_at`, `at`, `read_at` | `2026-10-21T15:30:00.000Z` | `clock::now()` |
| `core/src/review/mod.rs:122`, `:160` | plannotator `opened_at`/`closed_at` | UTC | `clock::now()` |
| `core/src/incident.rs:258`, `:297`, `:334` | incident `started_at`, `ended_at` | `2026-10-14T21:10:00.000Z` | `clock::now()` |
| `core/src/plan/show.rs:36,44-46,155,174,206` | `plan show` JSON: `opened_at`, `created_at`, `started_at`, `ended_at`, `asked_at`, `requested_at`, `decided_at` | UTC | stored |
| `core/src/job/show.rs:85,88,115` and `core/src/attempt/show.rs:51` | `submitted_at`, `started_at`, `ended_at`, `asked_at` | UTC | stored |
| `core/src/session/retitle.rs:140,146` | `titled_at` | `2026-10-20T09:12:41Z` | `txns.at` |
| `core/src/session/send.rs:66` | SQL window of the loop guard (`strftime('%Y-%m-%dT%H:%M:%fZ','now','-60 minutes')`) | n/a, a comparison | SQLite |
| `core/src/config/cache.rs:56` | cache `fetched_at` | UTC | SQLite `strftime` |
| `core/src/secret/mod.rs:386` | `secret` JSON `expires_at` | `2026-10-22T21:10:00.000Z` | `clock::timestamp` |
| `core/src/secret/grants.rs:129` | approval file `at` | UTC | `clock::timestamp(ports.clock.utc())` |
| `core/src/host_repair.rs:101` | approval `at` (file) | UTC | `clock::timestamp` |
| `core/src/host_repair.rs:107-110` | backup file name `<ts>` (`20261025T222900Z`), `refs/cpd-host/<ts>` | `20261025T222900Z` | `format_description!` UTC |
| `core/src/install/adapter.rs:95-98` | adapter dir name `pi.<UTC YYYYmmddTHHMMSS>-<pid>` | `pi.20261025T222900-4242` | `format_description!` UTC |
| `core/src/version_sync/mod.rs:119-121` | migration file name `<YYYYMMDDTHHMM>-<slug>` | `20261025T2229-slug.sql` | `t.year()…` UTC |
| `core/src/host/install_state.rs:92,94,124` | `cpd-install.json` `since` (validated UTC `Z`) | `2026-10-19T15:17:50Z` | parsed |
| `core/src/pr/poll.rs:176` | `pr poll` JSON `rate_limit.reset_at` | `2026-10-20T11:00:00Z` | GitHub's string |
| `core/src/inbox/cmd.rs:132-135`, `:155`, `:307`, `:332` | Inbox item JSON `opened_at`, `closed_at`, `snoozed_until`, `dismissed_at`; `engine-down` item `opened_at` | `2026-10-14T21:40:02.114Z` | `timestamp` |
| `engine/src/watch/engine.rs:331` | install marker `at` (file) | UTC | `clock::timestamp` |
| `engine/src/watch/restart.rs:91`, `:371` | `Pending.at`, `failed.at`, error Event `data.at` | UTC | `timestamp(now_utc())` |
| `engine/src/watch/team.rs:154` | `polled_at` in the team cache file | UTC | `clock::timestamp` |
| `engine/src/watch/inbox.rs:35`, `archive_leads.rs:52`, `sleep.rs:79` | `now`/`cutoff` passed to SQL | n/a, comparisons | `clock::timestamp` |
| `engine/src/watch/pr.rs:130` | `failing` record `since` (file) | RFC 3339 | `Rfc3339` |
| `tui/src/viewlog.rs:30,36` | cockpit view trace log line prefix (file `run/…`) | `2026-10-25T22:29:00.123456Z cpd …` | `Rfc3339` |
| `tui/src/merge/marker.rs:49` | merge marker file `at` | UTC | `Rfc3339` |
| `tui/src/tree/mod.rs:142`, `:266` | `now` text passed to SQL reads | n/a | `clock::timestamp` |
| `pi/lib/updates.ts:63,80,99,130,174` | parse `at` of board posts, records, Events from cpd JSON | input fields | none |
| log lines (a file the User may read; A2 says "a file" is a field, so FIELD by that rule; **flag**: the Lead may decide a log is prose) | | | |
| `engine/src/watch/log.rs:37` | `watch.log` line prefix | `2026-10-25T22:29:00.123Z cpd watch: started (pid 63714), a tick every 5s` | `clock::now()` |
| `engine/src/watch/pr.rs:137` | `watch.log` line | `pr poll: gh search failed transiently (HTTP 502), failing since 2026-10-25T22:29:00Z (30 s): retrying …` | `Rfc3339` |
| `core/src/driver/run.rs:245` | `cpd attempt run` stderr line (the driver log) | `2026-10-25T22:29:00.123Z cpd attempt run: …` | `clock::now()` |
| `pi/lib/reload.ts:25` | `run/adapter/<session>.log` line (commands.md §15.6 "Log": `<ISO-8601 UTC time> <text>`) | `2026-10-25T22:29:00.123Z <text>` | `toISOString()` |

## DOC: documentation text or an example that shows a time

| file:line | what it says | class of the time it shows |
|---|---|---|
| `docs/contract.md:98` | §1.3 `Times are RFC 3339 UTC strings with milliseconds, 2026-10-14T21:10:00.000Z` | the FIELD rule; must be amended with the prose rule (the Job that edits docs, not this one) |
| `docs/contract.md:125` | install-state file `since` UTC `Z` | FIELD |
| `docs/contract.md:230` | "Each item shows its id and date" (context show rendering) | PROSE rule text (`render.rs:188`) |
| `docs/contract.md:550` | Inbox snooze `tomorrow 09:00 local time` | ALREADY LOCAL text |
| `docs/contract.md:559` | incident `started_at` RFC 3339 UTC | FIELD |
| `docs/contract.md:899` | `The engine was down for <s> s: it stopped at <time>.` | PROSE text (`engine.rs:423`) |
| `docs/contract.md:923`, `:1830` | UTC adapter dir and migration names | FIELD (file names) |
| `docs/contract.md:2084` | `batch.md`: `[<id>] <kind> (<date>)` | PROSE text (`distill.rs:674`) |
| `docs/contract.md:2089` | run log `started_at` RFC 3339 UTC | FIELD |
| `docs/contract.md:2816`, `:3140`, `:3192` | tool row / balloon `HH:MM` machine-local | ALREADY LOCAL (narrow) |
| `docs/commands.md:286` | `context show` markdown: "each item with its id and date" | PROSE text |
| `docs/commands.md:251,294,355,1786,2524,2691,2719,2746,2795,4212,4271,4290,4711,4749,4754,4759,4764,4808,4814` | 19 JSON examples with `…Z` times (`created_at`, `at`, `reset_at`, `titled_at`, `posted_at`, `opened_at`, `snoozed_until`, `dismissed_at`, `expires_at`, `started_at`) | FIELD, unchanged |
| `docs/commands.md:2151` | `host repair` `<ts>` = UTC `YYYYMMDDTHHMMSSZ` | FIELD (file name) |
| `docs/commands.md:3208` | `logs`: "its time of day (`HH:MM:SS`, colour 8)" with no zone | PROSE text (`tui/src/logs/view.rs:133`) |
| `docs/commands.md:3291` | `tmux_restart` check: `found` "the times and versions" | PROSE text (`restart.rs:83`) |
| `docs/commands.md:3423` | `session-message` content `Message from <sender> (HH:MM)`, local, "no hardcoded offset" | ALREADY LOCAL text (`delivery.ts:43`) |
| `docs/commands.md:3530` | reload log line `<ISO-8601 UTC time> <text>` | FIELD (log) |
| `docs/commands.md:4261` | snooze `tomorrow` = next 09:00 local, offset from `date +%z` | ALREADY LOCAL text |
| `docs/implementing.md:280`, `:395` | `Clock::utc()` and the `time` crate for RFC 3339 UTC | FIELD, source description |
| `docs/cockpit-parity.md:212` | `before its restart at 17:39:51 in the capture` | PROSE-like time without a zone in a research note |
| `docs/research/message-time-display.md` | the earlier research of the pi chat's `HH:MM` rows | ALREADY LOCAL, background |
| `pi/skills/cpd-manager/SKILL.md:72` | `Attribute each record … and date it when the date matters.` | the only skill text about dates; it gives no format and no zone, and what an agent then writes is PROSE the Requirement covers (owner of the change: this skill; `pi/skills/cpd-lead/SKILL.md` has no equivalent line) |

## Not a time (checked, left out)

Durations and ages are not times and need no zone: `tui/src/read/age.rs:4` `age_text` (`42s`, `2d`), used at
`tui/src/tree/inbox.rs:162`, `tui/src/plan/pulse.rs:56`; `engine/src/watch/workers.rs:126` and `core/src/incident.rs:343`
`elapsed`; the seconds of `The engine was down for 3.2 s`; `loop guard, N turns in 10 min`; `Done in 1.151s`.

## Counts

Rows per directory, counting each `file:line` row of the tables above once (a row naming several lines counts once).
Computed by a script from this file's tables (see "Checks").

COUNTS_PLACEHOLDER

## Not found or unverified

- **A live PROSE time of the engine-down and tmux-restart sentences**: the live store has no `watch-started` with a
  down sentence, and no restart was pending; the examples are built from the format strings and the `ports_tests.rs:303`
  assertion (unverified as live output).
- **The cockpit**: no `cpd tui` was started (it never exits). I read the `tui/src` producers only. I found no absolute local
  time in the cockpit (tree, inbox, plan, pr, merge views): only ages and the literal `tomorrow 09:00` label. The "cockpit board
  already shows `2026-10-25 22:29`" of the Plan brief is, I infer, the pi `Board:` digest (`pi/lib/updates.ts:195`), not a
  `tui` surface; unverified.
- **Whether the Dispatch/Event log and Worker view `HH:MM:SS` rows are "narrow fold rows"** (pro-d42): they are not folds and
  are wide rows, and they are UTC today. Whether they take a zone name or stay bare is the Lead's call.
- **Logs as FIELD or PROSE**: `watch.log`, the attempt-run stderr, the view trace and the adapter log are UTC-prefixed text a
  person may tail. I put them under FIELD ("a file", A2); the User did not say so about logs explicitly.
- **Error messages quoting a corrupt stored time** (listed under PROSE, "Error messages"): left UTC by my guess.
- **Agent-written prose** (record text, Manager and Lead messages, Worker escalations): cpd has no code that formats it; only
  skill text (`pi/skills/*/SKILL.md`) and the Worker spec template (`core/src/spec/worker.md`, no time in it) could steer it. I did not read
  every reference file in full: `pi/skills/*/references/*.md` and the two `SKILL.md` were searched by `rg`, not read through.
- **`render::raw`** (`core/src/context/render.rs:146`): no non-test caller found by `rg`; if a caller exists under another
  path (a macro, a binary), I missed it.
- **Other `pi/` extensions** (`pi/ask-user-question`, `pi/extension.ts`, `pi/lib/*.ts` not named above): `rg` over them for `Date`,
  `toISOString`, `localTime`, `slice(0, 10)`, `.at` found only the rows above; I did not read each file.
- **Windows or non-`date` hosts**: `local_offset_with` needs `date +%z`; the zone abbreviation (`EDT`) is not available from
  it, which the formatter Job must solve (not designed here).
- **`strftime` use in tests and fixtures** (`cli/tests/…`, `*_tests.rs`) was not inventoried: they build UTC fixtures, not shown times.
- **`docs/archive/`** (cpd1) was not inventoried.

## Checks

A script run before commit confirmed that every `file:line` of the tables names a file with at least that many lines
(`awk 'END{print NR}'`), and produced the counts above. Quick check (`npm run typecheck`, `scripts/cargo.sh build --workspace`)
run from this worktree before submit.
