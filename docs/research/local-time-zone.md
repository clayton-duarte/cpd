# Finding and naming the machine's time zone

Job `research-zone`, Plan `local-time`. Goal: how cpd finds the machine's time zone and its name, and prints a time in prose as `YYYY-MM-DD HH:MM EDT`, in Rust and in TypeScript. Decisions already taken (Context records pro-d41, pro-d42; assumptions A2, A3): a time in a field stays ISO 8601 UTC with `Z` (`docs/contract.md` §1.3); a time in prose is machine-local with the zone abbreviation; the zone is the system's (`TZ`, then `/etc/localtime`), no config key; tests mock the zone.

**Evidence labels.** *verified* = I ran it or read the source on this machine (macOS 25.6 / Darwin, tmux 3.8, node v22.22.2, rustc 1.99.1, system zone `America/Toronto`, `/etc/localtime -> /var/db/timezone/zoneinfo/America/Toronto`) on 2026-10-25. *unverified* / *guess:* / *I infer* = not run. Crate versions are the ones in `~/.cargo/registry` (`jiff 0.2.35`, `time 0.3.55`, `chrono 0.4.45`, `iana-time-zone 0.1.65`); the Linux homeserver was not available, so every Linux claim is from source only.

## 1. Rust: the candidates

I built a throwaway crate outside the worktree (deleted afterwards) that printed, for the instants `2026-11-01T05:30:00Z`, `2026-11-01T06:30:00Z` (both sides of the fall-back change in Toronto; 2026-11-01 is the fall-back date, so both are 01:30 local) and `2026-01-15T12:00:00Z`, what `jiff`, `time` and `chrono` answer, under `TZ=America/Toronto`, `UTC`, `Asia/Kolkata`, `Europe/London`, `TZ` unset, and `TZ=Bogus/Zone`. Excerpts of that output are quoted below (verified).

### 1.1 `time` with `local-offset`

- **Already in the tree.** `time` is `0.3` with `formatting parsing macros` in `Cargo.toml:26`, and `cargo tree -e features -i time` shows the `local-offset` feature is already switched on, by `ratatui-widgets` (verified). `core/src/clock.rs` and `core/src/inbox/cmd.rs` do not use it.
- **How the zone is found.** `UtcOffset::local_offset_at` calls libc `localtime_r` and reads `tm_gmtoff` (`time-0.3.55/src/sys/local_offset_at/unix.rs:12-26,115-120`, verified). libc applies `TZ`, then `/etc/localtime`. The crate does nothing itself.
- **Abbreviation / IANA name.** Neither. The API returns a `UtcOffset` only (`-04:00:00`). No EDT, no `America/Toronto` (verified: the output above has only offsets).
- **DST.** Right on both sides: `05:30Z -> 01:30 offset -04:00`, `06:30Z -> 01:30 offset -05:00` (verified). On a target without `tm_gmtoff` it approximates from 1970 (`unix.rs:52-76`; not our targets).
- **Thread safety on Unix.** In 0.3.55 `local_offset_at` has no thread check (`unix.rs:115-120`, verified); the guard (`num_threads::is_single_threaded`) lives only in the separate `time::util::refresh_tz` (`sys/refresh_tz/unix.rs:44-55`, verified), which is not called by `local_offset_at` (`grep refresh_tz` in `src/`, verified). `localtime_r` is not safe against another thread calling `setenv("TZ")` on glibc (the tz-rs README cites RUSTSEC-2020-0071 and -0159; I did not read the advisories — unverified); macOS's libc has a thread-safe environment (`refresh_tz/unix.rs:5-17` lists it as such). cpd's engine is multi-threaded and never calls `setenv` (`grep '"TZ"'` over `core engine cli` finds none, verified), so the race needs a third party; guess: low risk, not zero.
- **Dependencies / build.** None new (`libc`, `num_threads` are already in `Cargo.lock`, verified).
- **Mock.** Pass a `UtcOffset` in. That is a fixed offset only: no DST rule, no abbreviation. A test of the fall-back hour cannot be written with it.

### 1.2 `jiff`

