# SWR: the models datasource (`swr_models`)

Status: normative for the models Workflow (Plan `external-sync-arch`). The shared rules (the two clocks, the status axes, the outbox, the fence) are in [swr.md](swr.md). The table DDL is [contract §4.4](../contract.md) ("The SWR tables"). The step is E15 in [contract §3.7](../contract.md). Decisions: pro-d448 (all sources, one DAG), pro-d440 (architecture), pro-d13, pro-d16, pro-d17, pro-d20 (model discovery).

Code references are to `main` as read for this page: `core/src/models/cache.rs`, `helper.rs`, `cmd.rs`, `engine/src/watch/models.rs`, `core/src/config/file.rs`. The cutover Job changes them.

## 1. What moves

- **The cache** moves from one file per provider, `run/models-<provider>.json`, to the `swr_models` table: one row per provider.
- **The fetch does not move.** It stays the node helper `pi/lib/models-list.ts`, run through the `Runner` port with a 120 s deadline (`core/src/models/helper.rs`, `DEADLINE`). The helper does I/O only (swr.md A7).
- **The step is E15.** Its body becomes the models Source, `core/src/swr/source/models.rs`, behind the Source trait. E15 runs it the way E5 runs `github` (swr.md §12). E24 runs the other sources.
- **The readers** read the table (§5).
- **No migration here.** `swr_models` is created by swr-core's one additive migration (A1). The models Workflow retires no table, so it adds none.

## 2. Table mapping

`swr_models (provider PRIMARY KEY, catalog_json, changes_json, polled_at, last_error, inactive_at)` holds one row per provider.

| Today: `run/models-<provider>.json` | Tomorrow: `swr_models` | Rule |
|---|---|---|
| file name `models-<provider>.json` | `provider` (primary key) | the token itself, `^[A-Za-z0-9._-]+$`. The helper refuses any other name as `models_unparseable` |
| `ids`, `known?`, `source`, `pi_version` | `catalog_json` = `{"pi_version", "source": {"kind", "auth", "base"}, "ids": [...], "known"?: [...]}` | the last **good** list. A good list replaces it whole. Ids are sorted, as the file had them |
| `changes?` | `changes_json` = `{"at", "appeared", "went_away"}`, or NULL | the diff of the last change (§2.1). NULL when there is none |
| `polled_at` | `polled_at` | the time of the last **good** list (contract §3.7 E15). It moves on every good list |
| `version` | none | the file's format number. The table has one shape, written by one function |
| none | `last_error` | `<code>: <message>` of the provider's last failed run. NULL after a good list |
| none | `inactive_at` | when no observer needs the row (§3.3) |

**Validation at the edge.** The engine parses the helper's answer once, as contract §3.7 E15 says, and writes rows from the validated values only. A row that cannot be parsed back is a store fault (`store_failed`, loud), not a provider error and not a warning.

### 2.1 Writes

- **One transaction per provider.** The write reads the provider's row, computes the diff and writes, in one `Store::write` (a `BEGIN IMMEDIATE`, contract §4.2). That replaces the `.tmp` and rename and `locks/models.lock`: the store admits one writer at a time, so the engine and `cpd models refresh` never interleave their diffs.
- **The diff** is the one contract §3.7 E15 states, per provider and per `auth`:
  - no row, or the row's `auth` differs: `changes_json` = NULL. The lists are not comparable, so there is no change (a first run announces nothing);
  - the same ids as the row's `catalog_json`: `changes_json` is kept (the last change, not the last poll) and `polled_at` moves;
  - different ids: `changes_json` = `{"at": now, "appeared": [...], "went_away": [...]}`, sorted.
- **Per provider, by the helper's outcome:**

| Outcome for the provider | Row | Endpoint (§4) | Event |
|---|---|---|---|
| good, non-empty `ids` | the table above; `last_error` NULL; `inactive_at` NULL | `record_ok` | none (one log line) |
| failed, or `models_empty` | `last_error` set; every other column kept | `record_failure` | `error`, once per outage (§4) |
| not in the answer | `inactive_at` set if no observer (§3.3); otherwise untouched | none | none |

