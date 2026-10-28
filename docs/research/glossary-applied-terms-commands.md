# Applied: terms in docs/commands.md (Job terms-commands, Attempt 1)

Line numbers are after the edit. Check that an old quote is gone: `rg -n -i -F '<old quote>' docs/commands.md` prints nothing.

| Where | Row | Applied? | Old quote -> new text | Reason when not applied |
|---|---|---|---|---|
| docs/commands.md:4253 | 1 | Applied | `(was "Workflow")` -> `(cpd 1's name for Plan)` | |
| docs/commands.md:3430 | 2 | Not applied | `the phase is `deploy`, not `pr`` | Claim false: the text already says the phase is `deploy`; no `pr` phase is named |
| docs/commands.md:1739 | 3 | Applied | `with `ci`, cpd's default` -> `with `none`, cpd's default (`core/src/policy/default.toml`)` | |
| docs/commands.md | 4, 5 | Not applied | contract rows | Not docs/commands.md: another Job's |
| docs/commands.md:1600 | 6 | Not applied | `pr.gate` | Claim false in this file: `rg -n 'pr\.gate' docs/commands.md` prints nothing; it already says `gate.pr` |
| docs/commands.md:3283, :3285 | 7 | Applied (sense) | `The tiers are named by their colours` -> `The urgency levels are named by their colours`; table header `Tier (colour)` -> `Urgency (colour)` | The literal 'tier names' is not in the file; the Inbox-colour sense is. Other Inbox 'tier' uses are code identifiers: see Deferred |
| docs/commands.md | 8 | Not applied | contract:2740 | Not docs/commands.md: another Job's |
| docs/commands.md:1076, :1079, :1580, :1600, :2107, :2109, :2140, :2147, :2148, :3081 | 9 | Applied | `workflow worktree` -> `Lead worktree` | |
| docs/commands.md:9 | Flow | Applied | `Context (Grounding, Clarifying, Specifying), Plan (Dividing ... Dispatching) and Deploy (...)` -> the three stages and steps of CONTEXT.md (Grounding, Grilling; Clarifying ... Orchestrating; Documenting ... Deploying) | |
| docs/commands.md:1739 | Flow | Applied | `the Mechanical checks step of the Deploy stage` -> `belongs to the Deploying step of the Dispatch stage` | |

## Deferred to the rename

Every `Deploy` hit of `rg -n -w 'Deploy' docs/commands.md` other than :1739 and :9 is the cockpit's third column, pane title or pill as the code draws it today, or the Inbox kind `deploy`: :1831, :2314, :3181, :3398, :3407, :4303, :4313, :4321, :4345, :4367, :4372, :4377, :4379, :4462.

- Event kinds, the phase literal `deploy`, the `[deploys]` domain and the Inbox kind `deploy`: unchanged.
- Inbox 'tier' (urgency) uses that name code: `palette::tier_span`, "tier glyphs", "tier colour", "dot-urgency tier" (:3283, :3291, :4407, :4409-4412). Not changed: unverified whether the code renames them.
- Gate value `ci` where the file names it as a Gate value, not as the default (:1773, :1783, :2278): the policy default is `none` (default.toml:5-7), so these say a value the default file does not use. Not changed: outside the rows given; needs a decision.
- Lines that call a Workflow a "PR" (`pr add`, `--pr`, `prs`, `pr_needs`, `pr-cancelled`, `pr-started`): code words today, unchanged.
