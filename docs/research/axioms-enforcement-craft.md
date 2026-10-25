# Axioms audit 2: craft axioms 1 to 7

## Method

Read (all read-only, hs-d606): this Spec; the axiom text as quoted in my own system prompt
(`/home/clayton/.pi/agent/AGENTS.md`); `docs/implementing.md` (Layout, Tests, Dependencies, The Gate, The portability
check), `docs/contract.md` (§6.5, §6.1.1, §8.1), `docs/commands.md` §15.11-15.17; `Cargo.toml` and the four crate
manifests; `scripts/{check-portable,check-migrations-frozen,npm-fresh,pi-check,repin,install-cpd,cargo,gate-run}.sh`;
`core/src/{policy/mod.rs,policy/exposure_tests.rs,config/file.rs,spec/worker.md,pill_tests.rs,events/mod.rs,host/tmux.rs,host/pi/launch.rs}`;
`cli/tests/tooling/{layout,dead_code,interface,npm_fresh,pill_sites,pill_parity}.rs`; `cli/tests/flow/config.rs`;
`tui/src/{logs/mod.rs,plan/box_tests.rs,tree/scroll_tests.rs}`; `pi/lib/{adapter,mainguard,noblock,testguard,secretguard,askguard}.ts`;
`pi/skills/skill.test.ts`. Ran the read-only `timeout`-wrapped commands listed under each entry's **Verified**.

**My own system prompt (this Attempt) does contain the text of `/home/clayton/.pi/agent/AGENTS.md`.** I read it in this
session: the 19 current axioms (`timeout 60 rg -c '^## ' /home/clayton/.pi/agent/AGENTS.md` → 19) are quoted verbatim under `<project_instructions path="/home/clayton/.pi/agent/AGENTS.md">`.
`core/src/host/pi/launch.rs:163` (`worker()`) builds the argv with no `--no-context-files` (only `distiller()` at `:195`
passes it), so cpd loads context files for a Worker. A9 is therefore checked on a live prompt, not inferred.

### 1. Convention over configuration

- **Enforced by:**
  - `test` `cli/tests/tooling/layout.rs:204`: pins the four workspace members, their directories, `cpd-engine` owning `watch`/`doctor`, `cpd-core` not, and `cpd-tui` not depending on `cpd-engine` (the one-way dependency direction). Blind spot: it does not require each dependency line to be `.workspace = true`; a crate can add a directly versioned dependency and this still passes.
  - `test` `cli/tests/tooling/layout.rs:294`: `-p <member>` must resolve the same features as `--workspace`. Blind spot: feature unification only, never versions or provenance.
  - `script` `scripts/npm-fresh.sh:23` (`exec npm ci --no-audit --no-fund`): installs exactly `package-lock.json`, so a Node deviation is pinned; pinned by `test` `cli/tests/tooling/npm_fresh.rs:45,50,56,61`. Blind spot: Node only. The Rust build is not run `--locked` in the Gate: `scripts/install-cpd.sh:76` and `scripts/preview-tui.sh:69` are the only `--locked` users.
  - `check` `scripts/check-portable.sh:87`: a file of `scripts/` in neither the covered nor the Linux-only list fails, each entry carrying a written reason, so a new script cannot skip the decision. Blind spot: portability, not dependency convention.
  - `test` `cli/tests/flow/config.rs:265`: every unknown `config.toml` key is refused with its key path; a deviation must be named, never silently accepted. Blind spot: cpd config only.
