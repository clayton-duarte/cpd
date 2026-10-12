# t2: How does CPD's Ledger map onto beads?

Tier: S3 · Report: `~/projects/cpd/docs/trials/t2-ledger.md` · Read `_common.md` first.

## Goal

Turn "use beads (`bd` 1.3.1) as the Ledger, written only by `cpd`" into a concrete, tested contract. Show how every Ledger concept maps onto beads, which `cpd` invariants beads cannot enforce, and the exact `bd` commands and JSON fields `cpd` will use.

## Ledger concepts to map

- **Workflow:** one ticket → one repo → one PR. Has a Lead Branch, a phase (Context, Plan, Dispatch, PR), and a Lead session reference.
- **Job:**
  - Spec as structured fields: goal, context, touches (files), acceptance (commands), out-of-scope.
  - A Tier (e.g. `S1`).
  - State: draft → queued → ready (derived) → running → in_review → merged | failed | cancelled.
  - Becomes **Locked** when Dispatch starts: Spec, Tier and Dependencies never change after that.
- **Dependency:** a Job may start only after its Dependencies are **Merged**. A **cancelled** Job must **not** silently unblock its dependents.
- **Attempt:** one try of a Job by one fresh Worker session.
  - Fields: number, Tier, worktree, branch, started/ended, outcome (submitted, approved, rejected, failed, timed_out), Reviewer feedback, retry cause.
  - Attempts are append-only and never rewritten.
- **Review verdict** per Attempt; **Escalation** (question, asker, answer, answered_by); **metrics** per Attempt (duration, turns, tokens, cost).
- **Append-only:** after Dispatch starts, nothing is edited or deleted. New Jobs can be appended and Jobs cancelled. History must be readable for post-mortems.

## Constraints

- **The product repo must not be touched by beads.** Use a scratch product repo `/tmp/cpd-trials/t2/repo` (git init, one commit). Before and after every `bd` step, prove it unchanged:
  - `git -C … status --porcelain`;
  - `git -C … config --list --local`;
  - `ls -la .git/hooks`;
  - a checksum of all files.
- **The Ledger lives outside the repo**, e.g. `/tmp/cpd-trials/t2/ledger/<workflow-id>/.beads`, selected with `BEADS_DIR` (docs: `/tmp/cpd-research/repos/steveyegge__beads/docs/reference/worktrees.md`, `configuration.md`).
- **Sandbox global writes:** run **every** `bd` command with `HOME=/tmp/cpd-trials/t2/home` and `XDG_CONFIG_HOME=/tmp/cpd-trials/t2/home/.config`. `bd init` options such as `--stealth` write **global** git settings, which must not reach the real `~/.gitconfig`. Afterwards, list every file bd created under the sandbox HOME and say what each is for.
- Read `bd init --help` and the docs before initialising. Find the flags that avoid writing `AGENTS.md`, agent hooks or repo git config (e.g. `--skip-agents`, `--stealth`, `no-git-ops`). State which combination `cpd` should use and why.

## Acceptance criteria

1. **must** Init recipe: a Ledger outside the repo, the product repo provably untouched, the list of global side effects in the sandbox HOME.
2. **must** Mapping table, every concept above → beads construct (issue types, parent/child, dependency types, statuses and their categories, metadata `--set-metadata`, labels, comments, gates). Each row demonstrated by commands and `--json` output.
3. **must** Ready semantics: build a 5-Job graph (A; B and C depend on A; D depends on B and C; E independent) and show `bd ready --json` at each step:
   - A Merged (closed);
   - B Merged;
   - C **cancelled**: show the representation that keeps D blocked (custom status or category, or another construct) and prove D is not ready;
   - an appended Job F.
4. **must** Attempts: three Attempts for one Job (rejected, rejected, approved) recorded append-only with their Tiers (S1, S2, S3) and feedback; read the full history back in order.
5. **must** Locked: show what beads allows that would break Locked (`edit`, `update` of description or dependencies, `delete`, `compact`). Test whether beads' freeze marker (`configuration.md` around line 527) or any other feature can protect a Job or a Workflow. Conclude what `cpd` must enforce itself.
6. **must** History: show `bd history`, `bd show --json` and any audit or Dolt log commands; what each preserves after an edit (old values recoverable?).
7. **should** Concurrency: run two `bd` write commands at the same moment (`&`) 10 times; report lock errors, waits, and the safe pattern for `cpd` (serialise with a file lock vs retry).
8. **should** Latency: time 20 representative commands (create, dep add, ready, show, close); report median and worst.
9. **should** JSON stability: the fields `cpd` will parse from `ready`, `show`, `list`, `history`, and whether `bd` prints non-JSON noise on stdout or stderr.

## Deliverable extras

In **Recipes**, propose the `cpd`↔`bd` contract: for each `cpd` operation, the exact `bd` command(s), the fields read, and the invariant `cpd` checks before writing:
- `workflow init`
- `job add`
- `plan lock`
- `job ready`
- `attempt start` / `submit` / `verdict`
- `job merged`
- `job cancel`
- `escalation ask` / `answer`
- `metrics record`
- `history`

Clean up: remove `/tmp/cpd-trials/t2/home` only after listing its contents in the report.
