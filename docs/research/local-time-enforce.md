# Holding cpd's code and its agents to local time: deterministic enforcement

Plan `local-time`, Job `research-enforce` (Attempt 1). Research only: no check implemented, no file changed but this one.
Read on the Lead Branch (HEAD `26fed547` when I started), 2026-10-26.

The rule (Context records pro-d41, pro-d42; accepted assumptions A2, A3): a time in a **field** stays ISO 8601 UTC with `Z`
(contract §1.3); a time in **prose** for a person is machine-local with the zone named (`2026-10-25 23:03 EDT`); narrow
`HH:MM` fold rows stay bare; tests mock the zone. This file does not inventory the surfaces or design the zone lookup
(`docs/research/local-time-inventory.md` and another Job do).

**Labels.** A claim is verified when I read the cited source or ran the command; everything else says `unverified`, `guess:`
or `I infer`. pi paths are relative to
`~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent/` (`PI`).

## 0. Verdict

1. **Layer 1 (cpd's code): types first, a corpus test beside them.** A UTC newtype with no `Display` makes `format!("…{}", ts)` a
   compile error for what the types own; a test that runs every text-producing command under a mocked zone and a fake clock
   and refuses a UTC time in prose catches what the types cannot (a stored or GitHub-supplied string printed as is). A
   source-scan test with an allowlist is the cheap stop-gap. clippy `disallowed_*` is the weakest fit: the Gate does not run
   clippy.
2. **Layer 2 (the agents' text): the cpd-side refusal first, the turn-end reminder second.** The six text-writing commands
   (`context decide|note|ask|answer`, `board post`, `session send`, `ask`) are one validation point for every agent and every
   harness. The turn-end reminder reuses the existing `agent_end` machinery for the text the commands never see. A
   tool-result rewrite is rejected (it rewrites fields, against A2). A `tool_call` refusal on `bash` duplicates the cpd-side
   refusal with a worse parser.
3. One detector, written twice (a Rust scanner without a regex crate, a TS regex), with one shared table of accept/refuse
   cases (§5).

## 1. Evidence: agents copy UTC into prose

`grep -rnE '[0-9]:[0-9]{2}(:[0-9]{2})?(\.[0-9]+)?Z\b'` over the docs, ran 2026-10-26: `docs/contract.md` 4 lines,
`docs/commands.md` 19 lines (many are field examples, not classified), `docs/research/*.md` 95 lines in 28 of 127 files,
`pi/skills/cpd-lead/SKILL.md` and `pi/skills/cpd-manager/SKILL.md` 0 lines (`grep -rnE '[0-9]Z' pi/skills` printed nothing).
So the skills do not teach UTC; the agents that wrote the docs used it unprompted. Real cases in prose, quoted:

1. `docs/contract.md:1868`: "the incident of 2026-10-19 18:14Z, where it held `locks/store.lock` …"
2. `docs/contract.md:2148`: "r4 of pr-links was requested on 2026-10-19 at 13:20Z; r5 was requested 63 minutes later"
3. `docs/contract.md:3420`: "evidence: 2026-10-20 16:43:13Z, E5 raised an engine error at once"
4. `docs/research/binaries.md:22`: "#13 (1237d94) merged 22:01:53Z, reverted by #14 (969a977) at 22:02:58Z"
5. `docs/research/records-recency.md:102`: "'hs has no tmux 3.8' no longer holds: it was installed at 00:55Z"

Limit: these are documents agents wrote, not chat turns or refused tool calls. I found **no log of a chat turn or a record that
quotes UTC** (I did not search the session logs: unverified that chat text shows the same rate). cpd's own prose with UTC is
inventoried in `docs/research/local-time-inventory.md` (for example `engine/src/watch/engine.rs:423`, `core/src/inbox/cmd.rs:311`).

## 2. Layer 1: cpd's own code

Facts. `core/src/clock.rs:10` `timestamp()` returns a `String` formatted UTC with `Z`; `clock.rs:17` `now()` returns the same
as `String`; `clock::Clock::utc()` (`clock.rs:26`) returns an `OffsetDateTime`. `rg 'clock::timestamp|clock::now|format_description' core engine cli tui --include=*.rs`
(minus `_tests` files) finds 65 lines: the producers and the sites. A stored time is a `String` all the way (`restart.rs:91`
prints `self.at` as is, per the inventory). `local_offset_with` (`core/src/inbox/cmd.rs:255`) reads the offset through the
mockable `Runner` (`date +%z`), the one existing mock point for the zone. `Cargo.toml:9-10` says "Later Jobs may not add any
[dependency]": **no `regex` crate** is available to Rust checks without a decision.

### (a) A type split: UTC type without `Display`, a people-facing formatter

- **How.** `clock::timestamp` and `now` return `Utc` (a newtype over the string; `Serialize`, `as_str()` for storage and JSON,
  no `Display`, no `Deref<Target=str>`). Prose is built from `LocalText` / `local::format(Utc, &Zone) -> Local`
  (`Local` has `Display`). Then `format!("stopped at {}", ts)` does not compile. Axiom: make illegal states unrepresentable.
- **Catches.** Every value that goes through the new type: all 65 producer sites and everything derived from them.
- **Misses.** Any time that enters as a plain `String`: a stored column read back (`txns.at`, `Event.at`: `String`, `core/src/events/select.rs:16`),
  GitHub's `reset_at` (`engine/src/watch/pr.rs:246`, `{reset_at}` is GitHub's string, per the inventory), `as_str()` used
  inside `format!` on purpose, a hand-typed `"…Z"` literal. Closing those means typing the store columns too (large).
- **False positives.** None at compile time; a legitimate field use needs `as_str()`, which is explicit.
- **Cost.** About 65 sites to touch, the store/JSON modules to adapt; no new dependency; a refactor, not a check. I infer
  it is the largest of the Layer 1 options (not measured).
- **Relies on.** `time` crate (already a dependency, `Cargo.toml` `time = { version = "0.3", … }`); no pi.

### (b) clippy `disallowed_methods` / `disallowed_types`

- **How.** A `clippy.toml` with `disallowed-methods = [{ path = "cpd_core::clock::timestamp", reason = … }]`; the storage and
  JSON modules opt out with `#[allow(clippy::disallowed_methods)]`.
- **State of the repo (verified).** No `clippy.toml`, no `.cargo/` directory, no `[lints]` in `Cargo.toml`. `scripts/cargo.sh clippy --version`
  prints `clippy 0.1.99` on this machine, but `docs/implementing.md:408` says "rustfmt and clippy are not installed" for Workers
  and the Gate, and the Gate's steps (`package.json` `verify:steps`: `check-migrations-frozen.sh`, `check-portable.sh`,
  `typecheck`, `npm test`, `pi-check.sh`, `cargo nextest run --workspace`) **run no clippy**. Using it means adding a Gate
  step (a decision about the Gate, outside this Job) and Gate hosts with clippy (unverified on them).