- **Prose only:** `docs/implementing.md:153`: "Every dependency is declared once ... **Later Jobs may not add, remove or change dependencies or features**" — no test reads a crate `Cargo.toml` for this, and the axiom's "search for an existing `pi-package` and take it pinned" step has no mechanism: the keyword is declared in `pi/package.json:7` but nothing searches npm for it.
- **Gap:** a crate can add `foo = "1.2"` in its own `[dependencies]` instead of `foo.workspace = true` and no check fails; the four manifests use `.workspace = true` 16/11/13/13 times (see Verified) but nothing pins that. The Gate's test build also re-resolves `Cargo.lock`, since `scripts/cargo.sh` passes no `--locked`, so a manifest edit can move the lock inside the Gate. No incident evidence: `no evidence`.
- **Proposal:** add one `test` to `cli/tests/tooling/layout.rs` (beside `:294`) that reads the four crate manifests and fails any `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` entry that is neither a `path` dependency nor `.workspace = true`, and add `--locked` to the Gate's cargo commands in `package.json` `verify:steps`. Size S.
- **Verified:**
  - `timeout 60 rg -n '\[workspace.dependencies\]' Cargo.toml` → `12:[workspace.dependencies]`.
  - `timeout 60 rg -c 'workspace = true' core/Cargo.toml engine/Cargo.toml cli/Cargo.toml tui/Cargo.toml` → `core/Cargo.toml:16`, `engine/Cargo.toml:11`, `cli/Cargo.toml:13`, `tui/Cargo.toml:13`.
  - `timeout 60 rg -n -- '--locked' scripts/*.sh` → `scripts/install-cpd.sh:76`, `scripts/preview-tui.sh:69` (2 lines).
  - `timeout 60 rg -n 'fn the_four_crates|fn every_member_resolves' cli/tests/tooling/layout.rs` → `204`, `294`.
  - `timeout 60 rg -n 'exec npm ci' scripts/npm-fresh.sh` → `23`.
  - `timeout 60 rg -n 'not classified' scripts/check-portable.sh` → `87`.
  - `timeout 60 rg -n 'fn config_refuses_each_bad_file' cli/tests/flow/config.rs` → `265`.
  - `timeout 60 rg -l 'pi-package' --glob '!node_modules' --glob '!target' | wc -l` → `13` files (only `pi/package.json:7` declares the keyword; the rest are docs); `timeout 60 rg -n 'pi-package' pi/package.json` → `7`.

### 2. Inference is not evidence

- **Enforced by:**
  - `none`. No type, guard, check, test, Gate or hook requires a claim to be labelled or verified. The generic Worker Spec template (`core/src/spec/worker.md`) has no `## Verified` and no `## Not verified` section; the TDD red check (`tests_not_red` at `core/src/spec/worker.md:193`, `tests_failed` at `:200`) proves a test fails first, which is a different rule (10, out of scope).
  - `check` the quick check the Spec embeds (`core/src/spec/worker.md:40`) runs `npm run typecheck` and `scripts/cargo.sh build --workspace`, which prove the artefact compiles, not that a claim is true; blind spot: compilation is not evidence of behaviour.
- **Prose only:** `docs/contract.md:7`: "**unverified** means a design assumption the implementation Job must check"; `docs/research/tests-mock-units.md:1`: "A claim I did not read in the source is marked `unverified`". Both are text the reader must honour; nothing fails when it is ignored. `rg -n 'unverified|Verified|not verified' core/src/spec/worker.md` prints nothing.
- **Gap:** a Worker, Lead or Distiller can submit an unlabelled inference and nothing refuses it. Evidence: `core/src/spec/worker.md` contains 0 matches for `unverified|Verified`; the two in-tree uses of the label are comments (`core/src/events/mod.rs:214`, `core/src/host/tmux.rs:657`); the Distiller prompt `core/src/context/distill_prompt.md` has 0 matches for `unverified|evidence|verify`. No `error` Event code records an unlabelled claim: `no evidence`.
- **Proposal:** add a `Verified:` line (and an explicit "label anything you did not verify `unverified`" rule) to the Worker Spec template `core/src/spec/worker.md`, and a `test` in `core/src/spec/mod.rs` that the rendered template contains that rule; a research/audit acceptance then cannot omit it. Size S.
- **Verified:**
  - `timeout 60 rg -n 'unverified|Verified|not verified' core/src/spec/worker.md` → exit 1, 0 lines.
  - `timeout 60 rg -n 'unverified' core/src/events/mod.rs core/src/host/tmux.rs` → `core/src/events/mod.rs:214`, `core/src/host/tmux.rs:657` (2 lines).
  - `timeout 60 rg -n 'unverified|evidence|verify' core/src/context/distill_prompt.md` → exit 1, 0 lines.
  - `timeout 60 rg -c 'unverified' --glob '*.md' docs | wc -l` → `111`.
  - `timeout 60 rg -n 'pub fn worker|pub fn distiller|no-context-files' core/src/host/pi/launch.rs` → `163`, `193`, `195` (the Worker argv has none; the distiller at `:195` passes `--no-context-files`).