- **How the zone is found.** `TimeZone::system()` / `try_system()` (`jiff-0.2.35/src/tz/timezone.rs:325,391`). `get_force` checks the `TZ` environment variable first, then the platform (`src/tz/system/mod.rs:147-163`, verified). `TZ` may be an IANA name, a POSIX rule (`EST5EDT,M3.2.0,M11.1.0`) or a file path (`mod.rs:166-200`). An unset `TZ` falls to `/etc/localtime`: jiff reads the symlink and takes what follows the last `zoneinfo/` in the target (`src/tz/system/unix.rs:62-70`) — so the macOS target `/var/db/timezone/zoneinfo/America/Toronto` works (verified by the run: `TZ` unset gives `name=Some("America/Toronto")`). A non-symlink `/etc/localtime` is read as an unnamed TZif file (`unix.rs:28-40`). The result is cached for 5 minutes (`mod.rs:62-90`), so a zone change reaches a long-running engine within 5 minutes (verified in source; I did not wait to see it).
- **Abbreviation / IANA name.** Both: `Zoned::strftime("%Z")` gives `EDT`, `TimeZone::iana_name()` gives `Some("America/Toronto")`, and `%Q` the name or the `%z` offset (`src/fmt/strtime/mod.rs:184,202`). `name=None` when the zone has no name (hard-linked `/etc/localtime`, or `TZ` as a POSIX rule) (`timezone.rs:661-701`).
- **DST.** Right: `05:30Z -> 2026-11-01 01:30 EDT`, `06:30Z -> 01:30 EST` (verified). `Europe/London` in July gives `13:00 BST` (verified). Asia/Kolkata gives `IST`.
- **Thread safety.** Pure Rust: reads `TZ` with `std::env::var_os` and `/etc/localtime` with `std::fs`; no libc `tzset`, so no setenv race (source read; `std::env::var_os` is itself safe against Rust's own `set_var`). Cache behind an `RwLock` (`mod.rs:62-130`).
- **Dependencies / build.** One new crate: `jiff-core` (`cargo tree -p jiff`, verified). `rust-version = 1.70` (`Cargo.toml:14`). Default features `std tz-system tz-fat tzdb-bundle-platform tzdb-zoneinfo tzdb-concatenated perf-inline`; `tzdb-bundle-platform` bundles tzdata only on Windows/wasm (`Cargo.toml:76-90,118-124`), so on macOS/Linux it reads the system's `/usr/share/zoneinfo` and adds no data to the binary (I infer from the feature name; the `tzdb-bundle-platform` definition itself is unverified). Compile time: the three crates plus `iana-time-zone` built clean in 3.8 s in `dev` (verified, one run, not a benchmark).
- **Mock.** The best of the five, with no filesystem: `TimeZone::posix("EST5EDT,M3.2.0,M11.1.0")` is pure, gives abbreviations and the DST rule, and `TimeZone::fixed(Offset)` a fixed zone (`timezone.rs:457,506`). Verified: `posix("EST5EDT,M3.2.0,M11.1.0")` printed `2026-11-01 01:30 EDT` for `05:30Z` and `01:30 EST` for `06:30Z`; `posix("IST-5:30")` printed `11:00 IST`; `posix("UTC0")` printed `UTC`. It has `iana_name() == None`. `TimeZone::get("America/Toronto")` reads the system tzdb, so a test must not use it (A3).

### 1.3 `chrono` with `iana-time-zone` / `chrono-tz`

- **How the zone is found.** `chrono::Local` on Unix is its own TZif reader: `TZ` if set (hashed to detect change), else `/etc/localtime`, cached per thread (`chrono-0.4.45/src/offset/local/unix.rs:12-90`, verified). `iana-time-zone` is a separate crate that answers the *name* only: on Linux it reads the `/etc/localtime` symlink, then `/etc/timezone`, then OpenWrt config (`iana-time-zone-0.1.65/src/tz_linux.rs:1-58`); on macOS it asks CoreFoundation (`tz_darwin.rs:1-30`). **It ignores `TZ`** (it does not read the environment; the run with `TZ=UTC` printed `iana=Ok("GMT")` and with `TZ=Asia/Kolkata` `Ok("Asia/Kolkata")` only because the system zone was Toronto... see note) — precisely: with `TZ=UTC` and the system zone Toronto it answered `GMT`, not `America/Toronto`, so it did read something other than the files; I did not trace why on macOS (CoreFoundation honours `TZ`: I infer). Do not rely on it across platforms.
- **Abbreviation / IANA name.** Name: from `iana-time-zone`, which disagrees with `TZ` on Linux (source). Abbreviation: **none**. `DateTime<Local>`'s `%Z` printed the offset, `-04:00` (verified: `chrono ... %Z=-04:00`), because `Local` yields a `FixedOffset` (`src/format/formatting.rs:286`). A real abbreviation needs `chrono-tz`, which only knows a name you give it: `chrono-tz` was not in the registry and I did not read it (unverified), and it embeds its own tzdata (which then can differ from the system's).
- **DST.** Right: `01:30 / %Z=-04:00` then `01:30 / %Z=-05:00` (verified), offsets only.
- **Thread safety.** Pure Rust, per-thread cache (`unix.rs:28-30`); no libc call (verified in the source read).
- **Dependencies / build.** `iana-time-zone`, `core-foundation-sys` (macOS), `num-traits` (`cargo tree -p chrono`, verified). A second date library beside `time`.
- **Mock.** `FixedOffset` passed in: offset only. A named zone with DST needs `chrono-tz` with its own data.
- **A bad `TZ`.** With `TZ=Bogus/Zone` chrono fell back to `/etc/localtime` (`01:30 -04:00`), jiff gave UTC with `name=None`, the libc and `date` gave UTC (verified). Three answers for one bad value; the engine and the adapter must not both parse `TZ` differently (see §3).

