# The schema pin without a shared line (design, for review)

Job `pin-design`, Plan `quality`, 2026-10-20. This is a design for the User's review. It changes no product or test code. Every number below was measured on `hs` against `origin/main` at `c7668949` and the 155 pull requests of `clayton-duarte/cpd`. The commands are given so the numbers can be reproduced. Anything not measured is labelled.

## Summary

- **What the pin catches.** `SCHEMA_HASH` catches three things: a schema change that comes without a migration (legacy Rust, the runner's table, the canonical form, the SQLite bundle), a migration edited after its hash was taken, and a tampered store. `PINNED` in `core/src/events/kind.rs` does less. It only forces whoever changes the Event kinds to touch it. It does not force a `PROTOCOL` raise: #118 added `context-owner`, kept `PROTOCOL` at 4 and re-pinned.
- **The cost, over two weeks** (all of the repo's history). 12 PRs edited `SCHEMA_HASH` and 9 edited `PINNED`. Replaying their merge commits gives **14 distinct merges that conflicted on `SCHEMA_HASH`** (2 of them also on `PINNED`), on the branches of 6 PRs. In 12 of the 14, the merge commit holds a third value, recomputed by hand.
- **What the conflict also does.** It is the only merge-time interlock between two migration PRs. GitHub's mergeability is textual. This private repo cannot have branch protection (the API answers 403). `cpd pr merge` accepts any `CLEAN` pull request. I infer that a pull request behind its base shows as `CLEAN` here, because no `behind` merge state was ever stored, in `pull_requests` or in its history. In **8 of the 14** conflicts, one side added a migration that sorts before a `breaking` migration on the other side. That is a real §4.5 O3 ordering conflict: a rename and a re-check were needed. The conflict forced that re-check. The other 6 conflicts were spurious.
- **Removing the line does not remove the conflict.** **13 of the 14** merges also conflicted on other files, `docs/contract.md` in all 13. The one merge where the pin was the only conflict (`2f5a36b4ef`, #95) was one of the 8 real ordering conflicts.
- **The options, tried on scratch branch pairs** (§2):
  - **(a) A cumulative hash file per migration.** Two migration PRs never conflict, but the later-named record is stale on the merged tree, in 3 of 3 pairs. `main` goes red instead.
  - **(b) A per-migration schema-delta record, with the pin folded from the records at build time.** No conflict, and every record stays valid on the merged tree in 3 of 3 additive pairs. The fold equals the live hash. But a pair that involves a `breaking` migration merges cleanly and then fails the order test on the merged tree: `main` goes red.
  - **(c) Keep the line and make its conflict mechanical.** The conflict stays. One script resolves it in 25–30 s and the pin tests pass.
  - **For `PINNED`:** deleting it removes its conflict (proved clean). Nothing is lost that another test does not already force.
- **Recommendation.** For `SCHEMA_HASH`: (c) now, with `scripts/repin.sh`. Move to (b) only if the User also wants an explicit merge interlock (open question 1). For `PINNED`: delete it. The Jobs are in §3.

## 1. What the pin protects today

### 1.1 `SCHEMA_HASH`

`SCHEMA_HASH` is defined in contract §4.3 and §4.5 (hs-d178 D2). It is the canonical hash of a fresh store that applied exactly this binary's migrations. It is pinned in code and never recorded in the store. It is checked in two places:

- **At run time** (`schema_mismatch`, `core/src/store/mod.rs`): at every open, and inside the migrating transaction before its commit. When the store has no unknown migrations, its canonical hash must equal `SCHEMA_HASH`. Inside the window, the store is compared with the reference *R* (`verdict::reference_database`), and *R* is trusted because a test asserts that its hash is `SCHEMA_HASH`. A mismatch makes write commands fail with `store_tampered`, or the migration fail with `migration_failed`.
- **In `cargo test`**, which the quick check builds and the Gate runs: `the_reference_of_the_built_in_list_has_the_pinned_hash` (`verdict.rs`), `a_migrated_database_has_the_schema_of_a_new_one`, the version-6 test (`schema/tests.rs`), and the orders test (§4.5 check 3).

What it catches, by the drift each test or check sees:

| Drift | Caught by | Without the pin |
|---|---|---|
| A store changed outside cpd (an object dropped or altered) | the run-time check, store vs `SCHEMA_HASH` | still caught, if the run-time check compares with *R* or with a generated pin |
| A schema change without a migration: a legacy function, `RUNNER_DDL`, the canonical form, a new SQLite bundle | the tests, fresh schema vs `SCHEMA_HASH` | **lost**. *R* changes with the code, so a self-referencing check passes, and every existing store would then read as tampered |
| A named migration edited before it reaches `main` | the tests (the hash changes) | **lost** for unmerged files. After `main`, R3 (checksum) and N5 (`scripts/check-migrations-frozen.sh`) catch it |
| Two migration PRs that do not commute (§4.5 O3) | **the textual conflict on the line** forces the second PR to merge `main` and re-run the Gate, whose order test fails | **lost**: §2.4 |

Generated history triggers are not in the hash (§4.3 item 5). They are checked against the generator at run time, so a change to the generator is not caught by the pin today either (read from `canonical_entries` and `generated_mismatch`; out of scope).

### 1.2 `PINNED` (the Event-kind hash)

`PINNED` is the test constant `(PROTOCOL, sha256 of the sorted kinds)` in `core/src/events/kind.rs`. Its test fails when the kinds or `PROTOCOL` change until `PINNED` is set again, and its message says to raise `PROTOCOL`. It does not enforce the raise. The contract says an added kind the adapter ignores does not raise `PROTOCOL` (§5.2), and #118 did exactly that (measured: `PROTOCOL` 4→4, kinds 47→48). Two other tests already force the adapter to follow:

- `pi/test/protocol.test.ts`: `KINDS` equals core's kinds, and both `PROTOCOL` values are equal.
- `cli/tests/it/events.rs`: `Kind::ALL` equals §5.2's table and the rows of `event_kinds`.

So `PINNED` only forces a decision point in core. A new kind changes no schema object, because it is a row of `event_kinds`. The scratch pair `k` below left `SCHEMA_HASH` unchanged (`a538a1a2…` before and after).

### 1.3 Measured: who edited the pins and who conflicted

The window is 2026-10-12 (the first commit) to 2026-10-20: the whole history, 263 commits on `main` and 155 PRs.

**Edits.**
- `git log -G'pub const SCHEMA_HASH: &str' origin/main` gives 7 squash commits on `main`: #75, #94, #95, #118, #122, #136, #138.
- `git log -G'const PINNED' origin/main` gives 6: #28, #30, #37, #105, #118, #136.
- (`git log -S'SCHEMA_HASH'` gives 16, but it counts commits that add or remove a mention, not value changes.)
- Counting every PR whose own non-merge commits (from `gh api …/pulls/<n>/commits`) edit the line, stacked and open PRs included:
  - **`SCHEMA_HASH`: 12 PRs**: #75 #94 #95 #107 #118 #119 #122 #136 #138 #139 #143 #152;
  - **`PINNED`: 9 PRs**: #28 #30 #37 #105 #107 #118 #136 #143 #152.
- (#136 carries #107's commits, because it is stacked on it.)
- Before the named migrations (#75), the shared lines were the legacy `SCHEMA_HASHES` array. 12 squash commits appended to it, and 5 merges conflicted on it (#11, #37, #42, #75 ×2).

**Conflicts.** The method:
1. For each of those PRs, take every merge commit in its commit list and redo the merge with `git merge-tree --write-tree --name-only <parent1> <parent2>`.
2. Count a pin conflict when a conflict hunk of `schema.rs` or `kind.rs` contains `SCHEMA_HASH` or `PINNED`.
3. Only 2 of these PRs were ever force-pushed (#28 and #105, once each), so merges hold almost every resolution.

The result is 14 distinct merges that conflicted on `SCHEMA_HASH`, all in the named era:

| Merge | Branch (PR) | Migrations on each side | Real O3 conflict (a name sorts before a breaking one on the other side) | Other conflicted files |
|---|---|---|---|---|
| `2f5a36b4ef` | sqlmig (#95) | read-indexes, history-changed **B** / job-urgent | yes: job-urgent < history-changed | none |
| `ad385e37e2` | titles (#107) | session-titles / job-urgent | no | kind.rs (`PINNED`), contract, commands |
| `b490254941` | titles (#107) | session-titles / read-indexes, history-changed **B** | no | contract |
| `a7a2c51fce` | titles (#107) | session-titles / policy-keys **B** | yes | kind.rs (`PINNED`), contract, 5 more |
| `31795abd48` | titles (#107) | session-titles / pr-cancel-started **B** | no | contract, 3 more |
| `b264548917` | titles (#107) | 3 additive / pr-cancel-open **B** | yes (×3) | contract, 2 more |
| `80c8b9d0e8` | repo-policy (#118) | policy-keys **B** / read-indexes, history-changed **B** | yes (×2) | contract, 14 more |
| `df63d6816e` | pr-links titles (#119) | 2 additive / 3 incl. history-changed **B** | no | contract, 9 more |
| `7b3d2e4c78` | pr-links titles (#119) | 2 additive / policy-keys **B** | yes | contract, 10 more |
| `4173912e1a` | pr-links titles (#119) | 2 additive / session-titles | no | contract, schema/tests.rs |
| `92bebc8b7f` | pr-links titles (#119) | 2 additive / pr-cancel-started **B** | no | contract, 5 more |
| `d24b74e039` | prcancel (#122) | pr-cancel-started **B** / read-indexes, history-changed **B** | yes (×2) | contract, 3 more |
| `2163a1b391` | inbox attn (#143) | waiting-state **B** / policy-keys **B**, pr-cancel-started **B**, 3 additive | yes (×2) | contract, 11 more |
| `15add78a0d` | inbox attn (#143) | waiting-state **B** / pr-cancel-open **B**, 3 additive | yes | contract, 10 more |

(**B** = `breaking`. The first five titles merges are also in #136 and #143, which stack on #107.)

- **Recomputed by hand:** in 12 of the 14, the merge commit's `SCHEMA_HASH` equals neither parent's. In `ad385e37e2` a later commit set it ("store: SCHEMA_HASH of the merged schema", `9addb69d`). Where `d24b74e039`'s value was fixed is unverified.
- **`PINNED`:** 2 conflicts, both on the titles branch. Both merges resolved them by putting the two sides' new kinds under one `PROTOCOL` (their commit messages: "PROTOCOL 5, the merged kinds", "PROTOCOL 5 with main's context-owner kind").
- **Corroboration** from cpd's store, read-only: `pr-feedback` Events with `data.conflict` true, for any file, number 26 for #107, 11 for #119 and 7 for #122.

## 2. Options

All the proofs ran on scratch branches `scratch/pin-design-a1/*`, cut from `origin/main` `c12d969a`. They have since been deleted. The base branch added one prototype:

- `build.rs` ignores `*.hash` and `*.schema` files.
- A test module `core/src/store/schema/proto.rs` applies the legacy set, the runner's table, then each named migration in name order. After each migration it writes or compares two records:
  - `<name>.hash`, the canonical hash after the prefix (option a);
  - `<name>.schema`, the canonical *atoms* the migration removes (`-`) and adds (`+`) (option b).

  An atom is one column item of a table (§4.3 item 3), a table's constraint list, its options, or one non-table entry. For example, `20261019T0451-job-urgent.schema` is one line: `+ ["column","jobs","urgent INTEGER NOT NULL DEFAULT 0 CHECK (urgent IN (0, 1))"]`.
- A frozen `base.schema` records the legacy set and the runner's table (279 atoms).
- `proto_fold` folds `base.schema` and every record into the canonical hash without running a migration, which is what a `build.rs` would do.

Each pair has two branches off the base, `a` and `b`. Each adds one migration, made as a real PR makes it, in each variant. The two are then merged with `git merge-tree`.

### 2.1 The pairs, and what each option does with them

| Pair | Migrations | Today: the single line | (a) cumulative `.hash` | (b) `.schema` deltas, pin folded |
|---|---|---|---|---|
| p1 | `CREATE TABLE pin_a` / `plans ADD COLUMN pin_b` | **conflict** in `schema.rs` | merge clean; `pin-b.hash` **stale** | merge clean; records valid; fold = live = `225dbc92…` |
| p2 | `jobs ADD COLUMN pin_x` / `jobs ADD COLUMN pin_y` (same table) | **conflict** | clean; `pin-y.hash` **stale** | clean; valid; fold = live = `388f7d7d…` |
| p3 | additive `CREATE INDEX` / `breaking after 24` `DROP INDEX` | **conflict** | clean; `pin-drop.hash` **stale**; **order test fails** | clean; records valid; **order test fails**: "`20261020T1301-pin-drop` says `breaking after 24` but 25 migrations sort before it" |
| k | kinds `plan-pinned` / `session-poked` (migration, `kinds!`, `spec`, `typed.rs`, `PROTOCOL` 5→6, `pi/lib/event.ts`, events SPEC) | **conflict** in `kind.rs`, on `PINNED` only. `PROTOCOL` 5→6 on both sides merges clean | n/a | `PINNED` deleted: merge clean; `PROTOCOL` 6 holds both kinds; `pi/test/protocol.test.ts` 9/9 pass; `the_kinds_are_exactly_those_of_5_2_and_of_event_kinds` passes; fold = live |
| kl | two kinds in one `KINDS` line of `pi/lib/event.ts` | **conflict** in `event.ts` (any option: same line) | | |

**Drift, on a scratch branch off the base.**

| Change | Today | (a) | (b) |
|---|---|---|---|
| `RUNNER_DDL`'s `CHECK (length(name) BETWEEN 6 AND 54)` → `60` | `SCHEMA_HASH` test fails | every `.hash` fails | `base.schema` fails |
| One index appended to `20261019T0504-read-indexes.sql` | `SCHEMA_HASH` test fails | every `.hash` from it on fails | exactly `read-indexes.schema` fails |

### 2.2 (a) One cumulative hash per migration, checked as a chain

`migrations/<name>.hash` holds the canonical hash after the legacy set and every named migration up to `<name>`, in name order. A test rebuilds the chain step by step and compares each step with its file. `SCHEMA_HASH` becomes the last file's value, through `build.rs`.

- **Keeps:** the run-time check, the drift check, and a stale record names the first migration that differs.
- **Loses:** the interlock. Worse, it turns *every* concurrent pair into a broken `main`. A cumulative value depends on every earlier name, so the later-named PR's record is stale whatever lands first (p1, p2, p3: 3 of 3). With no textual conflict and no check on the merged tree (§1.1), the second PR merges and `main` fails `cargo test` for every PR after it.
- **Cost:** `build.rs` accepts `.hash`; a chain test; a writer (`PIN_WRITE=1 cargo test …`, or a script); N5's frozen check also covers `.hash`.
- **Contract:**
  - §4.3: "`SCHEMA_HASH` … is the hash in the last migration's record";
  - §4.5 N4: "adds one file and its `.hash`" in place of "it also edits `SCHEMA_HASH`";
  - N5: records are frozen too;
  - `implementing.md` step 4.
- **Verdict:** rejected. It moves the conflict onto `main`.

### 2.3 (b) Per-migration schema deltas; the pin folded by the build; drift checked by a test

- **The records.** `migrations/<name>.schema` holds the atoms that migration removes and adds, as text a reviewer can read. `migrations/base.schema` is written once and frozen. A test applies the migrations one by one and compares each delta with its record. `scripts/new-migration.sh` (or a `PIN_WRITE=1` test run) writes the record.
- **The pin.** `build.rs` folds `base.schema` and the records into the canonical entries and writes `SCHEMA_HASH` into `OUT_DIR`. `proto_fold` proves the fold equals the live hash on the base and on all four merged trees.
- **The run-time check** is unchanged: the store against `SCHEMA_HASH`, or against *R* inside the window.
- **Rejected alternative:** building *R* at every open instead of folding at build time. It costs a median of 152 ms per process in release (min 97, max 397; 350 ms in debug; N = 7, on `hs` while other Workers ran). Today an open builds *R* only inside the window.

**What (b) keeps and loses.**
- **Keeps:** every drift row of §1.1. A stale record names exactly the migration. For additive pairs (p1, p2 and k) the records stay valid on the merged tree, because additive migrations touch disjoint atoms (A1, O2).
- **Loses:** the interlock, for the cases that need it. A pair where one side sorts before the other's `breaking` migration (p3) merges cleanly and fails check 2 on the merged tree. That breaks `main` for everyone until someone renames the migration. On the measured history this is 8 of the 14 conflicts. 5 of the 10 named migrations on `main` are `breaking`.

**Cost.**
- `build.rs` gets `sha2` and `serde_json` as build-dependencies, and the fold, which needs `column_key` from `schema.rs`. Sharing it through `include!` is unverified.
- A record test, a frozen `base.schema`, and the record writer in `new-migration.sh`. The prototype is 167 lines, its cost and fold tests included.
- N5 also covers `.schema`.
- An explicit interlock for O3 (open question 1).

**Contract.**
- §4.3: "`SCHEMA_HASH` is generated by the build from the migrations' records; a test asserts each record";
- §4.5 N4 drops "(it also edits `SCHEMA_HASH`)";
- N5 freezes `<name>.schema` and `base.schema`;
- A3 check 3 compares with the generated value;
- `implementing.md` step 4 says "write the record".

### 2.4 (c) Keep the line; make its conflict mechanical

`SCHEMA_HASH` and its tests stay as they are. `scripts/repin.sh` makes the resolution one command:
1. It takes our side of each conflict hunk that holds `SCHEMA_HASH` or `PINNED`, leaves any other hunk, and refuses when one remains.
2. It builds and runs a test that prints the fresh hash, and writes that value.
3. It writes `PINNED` from the `kinds!` block and `PROTOCOL`, then runs `git add`.

**Proof.** The prototype ran on `p1-today-b` merged with `p1-today-a`, and on `k-today-b` merged with `k-today-a`. Each merge stopped on the pin only. The script resolved it in **30 s** and **25 s** with a warm cache. Both pin tests (`the_reference_of_the_built_in_list_has_the_pinned_hash`, `the_event_kinds_are_pinned_to_the_protocol`) then passed. p1's value (`225dbc92…`) equals the fold of (b).

- **Keeps:** everything, the interlock included. GitHub still shows the conflict, so the second PR still re-runs its Gate on the merged tree. That is what caught the 8 ordering conflicts.
- **Loses:** nothing. It does not remove the conflict. In 13 of 14 merges, other files conflicted as well.
- **Is that enough?** It removes the by-hand recompute, which is the part of each conflict the pin alone causes. It does not remove the guaranteed conflict. That conflict is load-bearing for the 8 real ordering conflicts and redundant in the other 6.

**A merge driver instead** (`.gitattributes` `merge=`): git 2.43's `merge-tree` and `rebase` honour a configured driver (measured in a temporary repo: the conflict went away, exit 0). So a driver would also let `cpd pr rebase` step 3 pass. But a driver cannot compute the hash, because the merged tree is not built yet. It would leave a stale value for the Gate to fail on. It also needs `git config` in every clone. That GitHub's server-side merge ignores custom drivers is unverified (from GitHub's documentation). Not recommended: running the script after the merge is simpler.

**Cost:** one script and a print test. A Lead resolves with `git merge origin/main`, then `scripts/repin.sh`, then `cpd pr push`.

**Contract:** §4.3, after "A migration's PR updates the constant in the same change", add "`scripts/repin.sh` writes it, and resolves a merge conflict on it". The same goes in `implementing.md` step 4.

### 2.5 The Event-kind hash

| Option | Conflict (pair k) | Keeps | Loses | Cost |
|---|---|---|---|---|
| Today, `PINNED` | **conflict** on `PINNED` in every pair | a decision point when kinds change | | a recompute per conflict |
| (c) `repin.sh` writes `PINNED` | conflict stays, resolved by the script (proved, 25 s) | the same | | shares (c)'s script |
| **Delete `PINNED`** | **none** (proved: clean merge, `PROTOCOL` 6 for both, adapter and kinds tests pass) | `protocol.test.ts` still fails until `pi/lib/event.ts`'s `KINDS` follows, and that file's header says when to raise `PROTOCOL`, next to it; the events test still ties `Kind::ALL` to §5.2 and `event_kinds` | the check in core; two kinds under one `PROTOCOL` | delete 25 lines of `kind.rs` |
| A strict Gate check, "kinds changed since the merge-base ⇒ `PROTOCOL` raised" (`scripts/check-protocol.sh`, prototyped) | none | the raise, enforced | | **contradicts §5.2**: a kind the adapter ignores does not raise (#118); rejected |

**Merging two kinds under one `PROTOCOL`** is what the Leads already do when they resolve (§1.3). It is harmless for `main`'s binary and adapter. A preview adapter built from one branch would not be told about the other branch's kind, and ignores it as `unknown-kind` (`pi/lib/event.ts`; read from the code, not run).

**A residual shared line:** `KINDS` in `pi/lib/event.ts` lists several kinds per line. Two kinds in one group conflict (pair kl). That resolution is "keep both", with no computation.

## 3. Recommendation and its Jobs

**Recommendation:**
1. **Delete `PINNED`.** It has no interlock value, because two kind additions commute (pair k). It only forces a decision that the adapter test already forces. This removes its conflict and loses nothing.
2. **For `SCHEMA_HASH`, take (c) now.** `scripts/repin.sh` ends the hand recompute and keeps the only interlock this repo has. Removing the line, (a) or (b), turns 8 of the measured 14 conflicts into a broken `main` instead.

(b) is the right shape if the line must go. It gives readable per-migration records and no conflicts between additive PRs. But it should come only together with an explicit interlock (open question 1).

The Jobs follow hs-d341's order. `pr-open` follows as usual (hs-d340).

| Job | Who | Touches | Acceptance |
|---|---|---|---|
| **pin-docs** (docs) | any Worker | `docs/contract.md` (§4.3 the schema check, §4.5 N4, §5.2 "The protocol number"), `docs/implementing.md` (step 4) | §4.3 and N4 say: a migration's PR writes `SCHEMA_HASH` with `scripts/repin.sh`, which also resolves a merge conflict on it (takes either side of the pin hunks only, refuses when another hunk remains, writes the computed value, stages it). §5.2 says the kind set is tied to the adapter by `pi/test/protocol.test.ts` and to `event_kinds` by the events test, and no kind hash is pinned. `implementing.md` step 4 says "run `scripts/repin.sh`". No other section changes. |
| **pin-tests** (red tests; a different Worker from pin-impl) | | `core/tests/repin.rs` (new; a scratch clone as `core/tests/migrations.rs` does for the frozen check), `scripts/repin.sh` (stub: `exit 1`) | Tests, named, that fail against the stub for a reason other than a build error: (1) a merge that conflicts only on `SCHEMA_HASH` is resolved, and the file holds the value the print test gives; (2) a conflict on `SCHEMA_HASH` plus another hunk in `schema.rs` is refused, and the other hunk is left as it was; (3) no conflict: the script only rewrites a stale value; (4) rerunning changes nothing (idempotent). |
| **pin-impl** (implementation) | | `scripts/repin.sh`, `core/src/store/verdict.rs` (an `#[ignore]` test that prints `SCHEMA_HASH=<hex>`), `core/src/events/kind.rs` (delete `PINNED`, `kinds_hash` and their test) | pin-tests pass. The quick check and `npm run verify` pass. `rg PINNED core` finds nothing. |

## 4. Open questions for the User

1. **Do you want the line gone, (b), at the price of an explicit interlock?** There are three ways to get one:
   - `cpd pr merge` refuses when its base gained a migration file since the head's merge-base and the PR adds one, and asks for `cpd pr rebase` first (Gate on the merged tree). It needs the migrations path as a per-repo policy key (§8.3), and it does not cover merges made on github.com;
   - every PR is rebased onto its base before merge, which is GitHub's strict mode by hand: a Gate per merge, merges serialised;
   - accept that `main` goes red in O3 cases until someone renames the migration.

   (None of these was prototyped.)
2. `docs/contract.md` conflicted in 13 of the 14 merges. Contract §4.4 copies the DDL that every migration changes (`implementing.md` step 4), which makes it the next shared region after the pin. Should that be a Job of its own?
