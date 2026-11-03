# Skills audit: cpd-lead against cpd-manager and commands.md section 15.24

## Coverage

- Baseline: `git merge-base HEAD origin/main` = `a954761dfe6e7b6316f955ada638e363c170af55`.
- Read in full: `pi/skills/cpd-lead/SKILL.md` lines 1-451 (`wc -l` says 451, the Spec said 452); `pi/skills/cpd-manager/SKILL.md` lines 1-173; `docs/commands.md` section 15.24 (lines 5158-5175).
- Also checked (not audited): `cpd session send|job urgent|plan new|session rename|board post|session fork|session open --help`, `docs/commands.md` lines 180 and 205, the head of `pi/skills/skill.test.ts`.
- Internal anchors: a script listed every `](#slug)` link of both files against their headings. All resolve, so there is no anchor finding.
- Shared sentences: `/tmp/xs.py` (whitespace normalised, sentences of 12 or more words, lead line vs manager line). It listed 36 shared sentences, grouped below.

### Shared blocks (lead lines / manager lines, verdict)

| Block | Lead | Manager | Verdict |
|---|---|---|---|
| Epic/Context/Manager level table | 10-16 | 10-16 | word for word |
| Agent keys section | 26-36 | 25-35 | differs only in the `(you)` marker (on the Lead row in lead, on the Manager row in manager) |
| `ask_user_question` rule | 41 | 45 | differs: the core (G2, G4, state in the question, larger goes to a file) is word for word; the Lead adds `cpd_plan_review`, the Manager opening and the Lead's `cpd_plan_review` line are each one-sided |
| Reading a repo's latest main | 42 | 43 | word for word |
| `op` and secrets | 43 | 44 | word for word up to "or from a generator"; the Lead adds the Worker allowance text |
| Archive bans (`agent_forbidden`) | 44 | 48 | in meaning (same list; the Manager list also bans `context manager`) |
| Skills are for discovery (pro-d204) | 49 | 53 | word for word |
| Bug report protocol | 50-54 | 54-58 | differs: step 3 says `Workflow / Job` in lead and `Plan / Workflow / Job` in manager |
| Never block / 30 seconds | 57 | 42 | in meaning; the 30-second sentence is word for word, the rest differs |
| Policies paragraph | 68 | 67 | word for word |
| MCP servers paragraph | 70 | 69 | word for word |
| Context pointer line | 136 | 75 | word for word |
| Record commands | 170-176 | 81-87 | in meaning; the Manager adds `--context <c>` to each |
| Plan id pattern `^[a-z][a-z0-9-]{1,23}$` | 129 | 113 | word for word |
| Urgent Jobs | 300 | 136 | in meaning, different commands: the Lead `cpd job urgent <k>` (Plan from `CPD_PLAN`), the Manager `cpd job urgent <k> --plan <p>`; the Manager also says "instead of `cpd job dispatch`" |
| Communication rules, "Three ways" | 88-92 | 143-147 | differs in one place: the Message example key (`<context>` in lead 90, `<context>:<plan>` in manager 145); the Board and Record bullets are word for word |
| Lateral comms, four bullets | 96-99 | 151-154 | word for word (`diff` of lead 88-99 against manager 143-154 shows only the Message line) |
| Register planned work | 58 | 134 | differs: see XS-5 |

## Findings