### 1.4 `tz-rs`

Read from its README (`https://raw.githubusercontent.com/x-hgg-x/tz-rs/master/README.md`, fetched; the crate is not vendored and I did not read the source or run it):

- Finds the zone from a POSIX `TZ` string "which can be read from the environment": `std offset[dst...]` or a `file` / `:file` TZif path.
- Pure Rust reimplementation of `localtime`/`gmtime`/`mktime`; written *because* libc `localtime_r` races with `setenv`.
- No formatting ("This crate doesn't provide custom date time formatting"), no zone database ("tzdb files are not included"), and "the determination of the local time zone … is not supported on non-UNIX platforms".
- Whether it exposes the abbreviation (the TZif designation, `LocalTimeType::time_zone_designation`) and whether it finds `/etc/localtime` when `TZ` is unset: **unverified** (my recollection of its API, not read). It has no IANA name (a TZif file does not carry it; guess).
- It would mean formatting by hand: more code than jiff for the same result. Rejected on the criteria, not run.

### 1.5 The `date` command through the `Runner`

`core/src/inbox/cmd.rs:254-270` already calls `date +%z` through the mockable `Runner` and parses `+0200`.

- **How the zone is found.** libc, so `TZ`, then `/etc/localtime`. Verified: `TZ= date +%Z` -> `UTC`; `TZ=Bogus/Zone date '+%Z %z'` -> `UTC +0000`; `TZ=:America/Toronto date +%Z` -> `EDT`; `env -u TZ date '+%Z %z'` -> `EDT -0400`.
- **Abbreviation.** `date +%Z`, from the tzdata: `EDT`, `IST` (Kolkata), `BST` (London in July), `+04` (Dubai), `-03` (São Paulo) (verified, BSD date on macOS). **IANA name: no** (`date` does not print it). It would be a second step: `TZ` if set, else `readlink /etc/localtime` and strip up to `zoneinfo/` (verified that the link exists; the stripping is what jiff does).
- **A given instant.** `date` needs the instant as an argument, and the option differs: BSD `date -j -f %s <epoch> '+...'` (verified on macOS), GNU `date -d @<epoch>` (GNU on the homeserver — *unverified*, from memory). Two spellings, one `Runner` call per time shown, and a process spawned for every timestamp in a digest. `date +%z` today is asked once, for "now".
- **DST.** Right for the instant given (`TZ=America/Toronto date -j -f %s 1793511000` -> `01:30 EDT -0400`, verified).
- **Thread safety.** A child process; none to worry about.
- **Dependencies / build.** None.
- **Mock.** `Runner` returns a canned string, the best seam we have (`core/src/inbox/cmd.rs`). But a canned string tests the parser, not the formatting, and the mock must mimic what BSD and GNU `date` print.

