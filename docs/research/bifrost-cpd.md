# bifrost-cpd: cpd and `bifrost/*` Tier models (spike-docs-a1)

Plan `bifrost`, Job `spike-docs`, Attempt 1. Host `pro`, pi 1.3.0, node v22.22.2, pi-bifrost-provider 0.2.0.

Labels: `verified` = I ran it in this Attempt (command and trimmed output given); `inferred` = I read the code and did not run it. The virtual key was only ever in the environment; no file, header or command line holds it.

Method (verified). Scratch agent dirs under `/tmp/sd-a1/<case>/` selected with `PI_CODING_AGENT_DIR`; the real `~/.pi/agent` was not written. cpd's module is run as `catalog.rs::read` runs it: `cd <agentdir>; PI_MANAGED_INSTALL_ROOT=<agentdir>/install node --input-type=module -e "$(cat <module>)" -- <pi-coding-agent dist/index.js> <pi-ai dist/index.js> O=… S=… H=… G=…`. Refs: `O=bifrost/vertex/claude-opus-5-5 S=bifrost/vertex/claude-sonnet-5-5 H=bifrost/vertex/claude-haiku-4-5 G=bifrost/vertex/gemini-3.8-flash`. Timings are wall clock of the whole `node` process, three runs, in order.

---

## Q1. pro-n4: does the catalog check find `bifrost/*`?

### Fact that shapes the answer (verified)

Bifrost's `/openai/v1/models` carries no reasoning data: every entry has exactly the keys `id`, `object`, `owned_by` (47 models listed). The package therefore registers every model with `reasoning: false`, and pi-ai reports `["off"]` for them (see the candidate table: `["off"]` for all four). cpd turns `off` into effort 0, so a catalog built from the package alone has `efforts [0]` for O/S/H. **Resolving the refs is not enough; the thinking levels (O up to `max`, S and H up to `high`, which the Worker range H3, S0-S3 needs) must come from somewhere else.**

### (a) `catalog.mjs` as shipped

| setup | `settings.json` | `models-store.json` | O | S | H | G |
|---|---|---|---|---|---|---|
| s1 | `{}` | empty | unknown | unknown | unknown | unknown |
| s2 | package | empty | unknown | unknown | unknown | unknown |
| s3 | `{}` | populated (47 models, from an earlier live refresh) | unknown | unknown | unknown | unknown |
| s4 | package | populated | unknown | unknown | unknown | unknown |

verified: each printed `{"O":null,"S":null,"H":null,"G":null}`. pro-n4 is confirmed: the shipped module builds `ModelRuntime.create({allowModelNetwork:false})`, which never loads settings packages, so the extension-only provider `bifrost` does not exist for it, whatever the store holds. In cpd this is `tier_model_unknown`.

### (b) Candidates

`c1.mjs` (scratch, not in the repo) replaces `ModelRuntime.create` by `agent.createAgentSessionServices({cwd, agentDir})` (loads settings packages/extensions) and then `runtime.refresh({allowNetwork:false})`, `getModel`, `getSupportedThinkingLevels`.

| # | candidate | O | S | H | G | network | three runs (s) | median |
|---|---|---|---|---|---|---|---|---|
| 0 | shipped module, `github-copilot` refs (baseline, earlier run) | found | found | found | found | no | not re-measured | – |
| 1 | `c1.mjs`, package in settings, store **empty**, no env | unknown | unknown | unknown | unknown | no | 0.44 0.43 0.44 | 0.44 |
| 3 | `c1.mjs`, package in settings, store **populated**, no env (offline restore) | found `off` | found `off` | found `off` | found `off` | no | 0.45 0.44 0.45 | 0.45 |
| 2 | `c1.mjs`, package, store empty, `BIFROST_URL=http://127.0.0.1:8788` + key in env (discovery at load) | found `off` | found `off` | found `off` | found `off` | yes | 0.92 0.90 1.10 | 0.92 |
| 4a | **no extension**: static `providers.bifrost` in `models.json` (4 models, `reasoning: true`, `thinkingLevelMap`), **shipped** module | found off low medium high xhigh max | found off low medium high | found off low medium high | found `off` | no | 0.42 0.39 0.40 | 0.40 |
| 5 | `c1.mjs` + package + store populated + `models.json` with the same overrides | found off…max | found off…high | found off…high | found `off` | no | 0.44 0.45 0.45 | 0.45 |

