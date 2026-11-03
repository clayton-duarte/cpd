# Skills audit: cpd-manager

## Coverage

- Baseline: `git merge-base HEAD origin/main` = `a954761dfe6e7b6316f955ada638e363c170af55`.
- Read in full: `pi/skills/cpd-manager/SKILL.md` lines 1-173 (`wc -lc`: 173 lines, 23343 bytes; the Spec said 23098 bytes); `pi/skills/writing-for-agents/SKILL.md` 1-81; `pi/skills/writing-for-agents/SKILL-MECHANICS.md` 1-22.
- `pi/skills/cpd-manager/references/` holds only `.gitkeep` (`ls references`: empty apart from the dotfile).
- Verified counts: `grep -oi never SKILL.md | wc -l` = 37; `description` = 67 words (`sed -n 3p | wc -w`, minus the `description:` label); `## Rules` has 17 top-level bullets (lines 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 59).
- Cross-references checked: `commands.md` §24, §24.1, §15.17, §9.11, §9.12 exist as headings (`grep -n '^###* ' docs/commands.md`); `contract.md:1205` has the E6 row.
- Not read: other files, per the Spec. M7 is not covered.

## Manager findings M1 to M6

| M-id | Verdict | Verified numbers and lines | Finding ID |
|---|---|---|---|
| M1 | partly | The description is 67 words, not 48 (line 3). It restates the opening paragraph (line 8) almost word for word. The role-invocation branch is stated twice in line 3 ("when your first message says you are the Manager..." and "or when you continue a Session of role manager"). The "one trigger per branch" claim holds, the word count does not. | MG-1 |
| M2 | partly | Confirmed: `## 2. Plans` starts at line 99 (not 97) and the Manager's Plans steps sit under about 98 lines of preamble, tables and Rules. `## Rules` has 17 bullets (lines 39-59). The host section is `## 5` at lines 169-173 (not 172-176). The MCP paragraph is line 69 (not 68-70). Secrets is line 44 (not 39-41). Bug protocol is lines 54-58 (not 51-55). The "§4 notes it is applied by the cpd Manager" claim is line 152. Refuted detail: the "other roles' workflows" are one paragraph (line 23), not a block. | MG-2, MG-3, MG-4 |
| M3 | partly | Messaging a Lead (`cpd session send <context>:<plan>`) appears at lines 50, 62, 122, 134, 136, 145, 154, 159, 160 and 163 (grep). The M3 bullets are Rules 11 and 17 = lines 49 and 59 by count, but `session send` is in Rules 12 (line 50) and 17 (line 62). `context ask` is at lines 45, 60, 85 and 93, not 144/163 (those lines hold no `context ask`). `plan new` is at lines 49, 61, 95, 109, 110. `context pending` is at lines 50, 104, 121, 123, 132. "Never answer for the User" is line 40 and line 167; step 7 (line 114) says "Do not relay", so it is not a third copy. Confirmed in substance; the exact lines and counts differ. | MG-5, MG-6, MG-7 |
| M4 | partly | "never" appears 37 times (not 36) in 173 lines (not 176). Rules lines 47-48 name 7 + 14 = 21 forbidden commands, not 15: line 47 has 7, line 48 has 8 plus 6 archive commands. | MG-8 |
| M5 | confirmed | Raw citations: `pro-d204` (lines 53, 54, 152), `pro-d226; commands.md §24.1` (69), `pro-d305` (153), `contract §3.7 E6` (171), `§9.11, commands.md §9.11-9.12` (173). Section numbers without a path: `commands.md §24` and `§15.17` do carry the file name (line 44); `contract §3.7 E6` has no file path or extension (line 171). Lead-only commands are at lines 122 and 126-127 (`gh pr edit`, `cpd workflow add`, `cpd pr push`, `cpd pr rebase`). | MG-9, MG-10 |
| M6 | confirmed | Step order is 1 Intake (101), 2 Interview (102), 3 Split and agree the cut (103), 4 Check pending work (104), 5 Brief (105), 6 Start (106). Step 4 says "Before you start a Plan or route an issue", yet it follows the interview and cut. | MG-11 |

## Findings