### 3. YAGNI

- **Enforced by:**
  - `test` `cli/tests/tooling/dead_code.rs:19`: every `#[allow(dead_code)]` needs a live reason on its own line and may not cite a finished Job, so unused code must be justified rather than silenced. Blind spot: it fires only on an explicit `allow(dead_code)`; a `pub` item with no caller needs no attribute and is never seen, and a reason of "stub" or "not written yet" passes.
  - `check` `core/src/spec/worker.md:123` (`### Out of scope`): each Job's Spec carries the scope, and the acceptance is checked at submit; blind spot: it is text the Worker must obey, and the submit check tests the named acceptance, not that no extra file was touched (the diff may touch any file not guarded or protected).
- **Prose only:** `docs/implementing.md:153`: "Later Jobs may not add, remove or change dependencies or features ... If you need something not listed, ask the Lead" — a scope limit with no guard; `docs/implementing.md:177`: "Deliberately absent: a regex crate ... an error-derive crate ..." — the decisions are prose, and nothing fails when a new dependency is added.
- **Gap:** speculative code passes. `tui/src/tree/actions.rs:149` is `#[allow(dead_code)] // stub: the click that builds it is not written yet` — a stub kept alive by a reason that says it is not used, exactly what YAGNI forbids, and `dead_code.rs` accepts it because the reason is non-empty and names no Job. Evidence: `rg -n 'allow\(dead_code\)' --glob '*.rs'` counts 12 sites in 8 files (see Verified). No dependency guard exists: `cli/tests/tooling/layout.rs` reads `Cargo.toml` only for crates/features (2 matches), never to refuse a new dependency. No `error` Event counts scope creep: `no evidence`.
- **Proposal:** extend `cli/tests/tooling/dead_code.rs:19` to fail an `allow(dead_code)` whose reason contains a speculative marker (`stub`, `not written yet`, `later`, `future`, `TODO`), so the stub at `tui/src/tree/actions.rs:149` must be deleted or justified as live. Size S.
- **Verified:**
  - `timeout 60 rg -n 'allow\(dead_code\)' tui/src/tree/actions.rs` → `149`.
  - `timeout 60 rg -c 'allow\(dead_code\)' --glob '*.rs' | awk -F: '{s+=$2} END {print s}'` → `12`; `timeout 60 rg -l 'allow\(dead_code\)' --glob '*.rs' | wc -l` → `8`.
  - `timeout 60 rg -n 'fn every_allow_dead_code' cli/tests/tooling/dead_code.rs` → `19`.
  - `timeout 60 rg -n 'Later Jobs may not add|Deliberately absent' docs/implementing.md` → `153`, `177`.
  - `timeout 60 rg -n 'Out of scope' core/src/spec/worker.md` → `123`.
  - `timeout 60 rg -n 'Cargo.toml' cli/tests/tooling/layout.rs | wc -l` → `2`.

### 4. KISS

- **Enforced by:**
  - `none`. There is no lint, complexity check or size limit. `rg -n 'clippy' Cargo.toml core/Cargo.toml engine/Cargo.toml cli/Cargo.toml tui/Cargo.toml` exits 1 (0 matches), there is no `[workspace.lints]` and no `.cargo/config.toml`, so nothing in the Gate can fail a needlessly complex change.