Notes:
- verified: candidates 1/3 differ only in the store, so `refresh({allowNetwork:false})` does restore the `bifrost` catalog from `models-store.json`, but only after a live refresh persisted it. A fresh machine gets `unknown`.
- verified: with the package alone (1–3) the levels are `off` only, so `H3`, `S1`–`S3` would be `tier_invalid`.
- verified (earlier in this Attempt, no figures kept): candidate 4a answers a live request through plain pi (`pi --print --no-session --no-extensions --model bifrost/vertex/claude-haiku-4-5:low …` → a reply). It needs `apiKey: "$BIFROST_VIRTUAL_KEY"`: pi sends `Authorization: Bearer <apiKey>`, and Bifrost accepts `Authorization: Bearer <vk>` or `x-bf-vk: <vk>`; a wrong Bearer next to a right `x-bf-vk` gave 401. The key is read at request time from the environment, not written into `models.json` (`grep -c -F` of c4's `models.json` prints 0).
- verified earlier: a static `models.json` entry replaces the package's discovered entry of the same id and its defaults (128K context), so static Tier models need their own `contextWindow`/`maxTokens`.
- verified earlier: from a node process, `https://192.168.3.100:8787` fails with `UNABLE_TO_GET_ISSUER_CERT_LOCALLY` unless `NODE_EXTRA_CA_CERTS` points at the Caddy root certificate, and works with it. hs and air need that in pi's environment.
- verified earlier: the package downloads models.dev (about 6 MB, cached 24 h) on discovery.
- Not measured: whether Bifrost actually applies the effort to the Vertex Claude models (the request is accepted; unverified).

### (c) Bifrost unreachable (c1.mjs, package in settings; each time `models-store.json` reset to empty before the run)

| case | result | three runs (s) | stderr |
|---|---|---|---|
| `BIFROST_URL=http://127.0.0.1:9` (closed port) | module ends, exit 0, all four `null` | 0.47 0.44 0.43 | `pi-bifrost-provider: startup model discovery failed, continuing without a preloaded catalog: fetch failed` |
| same, store populated | ends, found `off` (restored) | 0.46 0.44 0.48 | same line |
| reachable, wrong key | ends, all `null` | 0.78 0.53 0.54 | `…discovery failed (401): authentication is required. Provide a virtual key (x-bf-vk)…` |

verified. A failed discovery never throws: the provider has no models, `getModel` is undefined, the module prints `null`, and cpd would say `tier_model_unknown` ("the model does not exist") when the truth is "Bifrost is down". The notice goes to stderr, so stdout stays the JSON cpd parses. A blackhole address (not re-run) took about 11 s earlier, and one live discovery hit the package's 15 s timeout once; the gateway is intermittently slow, so any fix that needs the network at catalog time is nondeterministic (verified earlier).

### (d) Recommendation

**Declare the four Tier models statically in pi's `models.json` (rendered by the dotfiles PR) with their `thinkingLevelMap`, `contextWindow`, `maxTokens`, and `apiKey: "$BIFROST_VIRTUAL_KEY"`; leave `catalog.mjs` unchanged.** It is the only candidate that finds all four refs with the full levels, needs no network, key, extension or store state, and costs the baseline time (0.40 s median). Cost: Tier model metadata is hand-maintained in dotfiles, and `baseUrl` is a literal in `models.json` (inferred: pi's docs interpolate env only in `apiKey`/headers), so per-machine URLs (loopback on pro, LAN on hs/air) must be rendered into both `models.json` and `config.toml`'s `bifrost_url`, or `bifrost_url` is only the allowance flag. Decide that before the PRs.