- **A whole-run failure** (`pi_not_found`, `spawn_failed`, `tool_timeout`, `models_helper_failed`, or a whole-run code) names no provider and writes no row. It keeps the existing step rule (contract §3.7, a step's failure): one `error` per code until it clears, repeated at most once per `watch.error_repeat_seconds`.

## 3. Due and gating rules

E15 evaluates these at each tick, with `now` as the engine's clock passed in (swr.md §16).

1. **Due** when `swr_models` has no row, or the newest `polled_at` among its rows is at least `watch.models_discovery_seconds` before `now`. The default is 604800 s, seven days (`core/src/config/file.rs`). The seven days count from the stored `polled_at`, never from the engine's start, so a restart repeats nothing. The run's one query key is `models:discovery`.
2. **One healthy provider resets the week.** A good list moves its `polled_at` to `now`, so the newest one moves. A failed or empty provider keeps its `polled_at`, so it resets nothing.
3. **Gate.** After **any** run end (a run that wrote, failed or did not start), the next run is not before `watch.error_repeat_seconds` after that end. The default is 600 s (`core/src/config/file.rs`). The end is kept in engine memory, as E15's `last_end` is today. An engine restart may run at once. That is harmless: a run is idempotent and the week is durable.
4. **One run at a time.** The run is detached on its own thread (E15's `running`) and collected at a later tick, so E15 never holds the tick.
5. **Endpoint pauses do not gate the run.** The helper takes no argument, so one run lists every provider. Open item §9.
6. **`cpd models refresh` is not gated.** It runs when asked. A refresh that writes moves `polled_at` as the engine does, so the engine's next run falls due a week after it.

### 3.1 Why models keeps no `swr_queries` row (deviation from swr.md §6)

swr.md §6 keeps freshness and change in `swr_queries`. The models Source keeps both in `swr_models`: its freshness is `polled_at` (rule 1, durable), its change is `changes_json` (an id diff, not a data hash) and its failure is `last_error`. Copying them into `swr_queries` would keep one fact in two places. So the models Source does not call `cache::commit`, `cache::fail` or `cache::settle`. It has no outbox mutation to settle.

### 3.2 Why an unchanged list writes its row (deviation from D5)

D5 says unchanged data opens no write transaction and that freshness lives in memory. The week cannot live there: rule 1 reads the durable `polled_at`, and contract §3.7 E15 says a restart repeats nothing. So each good list writes its row. That is one small transaction per provider per week, and it is the only write of an unchanged list.

### 3.3 Observers and `inactive_at`

- A row's **observers** are the providers the latest answer lists, and any open `models-changed` Inbox item that names the provider (subject `models:<provider>/<changes.at>`).
- A provider the answer no longer lists, with no open item, is marked inactive (`mark_inactive`: `inactive_at` = now). When it is listed again it is touched (`inactive_at` NULL).
- `gc::sweep` deletes a row whose `inactive_at` is older than `swr.gc_days` (default 7, contract §8.2) and that has no observer. **This is a change:** a provider file is never deleted today.

## 4. Provider failures and the `error` Event

A failed or empty provider feeds `swr_endpoints` (swr.md §7), one endpoint per provider host:

- **Endpoint name.** `github-copilot` for the Copilot provider. For a custom provider, the `host:port` of its `base`, scheme dropped. The helper gives `base` as an origin, never a path. (Assumption: the Copilot endpoint is named by its provider, because the answer does not carry Copilot's host.)
- **Kind** (this page's choice; the Lead can change this one table):
  - Transient: `models_unreachable`, `models_timeout`, `models_refresh_failed`, `models_empty`. An outage can heal by waiting.
  - Permanent: `models_key_unset`, `models_unauthorized`, `models_tls`, `models_unparseable`. Waiting does not heal them.
- **Calls.** Each failing provider: `endpoint::record_failure(endpoint, now, error, kind)`. Each good provider: `endpoint::record_ok`. The models Source never calls `record_rate` or `pause_until`, because the helper reports no rate limit.
- **The report.** `endpoint::due_report` is true once per outage: a transient outage after 10 minutes of continuous failure (A3), a permanent one at once. The report is the existing `error` Event through `engine/src/watch/report.rs`: `source` `watch`, `code` the helper's code, `details` `{provider, endpoint}` plus the helper's details. No key is ever in it. No new Event kind (A3).
- **Unparseable catalog.** A provider's `models_unparseable` is a permanent failure, so it is reported once. It replaces the file-level report, which had no equivalent once the file is gone.
- **Recovery.** A good list clears the outage (`record_ok`). The next failure starts a new outage and a new report.

## 5. Readers

Every reader goes through `core::models`, which reads the table through the store with `now` passed in. None reads a file.

| Reader | Today | After cutover |
|---|---|---|
| `cpd models show [--provider]` (`core/src/models/cmd.rs:75-101`) | every `run/models-*.json`; an unparseable file is named in `warnings` | the rows, one per provider; `--provider` filters the rows. The output shape does not change: `providers[]` with `provider`, `polled_at`, `age_seconds`, `pi_version`, `auth`, `count`, `ids`, `changes`. `warnings` stays and is `[]` (open item §9). `show` opens the store, runs no helper and writes nothing |
| Inbox `models-changed` (`core/src/inbox/rules.rs:798-853`) | each file whose `changes.at` is within `models::WINDOW` gives one white item (`core/src/inbox/models.rs:8-9`, 7 days) | each row whose `changes_json.at` is within the window and whose announced part is not empty gives one white item. The subject `models:<provider>/<changes.at>`, the label and the announced part (contract §2.2) are unchanged. A provider with no row keeps its open items, as a missing file does today |
| TUI dialog (`tui/src/widgets/inbox.rs:175-200`, `models_body`) | `read_all` over the files, the change matched on `at` | the row's `changes_json`, matched on `at`. When the row holds no such change: `changed again; the item closes at the next tick`, unchanged |
| `cpd models refresh` (`core/src/models/cmd.rs`) | writes the files | writes the rows (§2.1) |

## 6. Retired file glob

- **`run/models-*.json`**: every provider file, `run/models-<provider>.json`, and its `.tmp` sibling. After the cutover cpd reads and writes none of them.
- **`locks/models.lock`**: retired with the file writes (§2.1).

cpd deletes neither. A live home keeps its old files until someone removes them. Nothing reads them, so they are inert. Contract §1.4 drops both entries (§8).

## 7. Cutover

One PR (swr.md §16, "Cutover is one PR"). It moves the readers (§5) and the writer (§2.1) together, and removes `read_all`, the file `Read` type, the `.tmp` and rename, and `take_lock` in `core/src/models/cache.rs`. No dual write, no flag.

**No import.** The table starts empty. The first E15 run after the cutover is due at once (no row) and fills it. No file is read into the table. The consequences are:

- `cpd models show` lists nothing until that first run. A home with no file shows the same today.
- **Open `models-changed` items from the file era close at that run.** The first write of a provider has no `changes` (§2.1), so the row announces nothing, and the item's condition is gone. The User loses that notice, and no item is recreated. This is the cost of not importing.

## 8. Contract and command amendments

Made in this Job, in the four files the Job may touch:

- **`docs/contract.md` §1.4:** the `models-<provider>.json` and `models.lock` entries are retired, with a pointer to §4.4 and this page.
- **`docs/contract.md` §2.2:** the `models-changed` row and "The models-changed rule" read the row's `changes_json`, not a file.
- **`docs/contract.md` §3.7 E15:** the row states the models Source, the due and gate rules, the writes and the failure rules, and points here for the detail. Its helper invocation, validation and error codes are kept.
- **`docs/contract.md` §4.4:** the `swr_models` DDL carries a comment that points here.
- **`docs/contract.md` §4.7:** E24 runs every source except `github` (E5) and `models` (E15).
- **`docs/contract.md` §8.2:** `watch.models_discovery_seconds` counts from the newest row, and a missing row makes the step due at once.
- **`docs/contract.md` decisions table:** the model-discovery row cites §4.4 (`swr_models`) and this page.
- **`docs/commands.md` §14.5:** `models show` and `models refresh` read and write rows through the store. Neither uses a lock file. `show`'s `warnings` is `[]`.
- **`docs/commands.md` §13.2:** the `models-changed` dialog reads the row.
- **`docs/architecture/swr.md` §12 and §14:** E15 runs the models Source; the models query key is `models:discovery`; endpoints are per provider (§4 here).

## 9. Open items

Not decided by this page or the spec. The models Workflow or the Lead decides each.

- **Static endpoints.** `Source::endpoints()` returns `&'static [&'static str]`, but a provider's endpoint is known only from the answer. The stub returns `["models"]`. The models Workflow returns `[]` and names endpoints at run time. If E24's pause check ever needs them, the trait returns owned names (swr.md §11). E15 does not check pauses, so nothing waits on this.
- **Pauses do not gate the run** (§3 rule 5). The helper cannot skip one provider.
- **`watch.models_discovery_seconds` stays as the models staleTime.** swr.md A4 says staleTimes are code constants, so this is a deliberate exception. The key is in users' configs with a default of 604800. Retiring it is a config migration, for the models Workflow or a later Job.
- **`warnings` in `models show`** stays `[]` until someone removes the field from the output, which is a separate change.
- **Endpoint naming and the Transient/Permanent split** are this page's choices (§4).
