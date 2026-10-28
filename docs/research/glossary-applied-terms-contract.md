# Glossary applied: docs/contract.md (Job `terms-contract`, attempt `terms-contract-a1`)

Line numbers are in docs/contract.md after the edit. The check for an applied row is `rg -n -F '<old quote>' docs/contract.md` printing nothing (except where the row says otherwise).

| Where | Row | Applied | Old quote → new text | Reason when not applied |
|---|---|---|---|---|
| contract.md:210 | 1 | applied | `(cpd 1's Workflow id)` → `(cpd 1's name for a Plan id)` | |
| contract.md:267 | 1 | applied | `This was cpd 1's Workflow (D3)` → `This is cpd 1's name for Plan (D3)` | |
| contract.md:860 | 1 | applied | `Workflow lead-fork's` → `Plan lead-fork's` | |
| contract.md:3271 | 1 | applied | `one per Workflow` → `one per Plan` | |
| contract.md:2454, :2554 | 1 | not applied | `cpd 1's \`workflow-done\`` kept | an old Event kind literal, not the bare term; the line says what was renamed to `plan-done` |
| contract.md:599, :2698 | 2 | applied | ``Plan in `dispatch` or `pr` `` → ``Plan in `dispatch` or `deploy` `` | |
| contract.md:3571, :3572 | 3 | applied | ``With `ci` (cpd's default)`` → ``With `none` (cpd's default, core/src/policy/default.toml)`` | |
| contract.md:3701, :3708, :3709, :3739 | 3 | applied | ``` `ci` is cpd's default``` and three more uses of `ci` as the Gate's value → `none` | read at core/src/policy/default.toml:5-7: `merge = "none"`, `pr = "none"` |
| contract.md:3571 | 4 | applied | ``| `pre-commit` | **the Gate** for `gate.merge` `` → ``**the quick check** for `gate.merge` `` | |
| contract.md:3671 | 5 | applied | `runs no quick check and no Gate` → `runs the quick check (the pre-commit hook, …) and no Gate (the Gate is the pre-merge hook…)` | |
| contract.md:3745, :4024 | 6 (contract.md part only) | applied | ``when `pr.gate` is `local` `` → ``when `gate.pr` is `local` ``; ``old `pr.gate = "ci"` `` → ``old `[pr] gate = "ci"` `` (the deprecated key of §8.3) | docs/commands.md and the skill are other Jobs' |
| contract.md:583 | 7 | applied | `the tier names are` → `the urgency names are` | |
| contract.md:2735 (audit line) | 7 | not applied | no `tier names` there: the quote is gone | place gone: `rg -n -F 'tier names' docs/contract.md` prints nothing |
| contract.md:2751 | 8 | applied | `the waiting Workers count` → `the ready-Jobs count` | |
| contract.md, 30 places (e.g. :1027, :3520, :3527, :3629) | 9 | applied | `workflow worktree` → `Lead worktree` (also `<workflow worktree>` → `<Lead worktree>`) | the audit says about 44; 30 existed in this tree: `rg -n -i 'workflow worktree' docs/contract.md` prints nothing |
| contract.md:761 | flow | applied | `Context, Plan and Deploy` → `Context, Plan and Dispatch` | |
| contract.md:1186-1215 (§3.9) | flow | applied | table with Deploy stage and old steps → three stages Context, Plan, Dispatch with the steps of pro-d146 | |
| contract.md:1211 | flow | applied | `Who does what at Deploy since` → `Who does what at Deploying since` | |
| contract.md:1213 | flow | applied | `Grounding, Clarifying and Specifying` → `Grounding and Grilling` (then Clarifying in `plan`) | |
| contract.md:1215 | flow | applied | `(Reviewing and Monitoring)` → `(pull request review, in Deploying)` | |

## Facts that disagree with CONTEXT.md (for the Lead)

- CONTEXT.md's Gate entry still says the default is `ci` and the key `pr.gate`; the contract now says `none` and `gate.pr`.
- §3.9 puts Clarifying in phase `plan` (CONTEXT.md: Plan stage, phase `plan`). The old table had its predecessor in `grounding`.
- contract.md:3671 still calls `gate.merge` run "the Gate" in its first sentence (`the Gate (gate.merge … gate.pr …)`), as the keys are named; the hook `pre-commit` is the quick check (§7.3 table). Left as is: it needs a decision on whether `gate.merge` runs a quick check or a Gate.

## Deferred to the rename

Names of code, cockpit labels or other Jobs' files, seen and left unchanged:

- Cockpit's third column and pills still say `Deploy`: contract.md:349 (the Deploy column's merge icon, Deploy menu), :2624, :2627, :2629, :2634, :2714, :2732, :2738, :2860, :2899, :2900, :2949, :2956, :2959.
- Inbox kind `deploy` and `[deploys]` domain: :4266 and every `[deploys]` use.
- The `pi/skills/cpd-lead/SKILL.md` section names Dividing, Sequencing, Approving, Dispatching, Mechanical checks, PR, Addressing, quoted in §3.9's "Who does what at Deploying" paragraph and at :4235, :4238 ("Dividing"): they are the skill's headings today; the skill is another Job's.
- The `pr` phase value in migrations 5 and 0008 (:1267, :2151, :2163, :2202): history, applied migrations.
- :1174 "Sequencing across PRs" is the ordinary word, not a step name.
- Commands (`pr add`, `pr open`, `pr rebase`, `--pr`), the phase literal `deploy`, Events `pr-opened`, `pr-merged`, `pr-feedback`: the code's words today.
- Inbox colours: :2751 "the Inbox's cyan tier" and :2818 "its tier's colour" still use "tier" for urgency; not in the Spec's rows (only `tier names`), so not changed.
- :1189 etc. keep "PR" for the object, as the code names it.