- **Catches.** Calls to the UTC formatter outside the allowed modules. Same coverage as (a) for the producers, with less
  safety, and it does not see a `String` that was stored or received.
- **Misses.** Everything (a) misses; `allow` attributes spread as the escape hatch; `disallowed_methods` cannot tell
  "a field" from "prose" at one call site, only the module.
- **False positives.** Every storage write must carry an `allow`: noisy (the 65 sites).
- **Cost.** One file plus the `allow`s, plus a Gate step. Dependency: clippy on the Gate hosts.
- **Verdict.** Dominated by (a) (same call sites, weaker, needs a Gate change). Not recommended.

### (c) An architecture test that scans sources

- **How.** A `#[test]` (or one `node --test` file) reads the `.rs` files with `std::fs` and fails on `clock::timestamp(`,
  `clock::now()`, `format_description!` outside an allowlist of paths (store, JSON output, `clock.rs`, logs), one allowlist
  line per debt with a reason. Precedents in the repo: `pi/test/structure.test.ts:9-18` reads every `pi/lib` source and asserts on it;
  `pi/test/no-fixed-sleeps.test.ts` is a static check for TypeScript and Rust test code (`docs/implementing.md:231`,
  "The static sleep check"); `scripts/tripwire/allow.txt` is the allowlist shape (`docs/implementing.md:230`).
