# Manager patterns: the acting Manager's session (2e66cebd)

Research for hs-d252 item 3. Job `manager-patterns`, Plan `manager`. Findings only: no skill, code or record was changed.

Citation forms: `#N` is the 0-based entry index in the session file (line N+1 of the JSONL; the file has entries `#0`..`#852`). `hs-…` is a record of Context `building-cpd`. Times are UTC, from the file. A claim I could not tie to an entry or a record is marked **unverified**.

## Method

**Read.**

| Source | How |
|---|---|
| Session `2e66cebd-d059-46d9-87c8-1d09f4638084` (`~/.pi/agent/sessions/--home-clayton-projects-cpd--/2026-10-15T20-18-53-736Z_2e66cebd-….jsonl`, read-only) | A streaming `python3` script (under `timeout 120`) wrote one compact line per entry to a temp file: index, time, role, text (tool calls cut at 1200 characters, results at 400). I read all 68 `user` entries in full, the assistant text around every correction, and the full text of the entries a pattern rests on (`#662`, `#664`, `#803`, `#804`, `#831`–`#835`, and others) straight from the file. |
| Context `building-cpd` | `cpd context show --context building-cpd` (606 lines of markdown: plans, decisions, superseded decisions, questions, notes). Superseded pairs were the main lead to User corrections. |
| The current skill | `pi/skills/cpd-manager/SKILL.md` (88 lines), to tell what is already fixed. |
| Cross-checks | `docs/commands.md` §3.11 (`context pending`), `docs/research/unplanned.md` (gap-audit, snapshot 2026-10-18 05:53Z), and the session's own retro `~/first-manager-retro.md` (written at `#841`). The retro was used only as a list of leads: each pattern below was re-found in the session file. |

**Counts.**

- The file spans 2026-10-15 16:57Z to 2026-10-19 00:53Z: 853 entries: 402 assistant, 317 tool results, 68 user, 30 custom messages (adapter and `cpd-context` digests), 6 system, 30 others (plannotator state, session info, one compaction `#851`).
- `#0`–`#267` are the Lead's own work on `switch-over` (10-15). `#268` onwards (10-16 13:24Z) is cross-Plan coordination. The Manager role was named by the User at `#381` and recorded as hs-d83..hs-d93 (`#388`); from then the session is the acting Manager by convention (hs-d93, hs-d207).
- Assistant tool calls (regex over the commands): 271 `bash`, 36 `ask_user_question`, 40 `cpd context decide`, 16 `cpd context note`, 3 `cpd context ask`, 29 `cpd plan list`, 20 `cpd plan new`, 3 `cpd context pending`. 16 assistant messages ask the User to paste a message into a Lead (`#329`, `#343`, `#396`, `#418`, `#422`, `#436`, `#440`, `#462`, `#470`, `#494`, `#524`, `#610`, `#656`, `#743`, `#765`, `#773`).
- User corrections traced to the Manager's own routing or reporting: 7 (hs-d81, hs-d111, hs-d114/hs-d132, hs-d159, hs-d206, hs-d207, hs-d208) plus 2 verbal ones (`#407`, `#651`); wrong-id supersessions by the Manager: 3 (hs-d162→hs-d163, hs-d214→hs-d216, hs-n109).

**Limits.**