### 1.6 Comparison

| | finds zone (TZ then /etc/localtime) | abbreviation (EDT) | IANA name | DST both sides | thread safety | new deps | mock |
|---|---|---|---|---|---|---|---|
| `time` local-offset | libc, yes | no | no | yes (verified) | libc `localtime_r` (no guard in 0.3.55) | none | `UtcOffset` only, no DST |
| `jiff` | own, yes (verified) | yes | yes (`None` if unnamed) | yes (verified) | pure Rust | `jiff-core` | `TimeZone::posix(..)`/`fixed(..)`, no file |
| `chrono` + `iana-time-zone` | chrono yes; `iana-time-zone` ignores `TZ` on Linux | no (`chrono-tz` for it; unverified) | `iana-time-zone` only | yes (offset) | pure Rust | 3 | `FixedOffset`, no DST |
| `tz-rs` | `TZ` string (README) | unverified | no (guess) | unverified | pure Rust | unverified | unverified |
| `date` via `Runner` | libc, yes (verified) | yes (tzdata's own) | no (separate `readlink`) | yes (verified) | child process | none | canned stdout |

## 2. TypeScript: `Intl.DateTimeFormat`

`pi/lib/time.ts` uses `Date.getHours()` etc. and prints no zone. I ran, with the process zone set by `TZ` (node v22.22.2, `en-CA`, `hourCycle: "h23"`, `timeZoneName: "short"`; `timeZone` from `Intl.DateTimeFormat().resolvedOptions().timeZone`), verified:

```
== TZ=America/Toronto
2026-10-25T03:03:00.000Z America/Toronto "2026-10-24, 23:03 EDT"
2026-07-01T12:00:00.000Z America/Toronto "2026-07-01, 08:00 EDT"
2026-01-15T12:00:00.000Z America/Toronto "2026-01-15, 07:00 EST"
== TZ=UTC
2026-10-25T03:03:00.000Z UTC "2026-10-25, 03:03 UTC"
== TZ=Asia/Kolkata
2026-10-25T03:03:00.000Z Asia/Calcutta "2026-10-25, 08:33 GMT+5:30"
== TZ=Europe/London
2026-10-25T03:03:00.000Z Europe/London "2026-10-25, 03:03 GMT"
2026-07-01T12:00:00.000Z Europe/London "2026-07-01, 13:00 GMT+1"
2026-01-15T12:00:00.000Z Europe/London "2026-01-15, 12:00 GMT"
```

(The full run printed all three instants for every zone; rows trimmed to the ones that differ.)

- **IANA name.** `resolvedOptions().timeZone` returns `Asia/Calcutta` for `TZ=Asia/Kolkata` (verified): Node returns the old canonical name. The Rust side (jiff) says `Asia/Kolkata` (verified). For the one place the IANA name shows (the cockpit, which is Rust) this does not matter; if the adapter ever shows the name it differs for such zones.
- **Zone with no abbreviation.** `GMT+5:30` for Kolkata, `GMT+1` for London in summer, `GMT+4` for Dubai (verified). `en-GB` also prints `GMT+5:30`, `en-IN` prints `IST` (verified for 1970-01-01): the abbreviation depends on the *locale*, so a fixed locale (`en-US`/`en-CA`) is the only stable choice. `timeZoneName: "longOffset"` gives `GMT+05:30` / `GMT-04:00` (verified).
- **DST fall-back.** `2026-11-01T05:30:00Z -> 01:30 EDT`, `06:30:00Z -> 01:30 EST` (verified). `Date#getHours()` is `1` for both, so the current `localTime` prints `01:30` twice with nothing to tell them apart.
- **Spring forward.** `06:59Z -> 01:59 EST`, `07:00Z -> 03:00 EDT` (verified).
- **Mock.** Pass `timeZone` to `Intl.DateTimeFormat` explicitly: it uses Node's own ICU tzdata, not the system zone (I infer from the option's contract; the output above with `TZ=Asia/Kolkata` and `timeZone` omitted is the system-zone path). Existing pi tests set `process.env.TZ = "Asia/Kolkata"` (`pi/test/folds.test.ts:9`, verified): process-global, so an explicit parameter is cleaner.
- **Missing abbreviation, what cpd should print.** Print the numeric offset: `2026-10-25 08:33 +05:30`, never `GMT+5:30` (it reads as a zone named GMT shifted) and never a guessed word. Rule on both sides: if the abbreviation is letters only, print it; otherwise print `±HH:MM`.