- **Prose only:** `docs/implementing.md:177` ("Deliberately absent ...") records decisions against new machinery; `docs/implementing.md:13` pins the crate count and dependency direction as a design, but a test at `cli/tests/tooling/layout.rs:204` only checks that shape, not complexity inside a crate.
- **Gap:** a very long function, a deep generic tower or a needless abstraction passes every check. Evidence: 0 clippy matches across the five manifests; `timeout 60 rg -n 'too_many_lines|cognitive_complexity|complexity' Cargo.toml core/Cargo.toml tui/Cargo.toml cli/tests/tooling/*.rs` prints nothing. No `error` Event code measures complexity: `no evidence`.
- **Proposal:** add `[workspace.lints.clippy]` with `too_many_lines` and `cognitive_complexity` and a `scripts/check-lint.sh` (`scripts/cargo.sh clippy --workspace --all-targets -- -D warnings`) as a Gate step in `package.json` `verify:steps`; a `test` in `cli/tests/tooling/` pins that the Gate runs it. Size M (clippy is a toolchain component, no new crate).
- **Verified:**
  - `timeout 60 rg -n 'clippy' Cargo.toml core/Cargo.toml engine/Cargo.toml cli/Cargo.toml tui/Cargo.toml` → exit 1, 0 lines.
  - `timeout 60 rg -n 'too_many_lines|cognitive_complexity|complexity' Cargo.toml core/Cargo.toml tui/Cargo.toml cli/tests/tooling/*.rs` → exit 1, 0 lines.
  - `timeout 60 rg -n 'fn the_four_crates' cli/tests/tooling/layout.rs` → `204`.
  - `timeout 60 rg -n 'Deliberately absent' docs/implementing.md` → `177`.

### 5. DRY

- **Enforced by:**
  - `test` `core/src/policy/mod.rs:565` (`the_contract_domains_table_is_the_code`): the domains table of `docs/contract.md` equals `Domain::ALL`, the `Policy` fields and the `default.toml` tables, and every documented typed key/default/allowed value is accepted by the loader. Blind spot: the policy table alone.
  - `test` `core/src/policy/exposure_tests.rs:105`: the table's Exposure column equals the code's `exposure(domain)`. Blind spot: one column.
  - `test` `cli/tests/tooling/interface.rs:65`: `docs/interface.md` is generated from `cpd --help` and byte-compared. Blind spot: one generated page.
  - `test` `core/src/pill_tests.rs:10` and `cli/tests/tooling/pill_sites.rs:90,107`: `pi/lib/pill-spec.json` is the one pill source (caps, colours, clickable rule), and the cap glyphs and reverse-video escapes are refused in non-test source outside the spec and the three adapters. Blind spot: pill data only.
  - `test` `tui/src/logs/mod.rs:146` (`no_stale_statement_is_left`): a fixed list of stale phrases must not reappear in `docs/contract.md`, `docs/commands.md`, `logs/mod.rs` or `plan/mod.rs`. Blind spot: a phrase list, not a general check.
  - `test` `tui/src/plan/box_tests.rs:33`: `boxes.rs` must draw through `BoxWidget` and must not hand-build a box. Blind spot: the Plan boxes only.
- **Prose only:** `docs/implementing.md:104`: the area table is "the one source of truth" for test layout — enforced by `cli/tests/tooling/layout.rs:144`; `docs/implementing.md:62` and `docs/contract.md:2408` describe the pill's one source, enforced above.
- **Gap:** `docs/commands.md` is hand-written and is not generated or pinned to the binary. Only the `cpd …` commands the two skills name are checked to exist in it (`pi/skills/skill.test.ts:6`); nothing checks that a synopsis in `commands.md` still matches `cpd --help`, so it can restate a flag the binary no longer has. Evidence: `cli/tests/tooling/interface.rs:65` reads the binary and writes `docs/interface.md`; the only readers of `docs/commands.md` are `tui/src/logs/mod.rs:141` (a stale-phrase list) and `pi/skills/skill.test.ts:6` (skill commands exist in it), neither a synopsis-to-clap match. `no evidence` of a drift incident.
- **Proposal:** add a `test` in `cli/tests/tooling/` that parses every `cpd …` synopsis of `docs/commands.md` and checks each command path exists in `cli/src/interface.rs`'s `TABLE` (and each named flag in clap's `--help` for that path), the reverse of the skill check. Size M.
- **Verified:**
  - `timeout 60 rg -n 'fn the_contract_domains_table_is_the_code' core/src/policy/mod.rs` → `565`.
  - `timeout 60 rg -n 'fn the_contract_table_exposure_column_is_the_code' core/src/policy/exposure_tests.rs` → `105`.
  - `timeout 60 rg -n 'fn interface_page_is_generated_from_the_binary' cli/tests/tooling/interface.rs` → `65`.
  - `timeout 60 rg -n 'fn pill_spec_shipped_file' core/src/pill_tests.rs` → `10`.
  - `timeout 60 rg -n 'fn no_pill_cap_glyph|fn no_reverse_video_pill' cli/tests/tooling/pill_sites.rs` → `90`, `107`.
  - `timeout 60 rg -n 'fn no_stale_statement_is_left' tui/src/logs/mod.rs` → `146`.
  - `timeout 60 rg -n 'fn the_pr_boxes_are_drawn_by_the_box_widget' tui/src/plan/box_tests.rs` → `33`.
  - `timeout 60 rg -n 'docs/commands\.md' --glob '*.rs' | rg -i 'read_to_string|include_str'` → `tui/src/logs/mod.rs:141` (`include_str!`); `timeout 60 rg -n 'readFileSync.*commands\.md' --glob '*.ts' --glob '!node_modules'` → `pi/skills/skill.test.ts:6`.