- **Catches.** A new direct call of the UTC formatter outside the allowed modules, at once and with a message naming the file.
- **Misses.** A `String` that holds a time from storage or GitHub; a call through a new helper that wraps the formatter (the
  helper must be in the list); a UTC time built with another formatter (`to_offset(UTC).format(…)`, `Rfc3339` as in
  `tui/src/viewlog.rs:30`, per the inventory). It sees syntax, not meaning.
- **False positives.** Low and visible: a call in a module that is a field producer (fix: allowlist line).
- **Cost.** One test file (about 40 lines), no dependency, no Gate change (`npm test` and `nextest` already run in the Gate).
  Cheapest of the Layer 1 options that works today.
- **Relies on.** `std::fs`; for the TypeScript side `node:fs` like `structure.test.ts`.

### (d) A contract test under a mocked zone and a fake clock

- **How.** Run every command that produces prose (the surfaces of the inventory: `context show` markdown, error `message`,
  Event `text`, session and board digests, Inbox labels, `doctor` text, bare `cpd`) in a store whose times come from a
  `ManualClock` (`core/src/clock.rs`, fixed epoch `2026-01-01T00:00:00Z`) and the **mocked** zone (A3: the zone enters through
  `Ports.runner`, a `FakeRunner`, `core/src/ports.rs:18-22,62-65`; `local_offset_with` is the existing example). The test feeds every
  text output (not the `--json` fields) to the shared detector (§5) and asserts there is no match; and a second assertion that
  the local rendering (`2025-12-31 19:00 EST` for the epoch under a mocked `-0500`) appears where a time is expected.
  Fields are excluded by looking only at the text-valued members the contract names (`message`, `text`, markdown bodies).
- **Catches.** Any time that reaches a person, whatever its type, including stored strings and GitHub's `reset_at`. This is the
  only Layer 1 option that checks the *output*.
- **Misses.** A surface no test exercises (a new command with no test); a state the fixture does not reach (a downtime sentence,
  a rate limit). Needs fixtures per surface, so coverage equals the number of surfaces in the inventory.
- **False positives.** A time quoted by design in a message from a field (an Event whose text quotes a user-given `…Z`
  string, a record id like `hs-d123`): the detector refuses only a full time or `HH:MM` followed by `Z`/`UTC`/`GMT`, so ids and
  versions do not match (§5, cases verified with `node`).
- **Cost.** A harness plus one case per surface (the inventory lists the rows: tables in `local-time-inventory.md`
  lines 37-42 and 96-136 are the starting list). No dependency. The CLI tests run the built `cpd` as a subprocess
  (`cli/tests/…`); there the mocked zone has to enter as a fake `date` first on `PATH` (the Gate's tripwire already puts fake
  programs first on `PATH`, `scripts/tripwire/`; I infer a fake `date` fits it, unverified).
- **Relies on.** The `Clock` and `Runner` mocks already in `core`.

### (e) Golden outputs

- **How.** Snapshot files of the outputs under the mocked zone and clock; the test compares bytes.
- **Catches.** Any change of a covered output, including a UTC regression, and also every unrelated wording change.
- **Misses.** Anything not snapshotted; it says "changed", not "is UTC".
- **False positives.** High: every wording edit breaks a golden; a golden updated without reading blesses a UTC time.
  `Tests are the proof`: a test pins the contract, not the implementation; this pins the implementation.
- **Cost.** Snapshot files in the repo; no dependency. Not recommended as the enforcement; (d) states the contract directly.

### TypeScript side (pi/lib and the adapter)

- **Facts.** `pi/lib/time.ts` `localTime` / `localDateTime` format with `Date` local methods (verified, `time.ts:11-27`);
  `pi/lib/reload.ts:25` writes `toISOString()` into the adapter log (inventory). There is no eslint in `package.json` (verified:
  only `typescript`, `@types/*`, `@sinonjs/fake-timers`, pi packages as `devDependencies`); `tsc --noEmit` is the
  `typecheck` step.
- **Type.** A branded type `UtcIso = string & { readonly __utc: unique symbol }` returned by the parser of Event `at`; the prose
  formatter accepts `UtcIso` and returns `LocalText`; the message builders (`pi/lib/delivery.ts`, `render.ts`) take `LocalText`
  only. Catches a `UtcIso` interpolated by mistake only if template literals are type-checked: **they are not**; TypeScript
  allows `${x}` for any type (verified by language rule, not run here). So a brand does not stop interpolation; it only forces
  the author through the formatter at the conversion points. Weak.