## 3. Which zone the cpd processes see

- **Verified on this machine.** The engine (`cpd-work:watch`, pid 99837, command `cpd`) and a pi Worker (`cpd-work:...research-zone-a1`, pid 10000, `node`) have **no `TZ` in their environment** (`ps -ww -E -p <pid>`, filtered for `^TZ=`: 0 entries each); the tmux server (pid 5683) has none, `tmux show-environment -g` has no `TZ`, and `launchctl getenv TZ` is empty. My login shell has no `TZ` either (`env | grep '^TZ='` -> 0). So every process falls to `/etc/localtime` -> `America/Toronto`: the Rust engine and the Node adapter name the same zone here (verified for this host and for `TZ` unset; the jiff run with `TZ` unset gave `America/Toronto`/`EDT`, node `resolvedOptions().timeZone` with `TZ=America/Toronto` gave the same; I did not run node with `TZ` unset in a pane).
- **How cpd starts them.** The engine is respawned in `cpd-work:watch` by the tmux hooks (`docs/contract.md` §6.6 "Supervision"), Workers by `new-window` (`core/src/host/tmux.rs:862`), drivers by `core/src/driver/start.rs:370-385` with `env_set` `CPD_HOME` and `env_remove` of `CPD_*`/`TMUX*` only. Nothing sets or removes `TZ` (`grep` for `"TZ"` in `core engine cli`: none, verified), so a child takes its parent's.
- **The risk (verified with a scratch tmux server, `-L`, killed afterwards).** A pane's environment is the **tmux server's**, frozen when the server started: `TZ=Asia/Kolkata tmux -L x new-session ...` then `TZ=Europe/London tmux -L x new-window ...` gave both panes `TZ=Asia/Kolkata`. `TZ` is not in tmux's `update-environment` list (`tmux show-options -g update-environment`, verified), so a client's later `TZ` does not reach panes. Consequence: if the User sets `TZ` in a shell (or the server was started by launchd/systemd/cron with `TZ` unset or different), the CLI commands the User runs (Rust, from the shell) can name a different zone than the engine and the adapter, which run under the server's. On this host the server has no `TZ`, so they agree today. The Linux homeserver was not checked (*unverified*; systemd user sessions usually leave `TZ` unset, guess).
- **Same string, different readers.** With `TZ` unset both read `/etc/localtime`; with a valid IANA `TZ` both honour it (verified for jiff, `date`, node). With an invalid `TZ` they diverge (§1.3: jiff UTC, chrono system, `date` UTC); node with a bad `TZ`: not run (unverified).

## 4. DST ambiguity and the format

- At the fall-back change, the local time `01:30` occurs twice. An *instant* is never ambiguous (verified above: `05:30Z` is `01:30 EDT`, `06:30Z` is `01:30 EST`), so converting a stored UTC time is exact on every library listed (`time`, `jiff`, `chrono`, `Intl`). Ambiguity appears only when a *local wall time* is parsed back; cpd does not (fields stay UTC, A2), and `jiff` would expose it as `AmbiguousZoned` if it did (`timezone.rs:1058`).
- The zone abbreviation in the prose is what tells the two `01:30` apart (`EDT` vs `EST`), which is the reason to name it. The bare `HH:MM` rows of the pi chat (A2) are ambiguous only in that one hour per year; accepted by the decision.
- Missing abbreviation: `±HH:MM` (§2). jiff for such a zone (Dubai) — *guess:* `+04`, like `date` (`+04`, verified), which already passes the "letters only" test as *not* letters, so it would be replaced by `+04:00`. Not run.

## 5. Recommendation