Changes in cpd for it (inputs, not a design):
- `catalog.mjs`: none.
- `catalog.rs`: `load`, `load_with`, `load_from` and `read` take `config.tiers.{o,s,h,g}` (`catalog.rs:126,133,137-139,148-155`); they must take the provider's four models instead.
- `doctor` `tiers` (`engine/src/doctor/pi.rs:18-33`): reads the catalog once, uncached, and checks only `lead_tier`. It must check each provider the machine allows (github-copilot; bifrost when `bifrost_url` is set), both `lead_tier` and `manager_tier`, and for bifrost say "pi's models.json lacks `bifrost/<id>`" rather than `tier_model_unknown`.
- If the package is chosen anyway (candidates 2/3): `catalog.mjs` must call `createAgentSessionServices` with the package, cpd must pass `BIFROST_URL`/the key to the module, and a `models.json` with `modelOverrides` is still required for the levels, so it is strictly more work than 4a.

verdict: the shipped `catalog.mjs` finds none of the four refs in any of four setups (pro-n4 confirmed); the package alone cannot fix it because Bifrost reports no reasoning levels; the smallest correct fix is a static `models.json` provider with the shipped module unchanged (found, O off…max, S and H off…high, no network, 0.40 s median).

---

## Q2. Where cpd picks a model or reads the Catalog

Command (verified): `rg -n "tiers\.|Catalog|model_and_level|load_catalog" core/src engine/src cli/src`. `cli/src` has no hit. Hits in `core/src/store/*` are the schema `store::Catalog`, a different type; `config/{tier,cache,file}.rs` define the type. All rows are inferred from reading those lines. Owner path: Session `sessions.context` / Attempt `attempts.plan` / Job `jobs.plan` / PR → `plans.context` → `contexts.owner` (inferred from the schema in contract §4.4; `contexts.owner` may be NULL for older Contexts, `owner_unset`).

| file:line | role / caller | what it does | Context reachable from the row | needs to choose a provider |
|---|---|---|---|---|
| `core/src/session/row.rs:264-266` `model_and_level` | Manager, Lead (shared by `session show` `:344` and `session run` `run.rs:370`) | `config.tiers` by letter | row's `context` (args: config, session id, tier) | the Context's provider; the provider's four models |
| `core/src/driver/worker.rs:320,327` | Worker Attempt and review Worker (fork of the Lead, same `start`) | `catalog.model_arg(target.tier)` | `target.plan` → Context → owner | per-provider catalog and env |
| `core/src/driver/run.rs:155,268,288,360,647,685,735` | Attempt driver | `load_catalog(&config)` once; ladder, merge, worker-ended check | driver's Plan → Context → owner | the Plan's catalog |
| `core/src/driver/mod.rs:57`, `driver/start.rs:31,86,90,121,145,196` | `job dispatch`, E3 `decide`, ladder (`catalog.next`) | machine-wide `Catalog` in `StartCtx` | `plan`/`key` of the Job | the Plan's catalog; `StartCtx` cannot hold one global catalog |
| `core/src/attempt/ledger.rs:194,373` | attempt end, next Tier | `ladder.catalog.next(attempt.tier)` | `attempt.plan` → … | the Plan's catalog |
| `core/src/merge/mod.rs:36,72`, `merge/recover.rs:44` | merge, orphan recovery | `Ctx.catalog` → `Ladder` | the Attempt's Plan → … | same |
| `core/src/pr/rebase.rs:488-493` | `cpd pr rebase` repair (`settle`) | `config::load_catalog` lazily | the `plan` argument → … | same |
| `engine/src/watch/engine.rs:54,107-118` | engine, cached per tick | `load_catalog(&self.config)` | **none**: machine-wide | becomes per provider (map) or per Plan |
| `engine/src/watch/liveness.rs:170,194` | E2 dead driver, orphaned merge | `engine.catalog()` | the dead Attempt's Plan → … | the Plan's catalog |
| `engine/src/watch/schedule.rs:116` | E3, Jobs of all Plans in one tick | one catalog for all | each `(plan, key)` has its own owner | catalog per Plan |
| `core/src/job/mod.rs:218` `check_tier`, from `job/add.rs:69`, `edit.rs:59`, `retry.rs:27` | `job add|edit|retry --tier` | `catalog::load_with(&self.config, &mut self.store)?.check(tier)` then `check_worker` | `args.plan` (`CPD_PLAN` or `--plan`) → Context → owner | the Plan, before the catalog |
| `core/src/session/retitle.rs:22,227-228` | title writer | `load_catalog`, `model_arg(Tier G0)`, spawns `pi --print --no-session --no-extensions --model <G>`; removes only `CPD_*` from env | the titled Session id → `sessions.context` → owner | provider's G model; env; `-e` for the package route |
| `core/src/context/distill.rs:481-488` | distiller | `self.config.tiers.{o,s,h,g}` by letter, **no catalog check**; removes only `CPD_*`, adds `launch.env` | `self.context` → `contexts.owner` | provider's model and env |
| `core/src/config/mod.rs:46` `tiers` (`cpd tiers`) | CLI | `load_catalog(&config)`, prints O/S/H/G | **none** | print per allowed provider |
| `core/src/config/mod.rs` `config` (`cpd config`) | CLI | prints `Config` | none | only shows the new `bifrost_url` key |
| `engine/src/doctor/pi.rs:18-33` | `cpd doctor` `tiers` check | `read_catalog` uncached, `check(config.lead_tier)` | **none** | each allowed provider; `manager_tier` too |
| `core/src/config/catalog.rs:126-155`, `cache.rs:17-63` | the only reader/cache | models come from `config.tiers` | none | the models as an argument |
| Tier only, no model: `core/src/plan/new.rs:78` (`lead_tier`), `core/src/context/manager.rs:156` (`manager_tier`), `session/retier.rs`, `engine/src/watch/open.rs`, `pr/poll.rs`, `plan/review.rs` | Lead/Manager creation, open/update Jobs | write the Tier into rows | – | nothing now; the model is chosen later by `model_and_level` or the Worker start. `lead_tier`/`manager_tier` are not validated against a catalog except `lead_tier` by doctor |