### 6. Delete first, but check the fence

- **Enforced by:**
  - `script` `scripts/check-migrations-frozen.sh:22` (`migrations frozen check failed`): a file under `core/src/store/migrations/` that exists on the base is never modified, renamed or deleted; the Gate runs it (`package.json` `verify:steps`). Blind spot: migrations only.
  - `test` `core/tests/migrations.rs:521` (`the_frozen_check_fails_on_an_edited_deleted_or_renamed_migration`) and `:512` (`..._passes_on_no_change_and_on_a_new_migration`): pin the script's two directions. Blind spot: the script's own behaviour.
  - `test` `cli/tests/tooling/dead_code.rs:19`: deleting dead code is preferred; silencing it with `allow(dead_code)` needs a live reason. Blind spot: explicit `allow(dead_code)` only.
  - `test` `tui/src/logs/mod.rs:146`: stale statements are deleted, not left beside the code. Blind spot: a fixed phrase list.
  - `test` `tui/src/tree/scroll_tests.rs:207` (`the_tree_view_never_renders_the_widget_or_a_scratch_buffer`) and `tui/src/plan/box_tests.rs:33`: a deleted hand-made renderer must not come back. Blind spot: two views.
- **Prose only:** `scripts/repin.sh:1-2` describes the schema-hash fence in its header (the script is the enforcement); `docs/implementing.md` "Deliberately absent" and the "Checked and kept" pattern in the research notes carry the "find the reason it exists" practice in text.
- **Gap:** the fence exists only for migrations (and the `SCHEMA_HASH` pin `scripts/repin.sh` rewrites). A deletion or rename of any other append-only data (an Event kind name, a record kind name) has no fence: the migration tests (`core/tests/migrations.rs`) guard new migrations, not the removal of an existing name from code. Evidence: `rg -n 'frozen' scripts/ core/tests/migrations.rs` finds only `scripts/check-migrations-frozen.sh` and its tests; `no evidence` of a lost Event-kind incident.
- **Proposal:** add a `check` `scripts/check-frozen.sh` (or extend `scripts/check-migrations-frozen.sh`) that fails when a name present on the base in `core/src/events/kind.rs` or `core/src/store/migrations/` disappears from HEAD, and a `test` in `core/tests/migrations.rs` pinning both directions. Size S.
- **Verified:**
  - `timeout 60 rg -n 'frozen check failed' scripts/check-migrations-frozen.sh` → `22`.
  - `timeout 60 rg -n 'fn the_frozen_check_fails_on_an_edited|fn the_frozen_check_passes' core/tests/migrations.rs` → `521`, `512`.
  - `timeout 60 rg -n 'fn every_allow_dead_code' cli/tests/tooling/dead_code.rs` → `19`.
  - `timeout 60 rg -n 'fn no_stale_statement_is_left' tui/src/logs/mod.rs` → `146`.
  - `timeout 60 rg -n 'fn the_tree_view_never_renders' tui/src/tree/scroll_tests.rs` → `207`.
  - `timeout 60 rg -n 'frozen' scripts/ core/tests/migrations.rs` → only `scripts/check-migrations-frozen.sh` and `core/tests/migrations.rs`.

### 7. Explicit over implicit