| ID | Lens | Where | Kind | Quote | Evidence | Proposed fix |
|---|---|---|---|---|---|---|
| MG-12 | 1 | pi/skills/cpd-manager/SKILL.md:45 | conflict | Never use `cpd context ask`: it records an open question in the Context and asks nobody. | Line 85 and line 93 present `ask` as a record type to write; `cpd context ask --help` ("Record a question (§3.4)") shows the command exists. | Say once whether the Manager may write `ask` records; drop it from the record table and line 85, or drop the ban. |
| MG-14 | 4 | pi/skills/cpd-manager/SKILL.md:53 | conflict | never ad-hoc prose | Axiom "Prose is not enforcement": lines 53-58 state the rules (bug protocol, "all steps required") as prose instead of a hook or command. | Move enforcement of the bug protocol into a `cpd` check; keep one line here. |
| MG-1 | 3 | pi/skills/cpd-manager/SKILL.md:3 | bloat | the higher-level Lead that holds the Context's records and Requirements at the Epic level | M1: 67-word description repeats line 8; the trigger is stated twice. | Cut the description to one leading trigger: "Use when you are the Manager of a Context." |
| MG-2 | 3 | pi/skills/cpd-manager/SKILL.md:99 | bloat | ## 2. Plans | M2: heading at line 99 after Rules (39-63) and Policies (65-69); the Plans steps are the primary tier. | Move the Plans steps above the Rules and the role tables. |
| MG-3 | 3 | pi/skills/cpd-manager/SKILL.md:169 | bloat | ## 5. The host Manager | M2: lines 169-173 apply only to the `host` Context; the file loads them for every Manager. | Move to `references/host-manager.md` behind a one-line pointer for `--context host`. |
| MG-4 | 3 | pi/skills/cpd-manager/SKILL.md:69 | bloat | A Worker on the durable runner is told with a message that starts `cpd adapter:` | M2: line 69 spends half its length on what a Worker does; lines 44 (secrets) and 54-58 (bug protocol) are branch-specific too. | Disclose the MCP, secrets and bug-protocol text into `references/`; keep one pointer line each. |
| MG-5 | 1 | pi/skills/cpd-manager/SKILL.md:159 | bloat | cpd session send <context>:<plan> "<text>" --unblocks "<reason>" | M3: `grep -n 'cpd session send'` gives lines 50, 62, 122, 134, 136, 145, 154, 159, 160 (and 163 by text). Syntax stated at 62 and 145 and 159. | Keep one syntax block in `## 4`; elsewhere write "message the Lead ([4](#4-messaging-a-lead))". |
| MG-6 | 1 | pi/skills/cpd-manager/SKILL.md:61 | bloat | cpd plan new <id> --context <c> --title "<title>" --brief-file <file> | M3: `plan new` syntax at lines 61, 109 and 110; the brief rule is in lines 105 and 113. | Keep the command only in `## 2` step 6; reference it from Rules. |
| MG-7 | 1 | pi/skills/cpd-manager/SKILL.md:132 | bloat | **The turn-start message.** | M3: the turn-start message and `context pending` are at lines 50, 104, 121, 123 and 132. "Never answer for the User" is lines 40 and 167. | One home for the turn-start message (`## 3`); pointers elsewhere. |
| MG-9 | 3 | pi/skills/cpd-manager/SKILL.md:171 | bloat | (contract §3.7 E6) | M5: raw citations at lines 53, 54, 69, 152, 153, 171, 173. | Drop decision ids from instructions; keep the reason in a record. |
| MG-10 | 3 | pi/skills/cpd-manager/SKILL.md:126 | bloat | `gh pr edit <n> --base <branch>` | M5: lines 126-127 instruct `gh pr edit`, `cpd workflow add`, `cpd pr push`, `cpd pr rebase`, all Lead-only, in the Manager's file. | Reduce to "Tell the Lead the landing order and the branch to build on"; drop the Lead's commands. |
| MG-13 | 1 | pi/skills/cpd-manager/SKILL.md:41 | bloat | Never poll, sleep or run a foreground loop. | Line 42 ("**Never block.** No sleep, polling loop or long wait") says the same. | Merge lines 41 and 42 into one rule. |
| MG-15 | 1 | pi/skills/cpd-manager/SKILL.md:152 | bloat | The `cpd` Manager applies the bug report protocol (pro-d204). | The protocol is lines 54-58; this line plus line 53 repeat the pro-d204 pointer. | Keep the protocol in one place; the `cpd` Manager's own skill is the home. |
| MG-8 | 3 | pi/skills/cpd-manager/SKILL.md:47 | style | Never run what belongs to a Lead: `cpd plan phase plan`, `cpd workflow add`, `cpd job add` | M4: 37 "never" (`grep -oi never SKILL.md \| wc -l`); lines 47-48 list 21 forbidden commands, and `agent_forbidden` is already refused by the adapter (docs/commands.md:122). | State the positive target once ("act only through the commands listed in this file; message the Lead for everything else") and drop the list. |
| MG-11 | 3 | pi/skills/cpd-manager/SKILL.md:104 | style | **Check pending work first.** | M6: step 4 follows Interview (102) and Split (103), though it says "first". | Move to step 1, or fold into Intake. |
| MG-16 | 3 | pi/skills/cpd-manager/SKILL.md:73 | style | Your first message is the header and the pointer line: | The fenced block at lines 74-76 is indented 3 spaces inside a paragraph; line 17-18 has a double blank line. | Dedent the fence and remove the stray blank line. |

## Considered, not reported

- M2 "17-bullet Rules dump": 17 is correct (verified), but a flat Rules list is "a fine arrangement" for reference in the rubric; reported only for the buried steps (MG-2).
- Lead guess "37 uses of `never`": confirmed (37), it is not a finding on its own.
- `§N` anchors (`#2-plans`, `#1-records`, `#3-knowing-where-each-lead-is`) resolve to existing headings; no dead anchor.
- `contract.md:1194` says "E6 is removed" while line 1205 still has the E6 row and line 171 cites it: docs inconsistency, outside this file; unverified which one is current.
- Line 85 "nobody can answer yet" versus line 93 "the User has not settled yet": folded into MG-12.
- Step 7 (line 114) as a third "never answer for the User": refuted, it forbids relaying, a different rule.