Callers with no Context: `cpd tiers`, `cpd config`, `doctor`, `Engine::catalog` (machine-wide), `catalog::load`. `tiers` and `doctor` should iterate the providers the machine allows; the engine should hold one catalog per provider and pick it by the Plan of the row it works on. A `Ladder` must never mix providers.

verdict: 11 families of call sites load or read the machine-wide catalog or `config.tiers` (none in `cli/`); all but `cpd tiers`, `cpd config`, `doctor`, `Engine::catalog` and `catalog::load` reach a Context through a row they hold; the machine-wide `Catalog` in the engine, drivers, `StartCtx` and `Ladder` is the structural change. Open decision: whether `config.tiers` stays "the personal provider's models" with Bifrost's four as built-ins (A4), or a second `[…]` table; either way one source of truth per provider.

---

## Q3. The `tier_catalog` cache

**It does not thrash and needs no schema change.** verified: with the installed cpd, `cpd tiers` with `[tiers]` set to the Bifrost refs and `models.json` as in 4a returned O `[0,1,2,3,4,5]`, S/H `[0,1,2,3]`, G `[]` (`"ok":true`), and a second run with the default `github-copilot` refs then left both sets of rows in `tier_catalog` (read with `sqlite3 … "select … from tier_catalog"`: rows keyed by `(1.3.0, O|S|H, <model>)` for both providers). Why (inferred from `core/src/config/cache.rs`): `read` selects each model by `pi_version = ?1 AND letter = ?2 AND model = ?3` (`:33`); `write` is `INSERT OR IGNORE INTO tier_catalog (…)` (`:55`); nothing deletes. Models differ by provider prefix (`bifrost/…` vs `github-copilot/…`), so keys differ and the key already includes the provider. No provider column, migration or protocol bump (contract §4.5) is needed. The only code change is that `load`/`load_from` take the provider's models.