- I skimmed `#0`–`#120` (the forked parent's trial Lead work) and read tool results only in the cut form, except where a pattern needed the full text.
- The session was the *acting* Manager running the `cpd-lead` skill; the `cpd-manager` skill (`#786`, read at 13:12Z on 10-18) arrived only for its last day. Mistakes about rules the Manager skill states today may have had no rule to break at the time; each pattern says which.
- Costs are what the User said or did in the file; I did not measure time.

## Patterns

Numbered in the order I found them. Section Proposals orders them by User attention saved.

### P1. Work routed or started without reading the current state of the destination and of work in flight

| Instance | Entries, records | What happened |
|---|---|---|
| Dot fix and Context updates sent to a Plan that had finished | `#329` (sent to `hardening` at 15:31Z), `#363`–`#368`, hs-d80 → hs-d81 | Plan `hardening` was done (PR #8 merged, 16 Jobs). The Manager found out at 17:29Z and said why: "I hadn't run `cpd plan list` since early on" (`#368`). The User had to answer a correction dialog (`#364`). |
| Cockpit visuals routed to `tree-menu` while `tui-visuals` (started by the User) edited the same `tui/` | `#396` (hs-d94), `#416`–`#422` (hs-d101..hs-d103), `#426`–`#436`, hs-d111 | Four items were routed to `tree-menu`; at `#428` the Manager asks the User "tree-menu's follow-up list holds most of the other cockpit visuals I routed there today. Both edit tui/. Who owns which?" The User moved all of them (hs-d111); the Manager wrote two new paste messages (`#436`). |
| Fix owner for "a Plan with no PR is missing from the tree" | `#460`–`#462` (hs-n54: pasted to `cross-repo`, a Plan that was done), `#494`, `#502`, hs-d114 → hs-d132 | The owner changed three times in two days (`cross-repo`, `tui-visuals` by hs-d114, `tree-menu` by hs-d132). At `#494` (10-17 21:57Z) the Manager still says "no owner yet", which contradicts hs-d114 (dated 10-16). That it had not read hs-d114 is **unverified**: its reads at `#492`/`#500` used `--since 202`/`--since 210`. |
| Debug Plan started for a bug in-flight work already fixed | `#631`–`#651`, hs-n86, hs-d159 | Plan `pr-view-sync` was started although `tui-visuals` had 15 Jobs on its Lead Branch rewriting those views (landed as #20). The Manager had listed that pending work an hour earlier (`#656`). The User: "archived it, it fixed itself after pending work landed. something you overlooked" (`#651`). |

**Cost.** Four User corrections, each a dialog or a message of his; three paste messages rewritten; one Lead started and archived for nothing; `manager` itself missing from the tree for more than a day (`#447`–`#451`, `#494`).

**Likely cause.**

- Skill §2 step 4 ("Check pending work first") was added after hs-d159 and covers *starting a Plan or routing an issue* against pending Jobs. It says nothing about the owner's phase (done, archived, Lead stopped), about naming an owner in a record or brief (hs-d94, hs-d102, hs-d114 were records), or about reading records from the last seen id instead of from memory.
- `cpd context pending` does list Jobs `merged` on a Lead Branch and not on the base (`docs/commands.md` §3.11), so the hs-d159 case is now answerable by one command.

### P2. Reporting a Plan's state from an earlier read

| Instance | Entries | What happened |
|---|---|---|
| Plan states quoted from memory | `#368` | "I hadn't run `cpd plan list` since early on, so I didn't know that `hardening`, `tree-menu`, `dotfiles-sync` and `drop-hermes` had all finished." |
| `cpd-ci` reported as active after it merged | `#396` (18:17Z), `#406`, `#407`, `#408`–`#410` | The status quoted came from a `plan list` three minutes before PR #10 merged (18:20Z); the Manager repeated it at 18:37Z. The User: "cpd ci? that was merged" (`#407`). The Manager: "This keeps happening: my picture of the other Plans is only as fresh as my last `plan list`" (`#410`). |
| "No owner" against a record | `#494` | see P1, row 3 (unverified cause). |

**Cost.** The User caught it twice and had to spend a message each time.
**Likely cause.** No rule to read before reporting. The turn-start digest (hs-d86, hs-d154) now covers part of it, but the digest only reaches a turn's start; a long turn of tool calls still reports from its own earlier reads.

### P3. The User as the courier between Manager and Leads

16 assistant messages end with "paste this into the `<x>` Lead" (list in Method). The User asked "can't you message the Leads?" three times (`#401`, `#437`, `#740`) and pasted: "posted" (`#405`), "posted the messages to the leads" (`#463`). Each outstanding message was also a record-reading instruction ("run `cpd context show --since 120`"), because Leads saw no new records (`#329`).

**Cost.** One manual relay per decision, for two days (10-16 to 10-18).
**Cause.** A missing command, not a missing rule: `cpd session send` (hs-d92 A4) shipped with #30 at 13:07Z on 10-18 (`#784`, `#792`). The current skill documents it (§4). **No skill change needed.** At `#440` the Manager offered `tmux send-keys` as a stopgap only under three rules and with the User's approval; he chose to paste.

### P4. A broadcast that woke every idle Lead

At `#803`–`#804` (13:24Z, 10-18) the Manager ran 8 `cpd session send` calls in one command: to the official Manager `97de2471`, to `manager`, `pi-pin`, `cross-repo`, `inbox`, `pr-links`, `quality` and `dev-speed`. Under hs-d148 a send starts a turn in an idle Lead. Four of the eight messages were one text ("check your PR now … rebase if it conflicts", `rebase.md`) that nobody had asked for; another asked `dev-speed` for a proposal the User had not requested. At 13:29Z the User reported: "every lead is getting a context message when I navigate to them, that triggers a new turn … the lead gets confused and looses the current thread. this needs urgent fixing" (`#810`). The Manager traced it to its own sends plus the digest (`#811`–`#813`, hs-n145: "The acting Manager's 13:24Z send to 7 Leads started a turn in each idle one") and the User decided hs-d209 (send waits; `--wake` explicit; Leads read the Context on demand).

It had read the help text at `#777`–`#779`, which says "An idle Lead starts a turn with my message", and wrote "I message Leads directly from now on" (`#792`) without a rule on what is worth a message. A second instance of the same class: the message to `manager` (`#804`) woke it, which the Manager calls "the one use I'd still make of waking" (`#821`).

**Cost.** "Urgent": every Lead the User opened that afternoon started a turn and lost its thread (`#810`); Plan `manager` had to build PR `passive` (hs-d209).
**Cause.** cpd behaviour (hs-d148) times a missing habit: the skill's §4 and the "act only when…" rule do not say what a message is for. hs-d209 made the default safe; what is left is the content and the fan-out.

### P5. Citing record ids before the records existed

| Instance | Entries | What happened |
|---|---|---|
| Epics | `#662`, `#664`–`#666` | One call wrote hs-d162 (naming notes `hs-n87`/`hs-n88`/`hs-n89`) and the three notes in the same call. Other Leads had written records meanwhile; the notes were hs-n94/95/96. The Manager superseded it: hs-d162 → hs-d163 ("pointed at the wrong record ids because other Leads wrote records in between", `#666`). |
| Plan `inbox` brief | `#683`–`#687` | The brief named the split decision `hs-d169`; its real id was hs-d180. The Manager wrote a correction note (hs-n109) and told the User (`#687`: "One mistake on my side"). |
| Handoff | `#831`–`#835` | The first handoff record (hs-d214) pointed at "hs-n150" as the handover note; the real one was hs-n151. Superseded by hs-d216 (`#833`). |

**Cost.** Three extra records in a Context whose size is already a problem (hs-n111, hs-d182), and one Lead briefed with a wrong id (it had to rely on a correction note). The User's attention: small.
**Cause.** The skill's Records section says nothing about when an id is known. Ids come from the write; the Manager cited them in the same shell call or from memory.

### P6. Saying "started" or "harmless" without reading the state of what was started

| Instance | Entries, records | What happened |
|---|---|---|
| New Lead died 4 s after `plan new` | `#687`, `#688`, `#703`–`#709` (hs-n111) | At 04:24:36Z the Manager told the User "Plan `inbox` started with its Lead: open `park-inbox`". The pane had died at 04:24:26Z (first message of 138 KB over Linux's 128 KiB argument limit). The User: "I dont see the new lead" (`#688`). |
| Same cause, second Lead | `#713`–`#717` | Plan `argv-fix`'s Lead also started `stopped`; found only by `cpd session show` after the fact. |
| Install that ended six Lead panes | `#774`–`#779`, `#795`–`#807` (hs-n141, hs-d212, hs-d213) | At 13:10:57Z, 24 s after the install, the Manager called the new `store_too_new` message "the usual harmless one" after checking its own follower and `cpd doctor`. The install had stopped six Leads' panes (hs-d212: "the 13:10Z install ended six Lead panes instead of reloading them"). The Manager listed Lead states at 13:23Z (`#795`, four `stopped`) and re-ran `plan new` for each (`#799`–`#801`): 13 minutes. |

Of the five install checks before that (`#424`, `#512`, `#526`, `#554`, `#596`) none listed the Leads' states except `#596`, after hs-n81.

**Cost.** The User found the missing Lead himself (`#688`); four to six Leads were down for 13 minutes without anyone knowing.
**Cause.** The skill's §2 step 6 reports the Lead's creation and says "tell the User the Plan id". Nothing says to read the Lead's state afterwards. The turn-start digest carries "Plan created, phase change, PR opened…" but no line for a Lead that stopped (list in skill §3; checked against `#686`–`#687`: the Manager had no event to wake on). Whether a check right after `plan new` would have shown `stopped` (4 s) is **unverified**; the check later did (`#689`, `#715`).

### P7. Acting outside cpd commands

Acting Manager, running the `cpd-lead` skill, where hs-d15/hs-d16 allowed git and install steps on request and hs-d108 later allowed Leads `git` and `gh`. The current Manager skill forbids `tmux`, `wt`, worktree and Ledger access and limits the Manager to `cpd` commands.

| Instance | Entries |
|---|---|
| `tmux` reads of panes and views | `#368` ("I read its tmux pane to check that it had started … I shouldn't have"), `#452`, `#554`, `#596`, `#640`, `#642` (`tmux -L cpd capture-pane` of the Dispatch column) |
| `git merge` in the checkout and `install-cpd.sh` | `#516`–`#518` (fast-forward `~/projects/cpd`, install main), `#608` |
| `gh pr close` and `gh pr view` | `#508`, `#522` |
| Editing the Ledger's run directory | `#709`, `#715` (`run/<plan>/lead-message.md` cut by hand; the User chose it at `#705`–`#707`) |
| `sleep` in a command | `#520`, `#711`, `#713`, `#717`, `#801` |

**Cost.** No harm found in the file. The risk is the one the skill names (touching state cpd owns), and the facts above show why it happened: the Manager had no cpd command for "which binary does a view run", "is this Lead's pane alive", or "install main".
**Likely cause.** Missing commands plus a skill that forbids without saying who does these things (the User, or the Lead on request, hs-d16). A rule alone will not stop it ("prose is not enforcement").

### P8. Work decided but held only in records or in chat

| Instance | Entries, records | What happened |
|---|---|---|
| Context distillation | `#744`–`#759`, `#829`, hs-d182, hs-d192, hs-n150, hs-d218 | The User asked "did we plan that" (`#744`). Answer: "No, it's recorded but not planned. No Plan or Lead owns it yet" (`#747`). The Manager then routed it to `cross-repo` by a record and a pasted message (`#757`–`#759`). At `#829` the User says it "was not planned yet"; the handover note (hs-n150) records the mismatch with `cross-repo`'s queued PR `distill` (6 Jobs, `#792`); the User settled it in hs-d218. |
| Lists of unowned work | `#436` (`hardening`'s teardown fixes, hs-n35: "should I start that Plan, or nudge its Lead?"), `#494`/`#502` ("Planned work that no Plan has started": hs-n35, hs-n38, hs-n39, hs-d90, hs-d91, hs-n54, hs-n61, hs-n65, hs-n60, hs-n52, hs-n67, hs-q3) | The work existed only as notes and as the Manager's chat list. |
| The audit Job that this produced | hs-d204; `docs/research/unplanned.md` (snapshot 2026-10-18 05:53Z) | 357 records classed: BUILT 140, OWNED 77 (10 of them "record only, no Job"), MISSING 18, NO WORK 122. |
| The User's rule | hs-d239, hs-d241 | "Every piece of work a Lead intends is registered in cpd the moment it is planned … Enforcement (the skill rule and a guard) goes to Plan manager"; "(cpd-lead registers at once; cpd-manager checks)". |

**Cost.** A question from the User, a whole Worker Job to find the gaps (`gap-audit`), and one handoff that had to flag a contradiction (hs-n150).
**Likely cause.** The skill's Rules ask for records first (hs-d84) but not for *ownership*: a decision that implies work can be complete as a record while no Plan, PR or draft Job carries it. Draft Jobs (hs-d179, hs-d241) only exist since 10-18.

### P9. Role boundary and handoff gaps

- **`session open`/`fork` as the User's.** The Manager read the new skill at `#786` and wrote "I'll stop and leave opening Leads to you" (`#792`); the User corrected it (`#793`, hs-d206). The skill file in this repo already carries the correction (Rules: "You may create Leads and open sessions").
- **Two Managers on one Context.** Manager `97de2471` started at 13:12Z and recorded hs-n139 ("hs-d93's convention ends, so Leads message the Manager"); the User corrected it (`#793`, hs-d207) and the acting Manager wrote to `97de2471` to stand by (`#803`, `mgr.md`). The skill says "one live per Context" but nothing about a second Manager acting while another is acting by convention. One-off: the convention is gone with the handoff (hs-d216).

**Cost.** One correction dialog each. **Cause.** Skill text out of date (fixed) and a transition that will not recur (**unverified** that it will not: any future handoff passes through the same state).

## Proposals

Ordered by User attention saved, most first. Each quotes the smallest change to `pi/skills/cpd-manager/SKILL.md`. The quotes are proposals for the User's review; none was applied.

### 1. P1: check state at the point of naming an owner (saves the most: four corrections, one wasted Lead)

Replace §2 step 4:

```diff
-4. **Check pending work first.** Before you start a Plan or route an issue to a Lead, run `cpd context pending --context <c>` (`--markdown` to read it) and compare the issue with the title and touched files of every pending Job of every active Plan. When something in flight already fixes or changes it, tell the User which Plan and Job, and do not start a Plan.
+4. **Read the state first.** Before you start a Plan, route an issue, or name a Plan or Lead as an owner (in a message, a brief or a record), do all three in that turn: (a) `cpd context pending --context <c> --markdown`: compare the issue with the title and touched files of every pending Job of every active Plan (a `merged` Job is on a Lead Branch, not yet on the base); (b) `cpd plan show --plan <owner>` and `cpd session show <its lead>`: the owner's phase and its Lead's state, and whether another active Plan lists the same files; (c) `cpd context show --context <c> --since <the last record you read>`: records other Sessions wrote since. When something in flight already fixes or changes it, or the owner is done, archived or stopped, tell the User which Plan and Job and do not route or start.
```

### 2. P8: every decided piece of work has an owner in cpd (the User's own rule, hs-d241: "cpd-manager checks")

Add to Rules:

```diff
+- **Decided work has an owner in cpd.** When a decision, a note or a Lead's message names work, a Plan holds it before your turn ends: a Plan you start, or draft Jobs and PRs its Lead registered (`cpd job add --draft`, `cpd pr add --needs`; read them with `cpd context pending`). Work only in a record or in chat is unowned: say so to the User in that message, name the Plan you propose, and write a `note` saying who is to own it. When a Lead reports next steps without Job or PR keys, send it one message asking it to register them (hs-d239).
```

A guard (the turn-end reminder, hs-d241 (2)) is a cpd feature and already owned by Plan `manager`; this rule is only the Manager's half.

### 3. P6: read what you started, and treat a stopped Lead as stalled

Add to §2 step 6 after the `cpd plan new` block:

```diff
+After `cpd plan new`, and after any install of cpd, run `cpd session list --context <c> --role lead` (or `cpd session show <lead>`) before you tell the User a Lead runs: a Lead whose Session is `stopped` while its Plan is not done is not running. Rerun the same `cpd plan new` command to restart it, say so, and give the cause you read. Never call an adapter or install message harmless until the Leads' states agree.
```

A skill rule cannot fix the part where the Manager does not know to look: the digest has no line for it. The cpd feature needed (no design): a turn-start digest line for "a Lead of an active Plan became `stopped`" (hs-n141 stopped six at once; hs-n111 killed two new Leads in seconds). Whether a state read within seconds of `plan new` shows `stopped` is **unverified** (`#689`, `#715` read it minutes later).

### 4. P2: read before you report

Add to §3:

```diff
+- **Read before you report.** Before you tell the User a Plan's phase, a PR's state or what runs, read it in the same turn (`cpd plan list`, `cpd plan show`) and say when you read it. A table from an earlier turn is not a status.
```

### 5. P4: messages carry what the next turn needs, one reason each

Add to §4, after "Write the message so it stands alone.":

```diff
+Send a Lead only what its next turn needs, and only when the User or a record settled it. Do not fan one message out to several Leads (a status check, a rebase reminder) unless the User asked for that fan-out; if you do, say how many Leads it reaches. A reminder that a Lead can read itself in `cpd context pending` is not a message.
```

This also covers the `--wake` rule already in §4 ("sparingly"): the proposal adds fan-out and purpose, which hs-d209 left open.

### 6. P7: say who does what only a tool outside cpd can do (needs a User decision)

The facts do not settle whether the Manager may install main, close a superseded PR, or read a view. Two options for the User:

- **A (keep the ban, state the way out).** Add to Rules: "If you need something only `git`, `gh`, `tmux`, `/proc` or an install script can tell or do (which binary a view runs, installing main, closing a PR), do not run it: ask the User, or the owning Lead with `cpd session send`, and write a `note` naming the gap." The cpd features this points to (no design): a read-only report of the binary each cockpit view and follower runs and when it started (what `#452`, `#554`, `#596`, `#640` read with `tmux`/`/proc`); installing main and restarting stopped Leads through cpd. PR mergeability is already stored (`pull_requests.mergeable`, hs-d190), so `#508`/`#522`'s `gh` reads have a cpd source.
- **B (allow narrowly).** Mirror hs-d108 and hs-d16 for the Manager: read-only `git`/`gh` and the install script when the User asks.

I propose A: B repeats the exception the retro warns about and was never asked for. **Unverified** which the User prefers.

### 7. P5: cite an id only after reading it

Add to §1 after the record kinds table:

```diff
+Cite a record id only after you read it in the output of the command that wrote it (`record.id`). Write the record first, then the brief or message that names it. Never predict an id: other Sessions write records at the same time.
```

### 8. P9: a second Manager does not act until the User says (optional)

Add to Rules:

```diff
+- **One Manager acts.** If another Manager, or a Lead acting by convention, is already acting on the Context, do not act on it until the User says the handoff is made. When you hand over, write one `note`: the rules the User set, every Plan in flight with its next step, what is unplanned, the open questions, and the lessons; then message the new Manager its record ids (read from the output, see §1).
```

The handover that worked (`#831`–`#835`) is the model; the "do not act" half rests on hs-d207 alone. **Unverified** that it will recur, so optional.

### Not fixable by the skill

- **P3 (courier):** already fixed by `cpd session send` (#30). No change.
- **P6's detection gap and P7's missing commands:** cpd features named above.

## Keep

What the session did well, with evidence; the skill should keep (or already states) these.

1. **Records first, at once.** 40 `decide`, 16 `note`, 3 `ask` calls, attributed and dated, with `--supersedes` rather than edits (`#366`, `#432`, `#797`). The handover worked from records alone (`#831`–`#835`; hs-d84, hs-d216).
2. **Asking the User in short dialogs, recommended option first**, grouped by round (36 `ask_user_question` calls; `#376`, `#466`, `#670`–`#680`, `#755`). When the task was ambiguous it asked which meaning (`#755`: "Which handover do you mean?") instead of guessing.
3. **Labelling inference and checking code before claiming a cause.** "That's my inference; I haven't checked those code paths" (`#494`), "That's my inference from the timing" (`#656`), root causes read from `origin/main` (`#452`–`#462`, `#703`: the 128 KiB argument limit proved with `/bin/true`).
4. **Saying the mistake in the same turn and recording the correction** (`#368`, `#656`, `#687` with hs-n109).
5. **Briefs with a fixed shape**: the decisions table with record ids, verified facts with file and line or a command, out of scope, and the overlapping Plans to coordinate with (`#434`, `#648`, `#683`; `#687`: "I checked work in flight before starting it (hs-d159)").
6. **Refusing an unsafe stopgap without approval** (`#440`: `tmux send-keys` only on three rules and with the User's say-so) and answering a question without acting (`#401`, `#437`, `#740`: "A question is not an instruction").
7. **Evidence before a decision.** Load, memory and pressure measured before keeping 6 Workers (`#565`–`#573`; hs-d136).
8. **Idempotent restart:** re-running `cpd plan new` to restart a stopped Lead (`#799`–`#801`, hs-n111's workaround note at `#719`), which the skill already says is safe.
9. **A handover that names the lesson**: the Plans in flight and their next step, what is unplanned, the User's rules, and "I routed work without checking what was in flight" (`#835`, hs-n150, hs-n151).
10. **Not answering for the User:** Leads' questions went to the User (hs-d206); the Manager told Leads what a record or the User had settled and named it.