- **Enforced by:**
  - `type`+`check` `core/src/config/file.rs:479` (`finish()`): every leaf key is named once in `Config::parse`; whatever the walk does not consume is refused as "not a known key" with its dotted key path. `test` `cli/tests/flow/config.rs:265` pins the refusal for an unknown key at the top and in every table. Blind spot: `$CPD_HOME/config.toml` only.
  - `type` `#[serde(deny_unknown_fields)]`: 34 occurrences in 12 files (e.g. `core/src/config/repo.rs:11`, `core/src/policy/domain.rs:35`, `core/src/context/record.rs:17`, `core/src/pill.rs:5`) make an unknown field of a parsed file an error. Blind spot: it is opt-in per struct; nothing forces a new `Deserialize` type to carry it.
  - `check` `scripts/check-portable.sh:87`: a script must be explicitly classified (covered or Linux-only, with a reason); nothing is implicit. Blind spot: scripts only.
  - `test` `cli/tests/tooling/interface.rs:65`: `docs/interface.md` is generated from `cpd --help`, so the page is derived, never hand-kept. Blind spot: one page.
- **Prose only:** `docs/contract.md:95`: "Unknown flags and unknown config keys are errors. Nothing falls back silently." — the rule in text; its enforcement is clap plus `core/src/config/file.rs:479`. `pi/skills/cpd-lead/SKILL.md` names the guard reasons (e.g. G2's 1500-character limit) beside the tool, but the guard is code (`pi/lib/askguard.ts`).
- **Gap:** `deny_unknown_fields` is remembered by hand. Evidence: 12 files carry it (`rg -l`), while 21 `.rs` files mention `Deserialize`; `#[serde(default)]` supplies an implicit value at 11 sites (e.g. `core/src/context/record.rs:29`), and no test ties such a default to a documented value outside the policy table. A new file parsed from disk can accept an unknown key silently if its author forgets the attribute. `no evidence` of an incident.
- **Proposal:** add a `test` in `cli/tests/tooling/` that scans `core/src` for `#[derive(... Deserialize ...)]` structs whose input is a file cpd parses (`config`, `policy`, `context`, `pill`, `plan/outline`, `secret/grants`) and fails when `deny_unknown_fields` is absent, or introduce a small derive helper that adds it by default. Size M.
- **Verified:**
  - `timeout 60 rg -n 'fn finish' core/src/config/file.rs` → `479`.
  - `timeout 60 rg -n 'deny_unknown_fields' --glob '*.rs' | wc -l` → `34`; `timeout 60 rg -l 'deny_unknown_fields' --glob '*.rs' | wc -l` → `12`; `timeout 60 rg -l 'Deserialize' --glob '*.rs' | wc -l` → `21`.
  - `timeout 60 rg -c 'serde\(default' --glob '*.rs' | awk -F: '{s+=$2} END {print s}'` → `11`; `timeout 60 rg -n 'serde\(default' core/src/context/record.rs` → `29`.
  - `timeout 60 rg -n 'fn config_refuses_each_bad_file' cli/tests/flow/config.rs` → `265`.
  - `timeout 60 rg -n 'not classified' scripts/check-portable.sh` → `87`.
  - `timeout 60 rg -n 'fn interface_page_is_generated_from_the_binary' cli/tests/tooling/interface.rs` → `65`.

## Not verified

- Axiom 1: I did not run the proposed manifest test; that a directly versioned dependency is absent today is unverified (I only counted `.workspace = true` uses, 16/11/13/13).
- Axiom 2: the claim that a live Worker prompt carries `AGENTS.md` is verified for this Attempt only (I read my own context); whether every role and every pi version does is unverified. The Distiller's live prompt was not read.
- Axiom 4: whether clippy is installed on the Gate's machine and which lints its toolchain supports is unverified.
- Axiom 5: whether `docs/commands.md` currently drifts from `cpd --help` is unverified; I did not diff them.
- Axiom 6: whether an Event-kind or record-kind name has ever been removed from the code is unverified (`no evidence` found).
- Axiom 7: which of the 21 `Deserialize`-mentioning files parse untrusted on-disk input (and so need `deny_unknown_fields`) is unverified; I read the 12 that have it, not the 9 that do not.