Risk (staleness, not thrash): the key is `(pi_version, letter, model)`, but the efforts depend on pi's catalog **and** on the machine's `models.json`. `INSERT OR IGNORE` never updates a row, so after dotfiles changes a `thinkingLevelMap` the cached efforts stay wrong until pi's version or the model string changes. verified earlier: after reducing the levels in `models.json`, `cpd tiers` still printed the old levels while the uncached module printed the new ones. Under the package route a cached `[0]` from a transient missing override would stick too. Mitigation: `doctor`'s uncached read already catches drift (`doctor/pi.rs:14-17`, "never read from the cache"); a fingerprint of `models.json` in the key (migration and protocol bump) is a possible follow-up, not needed now. (Not verified: that the table refuses deletes; the sqlite CLI cannot run cpd's `cpd_txn()` trigger function.) The doc comment on `catalog.rs` that says rows are replaced is wrong.

verdict: keep the cache and schema as they are; the provider is already in the key through the model string; change only `load` to take the provider's models; stale rows are `doctor`'s uncached check to catch.

---

## Q4. Tier commands and the provider of a Context

Today (inferred): `check_tier` (`core/src/job/mod.rs:218-221`) loads the one machine-wide catalog and calls `check(tier)` then `check_worker()`; it never looks at the Plan or Context, so it would check a Bifrost Plan's Tier against the wrong provider. Verified: with the Bifrost refs and no `models.json`, `cpd tiers` answers `tier_model_unknown`, which is a wrong description of "Bifrost is not available here".

| situation | `job add|edit|retry --tier` should refuse with | exit |
|---|---|---|
| owner policy says bifrost, `bifrost_url` empty on this machine | `bifrost_not_allowed`, **before** the catalog | 3 |
| Context has no owner | `owner_unset` (exists) | 3 |
| no policy file / invalid file | `owner_unknown` / `policy_invalid` (exist) | 1 |
| key unset, `bifrost_url` set | no refusal: with the static `models.json` catalog nothing needs the key; the commands start nothing | – |

Exit codes (verified in `core/src/error.rs:102-103,203-204,465-469`): `policy_invalid` and `owner_unknown` are 1, `owner_unset` and `owner_mismatch` are 3 (`Refused`). Both new codes are unmet preconditions the User fixes, so exit 3, like `owner_unset`; neither falls back to github-copilot.

**Earliest refusal:** `context new --owner …` and `context owner` (they resolve the owner's policy already, inferred; `core/src/context/owner.rs` exists), since Contexts are machine-local; repeat the check at `plan new`, `context manager`, `session run`, the Tier commands, E3/`job dispatch`/`attempt run`, title writer and distiller, because the config or policy can change later and older Contexts exist.

**One function** (an input, not written):

```rust
pub enum Provider { GithubCopilot, Bifrost { url: String } }
pub fn provider_of_context(home: &CpdHome, config: &Config, owner: Option<&OwnerLogin>) -> Result<Provider, CpdError>;
impl Provider {
    pub fn models(&self, config: &Config) -> Tiers;            // the provider's four refs
    pub fn pi_env(&self) -> Result<BTreeMap<String, String>, CpdError>; // BIFROST_URL, key, empties; may return bifrost_key_unset
}
```

It resolves the **owner** policy (`PolicyKey::Owner`, never the Worker's `Repo` key) and returns `owner_unset` (3), `owner_unknown` (1), `policy_invalid` (1), `bifrost_not_allowed` (new, 3; details `context`, `owner`, `key: "bifrost_url"`), and, from `pi_env`, `bifrost_key_unset` (new, 3; details `variable`, never a value; not `config_missing`, which is exit 1 and about a file). Call sites: every row of the Q2 table marked "needs a provider"; `pi_env` only where a pi is started (`session/run.rs`, `driver/worker.rs`, `retitle.rs`, `distill.rs`).

verdict: the three Tier commands should refuse `bifrost_not_allowed` (exit 3) before the catalog check, and not refuse on an unset key; `bifrost_key_unset` (exit 3) belongs where a pi starts; the earliest refusal is `context new`/`context owner`; one resolver `provider_of_context`.

---

## Q5. A2–A6 checked

1. A2, inferred: the loader does **not** support owner-only domains. `parse_scope(file, text)` and `load_scope_file(path)` (`core/src/policy/load.rs:26,71`) take no scope kind and parse one `Scope` for owner and repo files alike. `[model]` set in a repo file would parse. For `policy_invalid` the loader needs the scope kind, and contract §8.3 and its test must name it.
2. A2, inferred: `domains!` (`core/src/policy/domain.rs:1-7`) is "one line per domain" with a text and audience per domain; a typed-only, text-less `model` domain may not fit it without a change. Not verified beyond the header comment.
3. A2, inferred: §8.3 resolves Workers with a `Repo` key; the provider must be resolved with `Owner(contexts.owner)` for every role.
4. A2, verified: `~/.cpd/policy/owners/` holds only `clayton-duarte.toml`; a `missionlane` file does not exist yet, so `context new --owner missionlane` is `owner_unknown` until dotfiles render it.
5. A3, inferred: `models.json` `baseUrl` has no env interpolation (verified runs used a literal), so under the static route the URL lives in two places (see Q1 (d)).
6. A3, verified earlier: hs/air need `NODE_EXTRA_CA_CERTS` for `https://192.168.3.100:8787`; nothing in A1–A6 sets it.
7. A4, verified: the four refs have no reasoning levels from Bifrost; the levels must come from `models.json` (Q1). Unverified: that Bifrost applies the effort.
8. A4, inferred: the ladder and Worker range are defined on one machine-wide catalog (Q2); it must be per provider. `lead_tier` and `manager_tier` have no catalog check except `lead_tier` in doctor.
9. A5, inferred: `launch.rs` `base_env` (`:77-84`) gives only `PATH`, `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `PI_SKIP_VERSION_CHECK`; the Manager has its own map (`lead_env`, `:123-`, plus plannotator vars); cpd adds these to tmux as `-e K=V` (`host/tmux.rs:418-426`). So cpd's own `BIFROST_VIRTUAL_KEY` reaches a pane **only if cpd adds it**, and a pane otherwise inherits the tmux server's environment (the tmux inheritance is my inference from tmux's documented behaviour; I did not run it). Hence the explicit empties of A5 are needed, and `BIFROST_API_KEY` too (a wrong Bearer beside a right `x-bf-vk` is 401, verified).
10. A5, inferred: `-e K=V` puts the key in the `tmux` client's argument list while it runs (visible in `ps`). Conflicts with D10's "never in an argument" for pane Sessions; decide an alternative (inherit, or file-based) before the cpd PR.
11. A5/A6, inferred from `retitle.rs:230-232` and `distill.rs:497-499`: the title writer and distiller spawn pi inheriting cpd's whole environment minus `CPD_*` (the distiller then adds `launch.env`), so a personal Context's auxiliary could inherit `BIFROST_*` from the engine's shell; set them explicitly there too.
12. A5, verified (candidate 3): "inert in personal Contexts" is only partly true: with `BIFROST_URL` empty nothing is discovered, but a populated `models-store.json` still restores the 47 models into the picker. They cannot be used, but they are visible.
13. A6, inferred: `core/src/host/pi/packages.rs:167` `extensions_to_pass` already implements the `-e` rule (skip a package already declared in settings), used for Leads/Workers (`launch.rs` `push_extensions`); the title writer builds its argv by hand (`retitle.rs:230`) and has no `-e`. Under the package route both auxiliaries need it; under the static `models.json` route (Q1 4a) they need no `-e` (verified earlier: `--no-extensions` + the scratch `models.json` answers).
14. A6, verified: with the package, every auxiliary run does a live discovery at load (0.92 s vs 0.40 s, and an empty provider on timeout); the static route has none.

verdict: A1–A6 hold in intent; five points need a decision before the PRs: the owner-only `[model]` domain needs a scope-kind check and does not fit "one line" (1–3), the levels cannot come from Bifrost and need a `models.json` block (7), the `Catalog` must become per provider (8), a pane's environment is what cpd adds so the key must be passed explicitly and then sits in an argument (9–10), and hs/air need `NODE_EXTRA_CA_CERTS` (6). The static `models.json` route removes 5, 13 and 14's package cost and is the simplest base.