- **eslint.** `no-restricted-syntax` on `toISOString()` / `new Date(...).toISOString` would need eslint, which is not a
  dependency: new dependency, an extra Gate step. Cost > benefit for the few sites in `pi/lib`.
- **Test.** A source-scan test next to `pi/test/structure.test.ts` (same technique as (c)): fail on `toISOString()` in
  `pi/lib` outside an allowlist (`reload.ts` for the log field). Plus unit tests of the formatters with `TZ` set for the
  test process (A3 forbids reading the real zone; the test sets `process.env.TZ`, which is authored input, and the Node
  `Date` reads it: I infer it is a mock in the sense of A3, but the tz database is Node's, so the Lead must confirm; unverified).

## 3. Layer 2: the agents' own text

### What pi lets an extension do (read from the 1.3.0 sources)

| Point | Can it inspect / block / rewrite | Source |
|---|---|---|
| `tool_call` | inspect the arguments; **block** with `{ block, reason }`; **mutate `event.input` in place** | `docs/extensions.md:106`; `dist/core/extensions/types.d.ts:1089-1098` (`ToolCallEventResult`: "To modify arguments, mutate `event.input` in place") |
| `tool_result` | inspect and **replace** `content`, `details`, `isError`, and `structuredContent` (omitting it drops it); handlers compose | `types.d.ts:1114-1120`; `docs/extensions.md:106,149` |
| assistant final text, `message_end` | inspect and **replace the finalized message**, same role required; a handler that returns another role is an error and ignored | `docs/extensions.md:106`; `dist/core/extensions/runner.js:864-885`; `types.d.ts:1121-1124` |
| assistant final text, `agent_end` | inspect only (`messages`); notification | `types.d.ts:748-751`; `agent-session.js:888-889` |
| `turn_end` / `agent_before_settle` | inspect; append `custom`, `custom_message`, `context_edit` entries; return `continue: true` for **one** next model request | `docs/extensions.md:118`; `types.d.ts:792-799,828-835` |
| `before_agent_start` | add one `message` (`customType`, `content`, `display`) to the run that starts | `types.d.ts` `BeforeAgentStartEventResult`; `docs/extensions.md:104` |

Also verified: a `tool_call` handler that throws blocks the tool (`docs/extensions.md:263`); tool calls of one message may run in
parallel (`docs/extensions.md:124`); `pi.on` **returns an unsubscribe function** in 1.3.0 (`docs/extensions.md:97`;
`types.d.ts:1218,1224`), while `pi/lib/turns.ts:68,82` and `pi/lib/adapter.ts:58` say it has none: those comments are stale or
refer to cpd's narrowed `PiApi` (`pi/lib/pi.ts:17-18` picks `"on"` from pi's `ExtensionAPI`, so the return is typed). I did not run pi.

### The existing turn-end `cpd reminder`

- **Where.** Spec: `docs/commands.md:3618` §15.8. Pure checks: `pi/lib/reminder.ts` (`talksOfFutureWork` `:36`, `needsReminder` `:61`,
  `lastAssistantText(messages)` `:68`, `REMINDER_TEXT` `:7`). Wiring: `pi/lib/turns.ts` `registerReminderCheck` (`:83`), called at
  `pi/lib/adapter.ts:33`; it is the `agent_end` handler (`turns.ts:84`) that takes `lastAssistantText(event.messages)`, runs the
  check and sets `r.reminderDue` (`pi/lib/run.ts:51-52`); `registerReminder` (`turns.ts:71-81`) is a `before_agent_start`
  handler registered the first time a reminder is due, returning a hidden message (`customType: "cpd-reminder"`, `display: false`).
- **How it detects.** A fixed list of substrings (`reminder.ts:9-28`) plus the pattern `\bafter\b[^\n.]{1,60}?\bmerges?\b`
  (`:31`), then "names none of the Plan's keys" from `cpd plan show`. It fires for role `lead` only (`turns.ts:86`) and at most once.