**Dependency: `jiff` (pinned `=0.2.x`, default features) on the Rust side; none on the TypeScript side.** It is the only candidate that gives the abbreviation, the IANA name and exact DST from `TZ` then `/etc/localtime`, in pure Rust, with one extra crate (`jiff-core`) and a pure test seam (`TimeZone::posix`). `time` stays for stored UTC timestamps (`clock.rs` is unchanged): jiff is used only to *show* a time. `time`'s offset has no name; `chrono` has none either and adds three crates and a second date library; `date` needs one process and two spellings per time shown and cannot give the IANA name; `tz-rs` would need hand formatting (unverified beyond its README). Adding a crate is a one-way door of small size, and `time::UtcOffset` plus `date` can be taken back out of any call site.

**One shared format.** `YYYY-MM-DD HH:MM ZZZ`: a 24-hour clock, zero-padded, no seconds, one space before the zone, `ZZZ` the abbreviation if it is letters only (`EDT`, `UTC`, `IST`, `GMT`), else the numeric offset `±HH:MM`. Rust spelling: jiff `%Y-%m-%d %H:%M %Z` (with the numeric fallback applied by the function); TypeScript: assembled from `Intl.DateTimeFormat.formatToParts`.

**Rust** (in `core/src/clock.rs` beside `timestamp`; the seam is the `Clock` trait, as for `utc()`):

```rust
use jiff::tz::TimeZone;

/// The zone prose is written in: the system's for SystemClock, a fixed one for ManualClock.
pub trait Clock { /* ... */ fn zone(&self) -> TimeZone; }
// SystemClock: TimeZone::system()
// ManualClock: TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").expect("a valid rule")

/// `2026-10-24 23:03 EDT`: `at` in `zone`, as prose.
pub fn prose(at: OffsetDateTime, zone: &TimeZone) -> String;

/// The IANA name once, for the cockpit: `Some("America/Toronto")`, `None` when the zone has none.
pub fn zone_name(zone: &TimeZone) -> Option<&str>;
```

The tests pass a `ManualClock` or a `TimeZone::posix(..)` value: nothing reads `TZ` or `/etc/localtime`, no tzdb file, no process (A3). Production passes `ports.clock.zone()`.

**TypeScript** (in `pi/lib/time.ts`, replacing the `Date` local methods):

```ts
/** `2026-10-24 23:03 EDT`. `timeZone` undefined = the process's zone; a test passes an IANA name. */
export function proseTime(at: string, timeZone?: string): string;
```

It builds `new Intl.DateTimeFormat("en-CA", { timeZone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hourCycle: "h23", timeZoneName: "short" })`, joins the parts into the shared format, and when the `timeZoneName` part is not letters only (`GMT+5:30`), takes `timeZoneName: "longOffset"` and prints `+05:30` (verified outputs in §2). The `en-CA` locale is fixed on purpose (the abbreviation changes with the locale). The test seam is the `timeZone` argument; no test sets `process.env.TZ`. The existing `localTime` (`HH:MM`) is untouched.

**The zone the two sides name.** Same when `TZ` is unset or valid (verified here); the engine and adapter inherit the tmux server's `TZ`, which can differ from the shell's (§3), so cpd names the zone from the process that prints. Not fixed here (no surface or enforcement in this Job).

**Rendered examples** for the instant `2026-10-25T03:03:00.000Z` (Rust column: jiff, from the run for the other instants and `date`/node for these; the Node column is verified from the `node` runs above; the Rust column for this exact instant I did not run: *I infer* it from the two verified jiff outputs for 2026-11-01 and 2026-01-15 in the same zones):

| zone | Rust (jiff) | TypeScript |
|---|---|---|
| America/Toronto | `2026-10-24 23:03 EDT` | `2026-10-24 23:03 EDT` |
| UTC | `2026-10-25 03:03 UTC` | `2026-10-25 03:03 UTC` |
| Asia/Kolkata | `2026-10-25 08:33 IST` | `2026-10-25 08:33 +05:30` |

The one divergence the shared format cannot remove: where Intl prints `GMT+5:30` and the tzdata has a word (`IST`, `BST`), the Rust side prints the word and the TypeScript side the offset. Both are right and unambiguous; they agree for the User's zone (`EDT`/`EST`) and for UTC.