| ID | Lens | Where | Kind | Quote | Evidence | Proposed fix |
|---|---|---|---|---|---|---|
| XS-1 | 2 | pi/skills/cpd-lead/SKILL.md:47 | wrong | `a Plan id (that Plan's Lead Session) or `<plan>.<prefix>` (a Session of that Plan by UUID prefix) as well as a full UUID` | docs/commands.md:5164: "no skill names them as an address". The line names a Plan id, `<plan>.<prefix>` and a UUID as addresses | Cut the line to the three agent keys; drop the Plan id, `<plan>.<prefix>` and UUID forms |
| XS-2 | 2 | pi/skills/cpd-manager/SKILL.md:134 | wrong | `a Job that `--needs` a `queued` or `running` Job` | docs/commands.md:205: `draft_parked` is for needs that chain to "a queued, running or failed Job"; lead line 58 says `queued`, `running` or `failed` | Add `failed` to the Manager list, or point to the Lead skill's list |
| XS-3 | 1 | pi/skills/cpd-lead/SKILL.md:241 | conflict | `and it handles the sequencing` | Manager line 122 says the Manager "moves the work or re-routes it, never sequences across Leads" | Say the Manager "moves the work or re-routes it" in both; one sentence in one place |
| XS-4 | 1 | pi/skills/cpd-lead/SKILL.md:53 | conflict | `Queue the follow-up root-cause fix (Workflow / Job)` | Manager line 57 says `(Plan / Workflow / Job)`; the same protocol (pro-d204) with two step 3 texts | One protocol text: the Manager one (it covers the Lead's case) or a shared reference |
| XS-5 | 1 | pi/skills/cpd-manager/SKILL.md:134 | bloat | `No planned work lives only in a Lead's chat` | Lead line 58 carries the whole rule of legal draft forms; the Manager copy restates a shortened version that already drifted (XS-2) | Keep the rule where the Lead acts on it; the Manager keeps one line: ask the Lead to register it |
| XS-6 | 1 | pi/skills/cpd-lead/SKILL.md:90 | bloat | `(`cpd session send <context> "<text>" --unblocks "<reason>"`)` | Lead 88-99 and manager 143-154 are the same block; `diff` of the two ranges prints only this line. A change in one is not made in the other | One source for the Communication rules (a `references/` file both skills link, or the Manager skill's block); a role-specific Message line stays in each |
| XS-7 | 1 | pi/skills/cpd-lead/SKILL.md:10 | bloat | `The levels, the cpd objects and their roles:` | Same 7-line table in manager line 10, word for word; the Agent keys section (lead 26-36, manager 25-35) states the same hierarchy again, so the table is a second statement of it | Keep the table in the Agent keys section of each skill, drop the first one |
| XS-8 | 1 | pi/skills/cpd-lead/SKILL.md:70 | bloat | `MCP servers: a cpd Session has no MCP server until you attach one` | Whole paragraph word for word with manager line 69; the Policies line (lead 68, manager 67) too | Move Policies and MCP to one shared source both skills link |
| XS-9 | 1 | pi/skills/cpd-lead/SKILL.md:43 | bloat | `Agents never call `op` directly; the adapter refuses it.` | Same secrets rule as manager line 44, word for word, 700+ characters | One shared source for the `op` and secrets rule; the Lead keeps its Worker-allowance sentence |
| XS-10 | 1 | pi/skills/cpd-manager/SKILL.md:136 | style | `instead of `cpd job dispatch`` | Manager line 48 lists `cpd job dispatch` as never run by the Manager, so the Manager is told to prefer a command it may not run | Drop the clause for the Manager |
| XS-11 | 1 | pi/skills/cpd-manager/SKILL.md:16 | style | Task | `sed -n '17,18p' pi/skills/cpd-manager/SKILL.md` prints two empty lines after the table ending on line 16 | Delete one blank line |

## Considered, not reported

- 15.24 requirement "`## Agent keys` before `## Rules`": met by both (lead 26 < 38, manager 25 < 37).
- Requirement "key shapes and `Manager > Lead > Worker` with `Context > Plan > Job`": met by both (lead 28-34, manager 27-33).
- Requirement "Manager key is `<context>`, never `<context>-manager`": met by both (lead 32, manager 31).
- Requirement "no skill tells an agent to rename or pass `--name` to `plan new`": met; `grep -n 'rename\|--name'` finds only manager line 35 ("never rename a session"); lead line 47 does not list `rename`.
- Requirement "neither skill names `<manager-id>` or `<lead-session>`": met by both (grep finds no such text). The Manager's `session open <id>` and `session fork <id>` (line 49) use a bare `<id>`: not the banned names, left alone.
- Requirement "Lead sends to its Manager with `--unblocks`, Manager to a Lead likewise": met (lead 81, 99; manager 50, 62, 154).
- Requirement "four lateral rules and the Board rule in each skill's Communication rules": met word for word (lead 96-99 and 91, manager 151-154 and 146).
- Requirement "every `cpd session send` code span has `--unblocks`": met; `grep -n 'cpd session send'` minus lines with `--unblocks` prints nothing for both files.
- Lead line 63 ("`cpd session fork` is for tangents the User starts") vs Manager line 49 (the Manager may fork): different roles, not a conflict; `cpd session fork --help` allows a Lead Session as source only. Lead line 174 and Manager line 85 show `cpd context ask` as a record command after a rule saying never to use it to ask the User: each says "(a record, not a question to the User)", so no conflict.