- **Reuse for UTC.** Same plumbing, different predicate: in the same `agent_end` handler call `utcInProse(text)` (§5) on the same
  `lastAssistantText`; if it matches, set a due flag with its own text ("cpd reminder: you wrote a UTC time; state times in the
  machine's local zone with its name, e.g. 2026-10-25 23:03 EDT"). Needed changes: (1) the role filter `r.info.role !== "lead"`
  (`turns.ts:86`) would have to include Managers and Workers' roles as chosen; (2) `reminderDue` (a boolean, `run.ts:52`) becomes a
  list of reasons, so a future-work and a UTC reminder can both be due; (3) a new `customType` for the renderer list if shown
  (hidden messages have no renderer, `docs/commands.md:3649`). No network call is needed for the UTC check (the future-work check
  calls `cpd plan show`; this one does not). The reminder reaches the agent on its **next** run, as it does now: a UTC time in a
  final message is already in the chat the User reads (it is a nudge, not a prevention).

### Candidate designs

**L2-1. Tool-result rewrite of cpd output into local time before the agent sees it.**
- **How.** A `tool_result` handler on `bash` whose input starts with `cpd`: rewrite every ISO UTC time in `content`
  into the local form. Returns `content` (and `structuredContent` if any; bash has none, `types.d.ts:985-1000`).
- **Catches.** Every time the agent reads from cpd, so it cannot copy UTC from cpd output (but still from its memory and its docs).
- **Misses / cost.** The output of `cpd … --json` carries fields (`at`, `created_at`, `reset_at`) that are **meant** to be UTC
  (A2, contract §1.3). Rewriting them makes the agent see values that disagree with the store and the contract, and breaks an
  agent that copies `at` into `--since`-type arguments or `jq`s a field (I infer, unverified: which flags take times). Telling
  field from prose needs a JSON parse of the output and then a walk by key name: a table of prose keys to maintain, equal to the
  inventory. The agent's own later UTC quote is still not caught.
- **False positives.** High: every JSON field, every record id that embeds a time-like token is safe, but any quoted log line is not.
- **New code / dependency.** One handler plus a JSON walker in `pi/lib`; the zone (the adapter would need the zone lookup).
- **Guards only for proven bad behaviour.** Not a guard: it changes data, so it falls under *Explicit over implicit* rather than that axiom;
  it is rejected here for the field cost, not the axiom.
- **Verdict.** Reject. It fixes the symptom at the wrong layer: cpd's prose should be local at the source (Layer 1).

**L2-2. Tool-call refusal for the text-writing cpd commands.**
- **How.** In the single `pi.on("tool_call", …)` handler (`pi/lib/adapter.ts:59`, which already refuses `bash` calls: `:82-91`), for
  `toolName === "bash"` and a command that starts a text-writing cpd command, test the argument text with the detector (§5) and
  return `{ block: true, reason }` naming the local form. Same pattern as `askguard.ts` (`tooLong`, `adapter.ts` calls it).
- **Catches.** A UTC time the agent is about to record or send: the six commands, in a Lead, Manager or Worker, before the write.
- **Misses.** Text written with `--file` (the file's content is not in the command; `session send --file`, `commands.md:2462`);
  text built by `$(…)`; the final chat message; a cpd run through `codemode` or a nested call (those do go through
  `tool_call`: `docs/extensions.md:149`, but the arguments are a script).
- **False positives.** The command line is shell text: a time inside a quoted JSON field value or a code span in the text
  being recorded (for example a record that explains the `Z` rule). Mitigated by the code-span exemption (§5), not removed:
  a `bash` string is not Markdown and the heredoc quoting makes backticks ambiguous (I infer: this is why a refusal on the bash string
  parses worse than one on the argument).
- **New code / dependency.** One pure module like `askguard.ts` plus a shell word splitter (cpd's other bash guards, for example
  `noblock.ts`, already read the command string; their parser is reusable: `adapter.ts:83-90`). No dependency.
- **Guards only for proven bad behaviour.** It is a guard on the agent's raw `bash` tool. §1 proves agents wrote UTC into
  documents; it does **not** show the refusal of a chat command, so by the axiom's letter the evidence is indirect. I judge it
  not yet met for `bash`; the same check inside cpd (L2-3) is not a raw-tool guard.

**L2-3. cpd-side refusal in the same commands.**
- **How.** `context decide|note|ask|answer`, `board post`, `session send`, `ask` already validate their text (the board's limit
  `TEXT_MAX_CHARS = 2000`, `core/src/board.rs:15`; the error table is `docs/commands.md:112` for `supersedes_invalid`). Add one more
  validation at the same place: text holding a UTC time is refused with a new error code (name: not chosen here) whose message
  says the local form, exit non-zero, nothing written.
- **Catches.** Every caller, whatever harness: pi, a script, a future adapter; including `--file` text, because cpd reads the
  file. Covers the Event and record text that later become digests (so the next agent reads local time).
- **Misses.** The final chat text and any command outside the list. A deliberate quote of a UTC time (a log line pasted into a
  record) is refused too unless it sits in a code span.
- **False positives.** The same as L2-2, but the text is a plain argument (no shell), so the code-span exemption is exact.
  Needs a way for a legitimate quote: code span, as in `` `2026-10-26T03:05:11.907Z` ``.
- **New code / dependency.** A scanner in `core` implementing §5 **without** `regex` (`Cargo.toml:9-10`: no new dependencies),
  one new error code in contract §3, tests of each command. Fits *Validate at the boundary, fail loud*.
- **Guards only for proven bad behaviour.** Not a guard on an agent's tool: cpd's own input validation; the axiom limits guards
  on raw tools. The evidence of §1 (agents write UTC into prose that cpd stores and shows) justifies it.
- **Verdict.** Recommended; the one check that holds for every agent and survives a harness change.

**L2-4. Turn-end reminder when the assistant text holds a UTC time.**
- **How.** As described under "The existing turn-end `cpd reminder`": `agent_end` → `lastAssistantText` → `utcInProse` → one hidden
  message at the next `before_agent_start`.
- **Catches.** The text the agent says to the User or to a Lead; the only option that sees the final chat message. Deterministic.
- **Misses.** It does not prevent the message (it lands after the User saw it); a run that ends without text; it needs the
  next run to correct anything; `message_end` could prevent but rewrites the agent's words (below).
- **False positives.** A quoted field value outside a code span (an agent pasting `"at":"2026-10-19T15:17:50.426Z"` in plain text)
  gets a nudge; the cost of a false positive is one hidden line the agent reads once. Low harm: the reminder never blocks.
- **New code / dependency.** About 20 lines in `reminder.ts` plus the flag changes above; no dependency. Reuses a tested path
  (`pi/test/reminder.test.ts`).
- **Guards only for proven bad behaviour.** A reminder is not a guard on a tool (it blocks nothing); §1 is the evidence.
- **Alternatives within pi.** `message_end` can **replace** the final message before it is kept (`runner.js:864`): rewriting the
  agent's UTC into local text would need the zone in the adapter and would change what the agent said without telling it:
  rejected (*Explicit over implicit*; unverified whether the replacement is also what the model sees on the next turn, I did
  not read `agent-session.js:925` against the persistence at `:756`). `turn_end` / `agent_before_settle` with `continue: true`
  could make the agent restate its message in the same run (`docs/extensions.md:118`); it spends a model request on each hit and
  needs a loop guard ("Guard continuation conditions"); a possible later step, not now.

## 4. Ranked recommendation

Order: works, then cheap, then good.

| # | Check | Works | Cheap | Good |
|---|---|---|---|---|
| 1 | **L2-3** cpd refuses UTC time in the text of the six commands | holds for every agent, catches before the write | one scanner and one error code, no dependency | one boundary, loud failure |
| 2 | **(d)** contract test: text outputs under mocked zone and clock have no UTC time | checks the real output incl. stored strings | harness plus one case per surface | pins the contract, not the code |
| 3 | **L2-4** turn-end reminder, reusing `agent_end` | the only check on the final chat text | ~20 lines on tested plumbing | nudge, not prevention |
| 4 | **(c)** source-scan test with an allowlist (Rust and `pi/lib`) | stops a new direct call of the formatter | one test file each | syntax only |
| 5 | **(a)** type split `Utc` / `LocalText` | compile-time for what it owns | 65 sites, a refactor | best design, misses stored strings |
| – | L2-2 `bash` refusal | duplicates #1 | parser of a shell string | evidence indirect: wait |
| – | (b) clippy, (e) golden | – | needs a Gate step / high churn | – |
| – | L2-1 tool-result rewrite | rewrites fields | – | against A2 |

(a) ranks below the tests only because it protects less (§2) and costs most; it is the end state if the Lead wants illegality
unrepresentable and the store columns are typed in the same Plan (the Lead decides).

## 5. The detector: exact patterns and cases

One definition, two implementations: JS `RegExp` for `pi/lib` (L2-4, and L2-2 if later), and a hand-written scanner in Rust
(no `regex` crate) for L2-3 and test (d). A text is **refused** when, **after removing fenced code blocks and inline code
spans**, either pattern matches. The removal is `` text.replace(/```[\s\S]*?```/g, "").replace(/`[^`\n]*`/g, "") ``.

```js
// a full date and time in UTC: Z, "UTC", "GMT", or a +00:00 / +0000 offset
const FULL  = /(?<![\w.:-])\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d{1,9})?)?(?:Z|[ ]?(?:UTC|GMT)(?![+-]\d)|[+-]00:?00)(?!\w)/;
// a bare time of day marked UTC: 13:20Z, 22:01:53Z, 10:15 UTC
const SHORT = /(?<![\w.:-])(?:[01]?\d|2[0-3]):[0-5]\d(?::[0-5]\d(?:\.\d{1,9})?)?[ ]?(?:Z|(?:UTC|GMT)(?![+-]\d))(?!\w)/;
```

Rust scanner (no regex): scan for `\d{1,2}:\d{2}` (and `\d{4}-\d{2}-\d{2}` before it), then optional `:\d{2}` and `.\d{1,9}`, then
optional one space, then `Z`, `UTC` or `GMT` (not followed by `+`/`-` digit), or `+00:00` / `-00:00` / `+0000` after a full date;
the match must not touch a word character (and for the left side not `.`, `:`, `-`) on either side. I did not write or run
the Rust scanner: the JS patterns above were run with `node` on the cases below; the Rust scanner is unverified.

**Verified with `node` (2026-10-26), all as listed:**

| Refuse | Why |
|---|---|
| `2026-10-19 18:14Z` | the §1 case 1: date, time, `Z` |
| `13:20Z` | bare time of day with `Z` |
| `at 02:01:40Z` | with seconds |
| `stopped at 2026-10-25T22:29:00.000Z.` | the engine's real text shape |
| `at 22:29 UTC` | the word UTC |
| `2026-10-25T22:29:00+00:00` | UTC offset spelt out |
| `10:15:07 GMT` | GMT |

| Accept | Why |
|---|---|
| `2026-10-25 23:03 EDT` | the required form |
| `Message from x (23:03)` | the narrow bare `HH:MM` row |
| `hs-d123 at 11:04` | record id; a time without a zone mark |
| `Version 1.2.3`, `Run 39130172286`, `port 8080:3000` | numbers that are not times |
| `version 12:30 PM` | a time with a non-UTC mark |
| `` `2026-10-26T03:05:11.907Z` is the field `` | a field value in a code span |
| `2026-10-25T23:03:00-04:00` | a non-UTC offset |
| `see zone UTC` | the word alone, no time |
| `2026-10-25 23:03 UTC-4` | a time given as a UTC offset of the local zone |

Known gaps of the patterns (no run): `22:29 utc` in lower case is accepted; `2026-10-25T22:29Z` inside a fenced block is accepted
by design; a time spelt with a month name is out of scope. The reminder (L2-4) and L2-3 must use the same table in their tests
(one shared fixture file would be the DRY form; the Rust and TS tests cannot import each other: unverified how to share, guess: a
JSON file read by both test suites).

## 6. Unverified, for the Lead

- That the Gate hosts have clippy (only this machine, `clippy 0.1.99`).
- That a fake `date` on `PATH` fits the CLI tests' tripwire (§2 (d)).
- That `process.env.TZ` set by a TS test counts as a mock under A3 (the tz data is Node's).
- That `message_end` replacement is what the model sees next turn (§3 L2-4).
- The share of chat text (not documents) that holds UTC: no session log was read.
- The Rust scanner; only the JS patterns were run.
