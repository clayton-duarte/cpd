# The pill spec: one data file, one atom, three adapters

Research for Plan `ui-widgets`, Job `pill-spec`, Attempt `pill-spec-a1`. Research only: no code, test or other doc changed.

**Code read:** `origin/main` at `c2953181` (`Invert pill and selection text, never colour 0 (hs-d594) (#291)`), read through a throwaway `git worktree` (removed afterwards). Plan `dango-v3`'s PR `pills` is merged, so `origin/main` holds the inverted pills. `git log origin/main --oneline` shows it first. Every `file:line` below is that commit. Anything not read or run is labelled `unverified`.

## 0. Findings that change the brief

1. **The tmux strings are not rebuilt every refresh.** cpd sets `pane-border-format` and `status-format[0]` once: `apply_server_options` (`core/src/host/tmux.rs:637`, called at `tui/src/cockpit.rs:134` and `engine/src/watch/ensure.rs:225`), `apply_status_line` (`tmux.rs:647`, `cockpit.rs:137`), and `new_session_args` on a fresh server (`tmux.rs:992`). tmux expands the format on every refresh; cpd does not. So a runtime-built tmux string costs one build per `cpd tui` start, never more than today per refresh (item (e) holds trivially for tmux). Only ratatui draws every frame.
2. **`pub const` is the real constraint on tmux.rs**, not `concat!` alone. `BORDER_FORMAT` (`tmux.rs:103`) and `STATUS_FORMAT` (`tmux.rs:301`) feed `const SERVER_OPTIONS: &[&[&str]]` (`tmux.rs:152`, line 163) and `const STATUS_OPTIONS: [(&str,&str);3]` (`tmux.rs:319`). A runtime spec turns those consts into `LazyLock` statics. Section 3 lists what that touches.
3. **A latent drift bug already exists in pi.** `pi/lib/hint.ts:13` builds the SGR as `` `\x1b[3${n}m` ``; for an index 8-15 that is `ESC[312m`, not `ESC[92m`. `pi/compact-tools/extensions/compact-tools-indices.ts:22-24` (`foreground()`) does it right (30+n, 90+n-8). Today `hint.ts` only passes 2, 4, 7, so nothing shows. Checked: `node -e 'const n=12;console.log(JSON.stringify(\`\\x1b[3${n}m\`))'` printed `"\u001b[312m"`.
4. **The pill's width arithmetic is hand-counted in three more places**, all assuming caps of 1 column each and padding 0: tmux `long_fits!`/`fill_start!`/`fill_count!` (`tmux.rs:214-234`: `14`, `12`, `3`), `tui/src/dialog/view.rs:31` (`CAPS: u16 = 2`, used at 316), `tui/src/plan/flow.rs:341,343` (`pill_w(i) + 2`), and pi `compact-tools-editor.ts:84` (`inner - 1 - 2 - 2`). So text padding and cap width are **not** free data fields (section 2).
5. **Real tmux in tests.** `core/src/host/tmux/palette.rs:105-155` (`Server`) starts a private tmux server in three tests (`:159`, `:186`, `:215`). The repo policy `tests` says no test uses the real tmux. This Plan's red tests avoid it (section 5); what to do with the three existing ones is open question Q3.

## 1. Inventory of every pill site

Found with `rg -n 'E0B6|e0b6|E0B4|e0b4|uE0B' tui core pi cli engine --glob '!**/*test*'` on the worktree, then by reading every `Style` that carries `REVERSED` (`rg -n 'REVERSED|reverse\]|\[7m'`) and every tmux pill macro. The list in the Spec was incomplete in one place: `tui/src/tree/look.rs` and `tui/src/widgets/column.rs` hold caps only inside `#[cfg(test)]` code (`tree/mod.rs:34-38`, `column.rs:355`), so they are test copies, not sites. `core/src/host/tmux/palette.rs` and `core/src/host/tmux/no_inbox.rs` are tests. `engine/src/watch/pill_texts.rs` builds the text only (no caps). `tui/src/palette.rs:149-151` `block()` is the one place `Modifier::REVERSED` is made (the test `no_dim_or_reversed_outside_the_palette`, `tui/src/palette/colours.rs:114`, enforces it).

Every site draws **0 spaces** between a cap and the text (the text begins right after the cap). "Reverse" is how the block is made.

| # | Site: file, function, line | Caps (where defined) | Cap colour | Variant (colour) | Reverse video | Differs from the others by |
|---|---|---|---|---|---|---|
| 1 | `tui/src/section.rs` `row()` 57-87; callers `title` 37, `collapsible_title` 45 | `CAP_LEFT`/`CAP_RIGHT` 26/28; pushed at 72, 74 (title) and 78, 80 (count pill) | `Style::new().fg(pill_style.fg.expect(..))` (58): fg only, no bg, not reversed | static 7 (`palette::pill_static`, 38, 46) | `palette::block` via the style passed in | Owns the whole title row: lead, rule fill, cut-to-width, optionally a second pill (the count). Pills are pushed as three `Span`s into a bigger `Vec` |
| 2 | `tui/src/section.rs` `box_top` 51-53 (same `row()`) | same | same | label: `palette::label_pill(focused)` (258): 15 unfocused, 12 focused (`input_border`, 201) | same | The cap and block colour follow the box border's focus |
| 3 | `tui/src/dialog/view.rs` `frame_top` 170-190 | its own `CAP_LEFT`/`CAP_RIGHT` 28-29; used at 183 | `Style::new().fg(style.fg.expect(..))` (180) | static 7 (179) | same | A second copy of row 1's cut rule (`width - 6`, `left >= text + 3`, 176-177) |
| 4 | `tui/src/dialog/view.rs` `pill()` 205-209 (buttons, call 403) | same consts; used at 207 | same pattern (206) | interactive 15, the first button `pill_selected` 12 (402) | same | Draws into a `Rect` through `Paragraph`; width is `label + CAPS` (31, 316) |
| 5 | `tui/src/widgets/legend.rs` `capped` 36-39 (call 132) | `CAP_L`/`CAP_R` 31-32; its doc says it copies `plan/flow.rs` | same pattern (37) | `palette::job_pill` (217): status colour, bright pair (clickable) | same | The doc comment is the drift admitted: "as the Plan view draws them (`plan/flow.rs`, which is private to its module)" (29-30) while `flow.rs:141` is `pub` |
| 6 | `tui/src/plan/flow.rs` `layout` 444-451 | `pub const CAP_L`/`CAP_R` 141-142; written with `set_symbol` at 447-448 | `Style::new().fg(style.fg..)` (445) | `Live::pill` 60-68 = `job_pill` | same | Writes caps straight into `Buffer` cells; the body is two `Line`s (left icon+key, right tier), left/right aligned in a rect stretched to the column width (343, `body_of` 461); the gap between them is filled by `Line`'s own style (`ratatui-core-0.1.2/src/text/line.rs:730` `buf.set_style(area, self.style)`, read) |
| 7 | `tui/src/plan/flow.rs` selected pill 575-587 | none (re-styles the cells of 6) | `Style::new().fg(palette::pill_focus().fg..)` (580) | focus: `pill_focus` (263), the blue block, 4, body also BOLD (582) | same | Restyles an already drawn buffer; the 5th copy of the `.fg(style.fg.expect(..))` pattern |
| 8 | `core/src/host/tmux.rs` macro `title_pill!` 81-88; uses 104, 106 | literals `\u{e0b6}` 84, `\u{e0b4}` 86 | `#{?pane_active,#[fg=blue],#[fg=colour7]}` (84): a tmux conditional, so the colour is not one index | focus 4 (`blue`) / static 7 (`colour7`), by `pane_active` | `#[reverse]` 84 ... `#[noreverse]` 86 | A `━` lead in the pill's colour before the left cap (84); no `#[default]` after; the colour is a format expression; text is a format expression too (`#{@cpd_title}`, a conditional chain) |
| 9 | `core/src/host/tmux.rs` macro `button!` 255-259; uses 303, 305, 307 | literals in the `concat!` at 257 | `#[fg=colour15]` | interactive 15 | `#[reverse]` ... `#[noreverse]` (257) | Wrapped in `#[range=user|<name>]` ... `#[norange]`; ends `#[default]` |
| 10 | `core/src/host/tmux.rs` macro `pill!` 261-265; uses 308 (`colour3`), 311, 312 (`colour8`) | literals at 263 | `#[fg=<colour literal>]` | preview 3, workers 8, version 8 (all read-only meaning colours) | `#[reverse]` ... `#[noreverse]` (263) | The colour is a macro argument (`$colour:literal`), the one tmux site that names an arbitrary colour; ends `#[default]` |
| 11 | `pi/lib/render.ts` `rowsOf` 166 (consts 136-137) | `CAP_LEFT`/`CAP_RIGHT` 136-137 | `\x1b[35m` hard-coded 5 | magenta 5 (the `cpd` brand pill, planning) | `\x1b[7m` ... `\x1b[27m` | The only pi site with the colour as a literal escape, not an index; the width is measured (`visibleWidth(pill)`, 172), not counted |
| 12 | `pi/lib/hint.ts` `pillN` 13, `titlePill` 14, `tabLine` 35-36, header 68 | literals `\uE0B6`/`\uE0B4` at 13 | `` `\x1b[3${n}m` `` | 7 (header, unanswered tab), 4 (current), 2 (answered) | `\x1b[7m` ... `\x1b[27m` | Valid only for index < 8 (finding 3); a second definition of what `indices.ts` `pill` is |
| 13 | `pi/compact-tools/extensions/compact-tools-indices.ts` `pill` 75-79; `statusPill` 82-84; `thinkingPill` 87-89 | literals `\uE0B6`/`\uE0B4` at 78 | `foreground(index)` (22-24), correct for 0-15 | running/not started 6, done 2, failed 1, asking 5 (`statusIndex` 72-73); thinking 7 | `\x1b[7m` ... `\x1b[27m` | The best pi copy: takes an index. Callers: `compact-tools.ts:310`, `compact-tools-thinking.ts:251`, `compact-tools-editor.ts:87` (6, text `${loaderFrame()} ${label}`), `compact-tools-progress.ts:132` (6) |

Copies of the caps in **test** code (they pin the contract and are allowed to; see Q2): `tui/src/tree/look.rs:28-29`, `tui/src/widgets/column.rs:797`, `tui/src/section.rs:95` (imports the non-test consts), `tui/src/pill_colours.rs:14` (imports `flow::CAP_L/CAP_R`), `cli/tests/tui/tui_titles.rs:6-7`, `core/src/host/tmux/palette.rs:89`, `core/src/host/tmux/no_inbox.rs:10,14`, and the many spelled rows in `cli/tests/tui/*.rs`, `cli/tests/session/pill_lines.rs`, `pi/test/*.test.ts`.

Not a pill, same shape: the selection and cursor blocks (`palette::selected`, `tui/src/menu.rs:160`, `tui/src/input_box.rs:60`, `tui/src/logs/view.rs:193`, `tui/src/tree/rows.rs:549`) use the same `block()` style with no caps. They stay as styles; the atom is for blocks with caps.

## 2. The spec

**What is data and what stays code** (the rule applied: a field is data only when Rust and pi can both honour any value of it).

| Field | Value | Data or code | Where it lives today |
|---|---|---|---|
| `caps.left` | U+E0B6 | **data** | 7 non-test literals: `section.rs:26`, `dialog/view.rs:28`, `flow.rs:141`, `legend.rs:31`, `tmux.rs:84,257,263`, `render.ts:136`, `hint.ts:13`, `indices.ts:78` |
| `caps.right` | U+E0B4 | **data** | same sites, `:29`, `:29`, `:142`, `:32`, `tmux.rs:86,257,263`, `render.ts:137`, `hint.ts:13`, `indices.ts:78` |
| `meanings.plain` | 7 (grey: the default when no meaning applies; Rust `NotStarted`, `Grounding`) | **data** | `palette.rs:82-92` (`Color::Gray`); tmux `colour7` (84); pi `INDEX.grey` (`indices.ts:9`) |
| `meanings.in_progress` | 6 (cyan; Rust `InProgress`, `Waiting`) | **data** | `palette.rs:82`; pi `RUNNING` (`indices.ts:17`) |
| `meanings.review` | 3 (yellow) | **data** | `palette.rs:82`; tmux `colour3` (`tmux.rs:308`) |
| `meanings.done` | 2 (green) | **data** | `palette.rs:82`; pi `statusIndex` 72 |
| `meanings.failed` | 1 (red) | **data** | `palette.rs:82`; pi `statusIndex` 72 |
| `meanings.planning` | 5 (magenta) | **data** | `palette.rs:82`; pi `render.ts:166` (`35`), `statusIndex` 73 |
| `meanings.nothing` | 8 (dark grey; no clickable pair) | **data** | `palette.rs:82`; tmux `colour8` (`tmux.rs:311-312`) |
| `meanings.focus` | 4 (blue: focus, selection, a cpd notice; Rust `Notice` and `FOCUS` 141) | **data** | `palette.rs:141`, `palette.rs:82`; tmux `blue` (`tmux.rs:84`); `hint.ts:35` |
| `clickable` | `{"add": 8, "from": 1, "to": 7}`: a base `n` in 1-7 is `n + 8` on what can be clicked; outside 1-7 (8) it stays | **data** | `palette.rs:159-171` `bright` and 172-178 `color_clickable`; tmux `button!` hard-codes 15 (257); pi has none (no clickable pill) |
| roles (not fields): `static` = `plain`; `interactive` = `plain` clickable (15); `focus` = `focus` base (4); `selected` button and focused label = `focus` clickable (12); workers and version pills = `nothing` (8); preview = `review` (3); `cpd` brand = `planning` (5); tool pill = `in_progress`/`done`/`failed`, asking = `planning` | derived | **code, in each adapter's caller** | `palette.rs:232-266` (`pill`, `pill_clickable`, `pill_static`, `pill_interactive`, `pill_selected`, `label_pill`, `pill_focus`); tmux 84, 257, 308-312; pi `indices.ts:72-89`, `hint.ts:14,35-36`, `render.ts:166` |
| block drawn with reverse video (hs-n447): text reversed in the colour, terminal background as fill | fixed | **code constant of each adapter**, pinned by tests | `palette.rs:149-151`; `tmux.rs:84,86,257,263`; `indices.ts:78`, `hint.ts:13`, `render.ts:166` |
| caps not reversed, the block's colour as foreground, no background | fixed | **code constant**, pinned by tests | `section.rs:58`, `dialog/view.rs:180,206`, `legend.rs:37`, `flow.rs:445,580`; tmux and pi per above |
| text padding | 0 | **code constant, not data** | implicit (no site pads); hand-counted widths in finding 4 assume 0 |
| index 0 never drawn as text or on a block | invariant | **a test**, not a field | `tui/src/pill_colours.rs:137`, `cli/tests/session/pill_lines.rs:99`, `palette/colours.rs:114` |

Why padding and reverse are not data: the three adapters and the width arithmetic of finding 4 are written for padding 0 and reverse video only. A `"padding": 1` or `"reverse": false` field would be representable in the file and honoured by no one. If the User later changes either, it is a code change in the three adapters plus those four hand counts, which a test (section 5) fails on. This is "make illegal states unrepresentable".

**The contract's meaning table to spec keys** (`docs/contract.md` 6.1.1 "What each colour means", read): red (1/9) `failed`, green (2/10) `done`, yellow (3/11) `review`, blue (4/12) `focus`, magenta (5/13) `planning`, cyan (6/14) `in_progress`, grey (7/15) `plain`, plus rule (2)'s 8 `nothing`. Index 0 has no key. The Rust `Meaning` enum (`palette.rs:27`) stays: it maps domain facts (a Job status, a phase) to a spec key (`job_status` 331, `attempt_status`, `phase`, `tier_span`), which is code. What moves out of Rust is only the numbers inside `color()` (82-92).

**The exact file** (`pi/lib/pill-spec.json`, recommended in section 3):

```json
{
  "caps": { "left": "\ue0b6", "right": "\ue0b4" },
  "clickable": { "add": 8, "from": 1, "to": 7 },
  "meanings": {
    "plain": 7,
    "in_progress": 6,
    "review": 3,
    "done": 2,
    "failed": 1,
    "planning": 5,
    "nothing": 8,
    "focus": 4
  }
}
```

Both readers reject an unknown or missing key (Rust: `#[serde(deny_unknown_fields)]` on a struct with one `u8` field per meaning; pi: an explicit key check at load) and any index above 15 or equal to 0.

## 3. The single-source mechanism

Constraints checked in code, in the order of the Spec.

**(a) tmux.rs.** `concat!` takes literals only. Experiment (a scratch crate outside the worktree, deleted): `include!(concat!(env!("OUT_DIR"), "/g.rs"))` of a build-script-written `macro_rules! cap_l { () => { "\u{e0b6}" }; }` then `concat!(cap_l!(), "#[reverse]", $t)` in a `const` compiled and printed `"\u{e0b6}#[reverse]x"`; and `concat!(include_str!("../cap.txt"), "y")` printed `"Xy"`. So compile-time literals can come from a file, but only as whole-file text or via generated macros; picking one field out of a JSON file needs a parser in `build.rs`. Call sites of the consts (all read): `SERVER_OPTIONS` in `tmux.rs:638,995,1255`; `cli/tests/session/host.rs:122-136,531`, `session.rs:158`, `tui.rs:132,278`, `pill_lines.rs:3-71`, `tooling/harness.rs:580`; `core/src/host/tmux/palette.rs:152,169,178,217`, `junctions.rs:60`, `no_inbox.rs:8-14`. None needs a `const`; they need a `&str` or an iterator.

**(b) How pi is installed.** `scripts/install-cpd.sh:130` is `cp -R "$root/pi/." "$adapter_tmp/"`, then `rm -rf "$adapter_tmp/test"` (131). Nothing outside `pi/` is installed (`pi/package.json` lists `./extension.ts`, `./compact-tools/...`, `./ask-user-question/index.ts`, `./skills`, `./themes`; no path leaves `pi/`). So a pi module **cannot** import a file outside `pi/` at its installed place (`~/.local/share/cpd/pi`), and **can** read one under `pi/` with no change to the script or `package.json`. `scripts/check-portable.sh` checks scripts only (its `covered` list, line 18), so a JSON file passes it; `scripts/pi-check.sh` loads `pi/` as the adapter and would catch a failed load (`pi-check.sh:33`, `adapter_dir="$root/pi"`). Rust can read a file under `pi/` at compile time: `tui/src/logs/mod.rs:138-139` already does `include_str!("../../../docs/contract.md")` (a path outside the crate directory), and `core/src/config/catalog.mjs` is `include_str!`'d at `config/catalog.rs:24`. Both ways of reading JSON in pi were run with the pi-installed `jiti` (`.pi/agent/install/releases/1.3.0/node_modules/.bin/jiti`) and with node 22.23 (the repo's `engines` is `>=22.18`): `JSON.parse(readFileSync(new URL("./spec.json", import.meta.url), "utf8"))` and `import spec from "./spec.json" with { type: "json" }` both printed the cap character. `import.meta.url` in installed pi code has a precedent (`pi/lib/reload.ts:28`). The repo's `tsc` accepts neither form unverified (`tsconfig.json` has `module: nodenext`, no `resolveJsonModule`; not run); the recommendation uses `readFileSync` + an explicit check, so typecheck does not matter.

**(c) Crate layout.** `cpd-tui` depends on `cpd-core` (`tui/Cargo.toml:13`); `tmux.rs` is in `cpd-core`; `cpd-core` has `serde` and `serde_json` already (`core/Cargo.toml` `[dependencies]`), and does not depend on ratatui (it is a dev-dependency only). `docs/implementing.md:145`: "Later Jobs may not add, remove or change dependencies or features." So the reader goes in `cpd-core` and returns plain data (`u8`, `&'static str`); `cpd-tui` maps an index to `ratatui::style::Color`. A `build.rs` that parses JSON would need `serde_json` as a **build-dependency**, a `core/Cargo.toml` change that rule forbids without the Lead (the existing `core/build.rs` parses by hand). Gate checks a new data file must pass: none of its own (`check-portable.sh` covers scripts; `pi-check.sh` loads the adapter; `npm test` runs `pi/test/*.test.ts`).

**(d) Rust `Meaning` and `palette::color_clickable`:** section 2 (the enum and its mappings stay; the digits and the `n+8` rule move).

**(e) Cost.** Rust parses once: `static SPEC: LazyLock<Spec>` in `cpd-core`; the atom borrows `&'static str` caps from it (`Span::styled(&spec().caps.left, ..)`, no per-call allocation; the per-frame `Vec<Span>` is the allocation each caller already makes). tmux: the strings are built once into `LazyLock<String>`s and set once (finding 1). pi: parsed at module load into a `const`. No benchmark was written or run; the claim is structural (unverified by measurement).

### The options

| | Option 1: one data file read by Rust and pi | Option 2: Rust source plus a generated TS file, kept in step by a test | Option 3: two definitions plus a shared-fixture conformance test |
|---|---|---|---|
| Source of truth | `pi/lib/pill-spec.json` | a Rust file (`core/src/pill.rs` consts) | Rust consts and `pi/lib/pill.ts` consts |
| Rust reads | `include_str!("../../pi/lib/pill-spec.json")` + `serde_json` once (`LazyLock`) | consts | consts |
| pi reads | `readFileSync(new URL("./pill-spec.json", import.meta.url))` (run, above) | `pi/lib/pill-spec.generated.ts` | `pi/lib/pill.ts` |
| What keeps them equal | nothing needed: there is one file | a Rust test renders the TS and compares it to the committed file (the Gate runs it; `cargo` has no TS step; a regenerate step is a human or a script call) | a fixture `pi/test/fixtures/pill-cases.json` that both test suites read |
| A copy is a second source? | no | yes (the committed `.ts` is a copy) | yes (two definitions) |
| tmux.rs change | consts become `LazyLock` statics built with `format!` (finding 2) | none for the caps (the macros keep their literals), caps still 3 places in tmux.rs | none |
| New dependency | none | none | none |
| New moving parts | one JSON file, one Rust module | a generator and its test | a fixture and two readers |
| Fails when | a reader's check rejects a malformed file, at build or `npm test` | the generated file is stale | the two tables diverge and a fixture row exercises it; **a meaning added on one side and not in the fixture passes** |
| Verified | pi read (jiti, node), Rust precedent `include_str!` outside the crate, install copies `pi/.` (`install-cpd.sh:130`) | the test would need `cpd-core` to emit TS (unverified how the Gate regenerates) | n/a |

Option 4 (rejected): the data file at the repository root, `spec/pill.json`, copied into the adapter by `install-cpd.sh`. The installed adapter would have the file, but `scripts/pi-check.sh:33` and `npm test` run `pi/` in place, where the root file is not beside the code. A symlink `pi/lib/pill-spec.json -> ../../spec/pill.json` fixes the checkout, but `cp -R` copies the link, which dangles under `~/.local/share/cpd/pi` (`cp -RL` would fix it, and is a change to a Gate-checked script). Two more moving parts for no gain.

### Recommendation: Option 1

**The file:** `pi/lib/pill-spec.json`, exactly the JSON of section 2. The one location both can read with no copy: it is inside `pi/` (installed by the existing `cp -R`) and Rust reaches it with `include_str!` the way `tui/src/logs/mod.rs:138` reaches `docs/`.

**cpd-core loader** (`core/src/pill.rs`, new, `pub mod pill;` in `core/src/lib.rs`):

```rust
const SPEC: &str = include_str!("../../pi/lib/pill-spec.json");

pub fn spec() -> &'static Spec {
    static CELL: LazyLock<Spec> = LazyLock::new(|| Spec::parse(SPEC).expect("pi/lib/pill-spec.json is a valid pill spec"));
    &CELL
}
```

`Spec::parse` is also the function the test calls on a bad file, so the `expect` can never fire in a build that passed its tests (the file is compiled in). `cpd-tui` and `tmux.rs` call `cpd_core::pill::spec()`.

**tmux.rs:** `title_pill!`, `button!`, `pill!` (81-88, 255-265) go; `BORDER_FORMAT`, `STATUS_FORMAT` become `pub static ...: LazyLock<String>` built from the adapter in section 4 around the `concat!` pieces that are not pills (`fill!`, `long_fits!`, `join!`, `reset_glyph!` stay macros: they hold no pill). `SERVER_OPTIONS` becomes `pub static SERVER_OPTIONS: LazyLock<Vec<Vec<String>>>`, `STATUS_OPTIONS` a `LazyLock<[(&'static str, String); 3]>`. `SERVER_OPTIONS.iter()`, `o[1] != "-as"` and `o[3]` keep compiling (a `String` against a `&str`); `*o == ["set", ...]` (`host.rs:122,130`) and `o.get(2) == Some(&name)` (`palette.rs:217`) need small edits (unverified until compiled). The three uses inside `tmux.rs` (`638`, `995`, `1255`) iterate and call `strings(option)`, which takes `&[&str]`: it takes `&[String]` after the change.

**pi** (`pi/lib/pill.ts`, new):

```ts
const raw: unknown = JSON.parse(readFileSync(new URL("./pill-spec.json", import.meta.url), "utf8"));
export const spec: PillSpec = parseSpec(raw);   // throws on an unknown/missing key or an index outside 1-15
```

`readFileSync`, not `import ... with { type: "json" }`: both ran, but the explicit parse is the validation at the boundary and does not depend on `tsc`'s JSON settings.

## 4. The API

### ratatui: the atom (hs-d596: a function that returns `Vec<Span>`, no state, not a `Widget`)

`tui/src/pill.rs` (new, `mod pill;` in `tui/src/lib.rs`). `ratatui-core-0.1.2/src/text/span.rs:418` and `line.rs:699` show `Span` and `Line` are `Widget`s already, so a pill inside a `Line` needs no new widget:

```rust
/// `[left cap, text, right cap]`: the caps in the block's colour (foreground only, not reversed), the text in `block`.
pub fn pill<'a>(text: impl Into<Cow<'a, str>>, block: Style) -> Vec<Span<'a>>;
/// The style of a cap: the block's foreground, nothing else. For code that restyles drawn cells (the selected Job pill).
pub fn cap_style(block: Style) -> Style;
/// Cells of both caps: what the width arithmetic adds to the text (2).
pub const CAPS_COLUMNS: u16 = 2;
```

`block` is always a `palette` style (`pill_static`, `pill_interactive`, `pill_focus`, `pill_selected`, `label_pill`, `job_pill`, `pill`, `pill_clickable`), so the colour logic stays in `palette.rs`, which only changes `color()` to read `spec().meanings` and `color_clickable()` to apply `spec().clickable`. The index to `Color` map (`0..=15` to `Color::Red`... `Color::White`) is one function in `palette.rs`, the only file that may name a colour (`no_colour_outside_the_palette`).

Call sites replaced (all `file:line` of `c2953181`):

| Replaced | By |
|---|---|
| `section.rs:26-28` consts and `:58,72-74,78-80` | `spans.extend(pill(text, pill_style))` (twice) |
| `dialog/view.rs:28-29`, `frame_top` `179-183` | `spans.extend(pill(text, palette::pill_static()))` |
| `dialog/view.rs:205-209` `pill()` and `:31` `CAPS`, `:316` | `Paragraph::new(Line::from(pill(label, style)))`; `pill::CAPS_COLUMNS` |
| `widgets/legend.rs:29-39` `capped`, `:132` | `Line::from(pill(format!("{icon} key│S1"), palette::job_pill(..)))` |
| `plan/flow.rs:141-142` consts (and `pill_colours.rs:14` import), `:444-451` | the body text with its gap (`left + " ".repeat(gap) + right`, the gap known from `rect_w` 343) is one `pill(..)`; `Line::from(..).render(rect, &mut buf)`. The gap cells are blank and reversed, as `Line::style` fills them today |
| `plan/flow.rs:341,343` `pill_w(i) + 2` | `+ pill::CAPS_COLUMNS` |
| `plan/flow.rs:575-587` | `pill::cap_style(palette::pill_focus())` for the two cap cells; the body keeps `pill_focus().add_modifier(BOLD)` |

`tmux` and pi are not ratatui: they share the spec, not this function.

### tmux adapter

`core/src/host/tmux/pill.rs` (new, `mod pill;` in `tmux.rs`), pure string functions, no `Tmux` value:

```rust
/// `#[fg=colourN]`
pub fn fg(index: u8) -> String;
/// `#{?cond,#[fg=colourA],#[fg=colourB]}`: a colour that depends on a tmux condition (the pane title: `pane_active`).
pub fn fg_if(cond: &str, then: u8, otherwise: u8) -> String;
/// `fg` + `lead` + left cap + `#[reverse]` + `text` + `#[noreverse]` + right cap. `lead` is what sits before the left cap
/// in the pill's colour (the pane title's `━`; empty otherwise). `text` may be a tmux format expression.
pub fn pill(fg: &str, lead: &str, text: &str) -> String;
/// `#[range=user|<name>]` + `inner` + `#[norange]`.
pub fn range(name: &str, inner: &str) -> String;
```

Callers add the tail themselves: `format!("{}#[default]", pill(..))` for the status pills, nothing for the border title. `fg_if("pane_active", focus, plain)` takes its two indices from `spec().meanings.focus` and `.plain`. All indices print as `colourN` (so `blue` becomes `colour4`: the same colour, `docs/contract.md` 6.1.1 line 2358 "tmux: `colour0` ... `colour15` (`blue` is 4)"); the tests that pin `fg=blue` in the title pill change (section 5).

Replaced: `title_pill!` (`tmux.rs:81-88`, uses 104, 106), `button!` (255-259, uses 303, 305, 307), `pill!` (261-265, uses 308, 311, 312).

### pi adapter

`pi/lib/pill.ts` (new):

```ts
export const spec: PillSpec;
/** `ESC[<30+n|90+n-8>m` cap ESC[7m text ESC[27m `ESC[<..>m` cap ESC[39m, n an index 1-15. */
export function pill(index: number, text: string): string;
export function clickable(index: number): number;   // the spec's rule: n+8 for 1-7, else n
export function meaning(name: keyof PillSpec["meanings"]): number;
```

Replaced: `indices.ts:75-79` (`pill`, and its `foreground()` at 22-24 moves to `pill.ts` or stays and is imported: `pill.ts` must not import `compact-tools` to keep `lib/` free of the extension tree; so `foreground` moves to `lib/pill.ts` and `indices.ts` re-exports it), `statusPill` 82-84 and `thinkingPill` 87-89 call `pill(meaning(..), text)`; `hint.ts:13-14,35-36,68` (`pillN`, `titlePill`: calls `pill(meaning("plain"), ..)` etc.); `render.ts:136-137,166` (`CAP_LEFT/CAP_RIGHT` and the `\x1b[35m` literal: `pill(meaning("planning"), "cpd")`). The callers `compact-tools.ts:310`, `compact-tools-thinking.ts:251`, `compact-tools-editor.ts:87`, `compact-tools-progress.ts:132` keep calling `pill`/`statusPill` and are unchanged. Output stays byte-identical for every pi pill (no golden changes; the `hint.ts` pills at index < 8 print what they print today).

## 5. The test plan

**What pins what, per renderer.** Each decodes its output back to `(left cap, right cap, colour index, text reversed, caps reversed, text)` and compares to the spec.

| Renderer | Decoder | Pins caps | Pins reverse | Pins colour |
|---|---|---|---|---|
| ratatui | the atom's `Vec<Span>` painted into a `Buffer` (`tui/src/pill_colours.rs:42-60` `assert_pill` does this today) | `symbol()` of the two cap cells equals `spec().caps` | `Modifier::REVERSED` on every text cell, none on the caps | cap `fg` and text `fg` equal the palette `Color` of the spec index; `bg` is `Reset` |
| tmux | `cells()` of `core/src/host/tmux/palette.rs:51-73`, a pure parser of `#[fg=..]`, `#[reverse]`, `#[noreverse]`, `#[default]` (no tmux process; the file's `Server` stays out of these tests) | first and last non-style char equal `spec().caps` | `reverse` true on the text cells, false on the caps | `fg` equals the spec index on all |
| pi | a regex SGR decoder over `pill()`'s string in `pi/test/pill-spec.test.ts` (the pi tests already match SGR, `pi/test/pill.test.ts:27`) | the two chars around the text equal `spec.caps` | `ESC[7m`..`ESC[27m` spans exactly the text | the leading `ESC[3Nm`/`ESC[9Nm` decodes to the spec index |

**Parity test (fails when one renderer drifts).** All three tests iterate the same table: every `meanings` key, base and clickable (rule applied), plus a text with a space, a one-character text and an ellipsis. Because each renderer is checked against the one file, a renderer that drifts fails its own test, and a change to the file moves all three together. A cross-Rust check goes in `cli/tests/tooling/pill_parity.rs`: for each row the ratatui tuple and the tmux tuple are equal (the pi tuple is the pi test's, same file). A fourth guard, `cli/tests/tooling/pill_sites.rs` (modelled on `pill_lines.rs:82-97` `scan`, and `palette/colours.rs:114` `no_dim_or_reversed_outside_the_palette`), fails when U+E0B6 or U+E0B4 appears in any non-test source under `tui/src`, `core/src`, `engine/src`, `cli/src`, `pi/lib`, `pi/compact-tools`, `pi/ask-user-question` other than `pi/lib/pill-spec.json`, and when `\x1b[7m` or `#[reverse]` appears outside the three adapters. This is what stops a sixth hand-assembled copy; without it the parity tests would pass on the sites that use the atom and say nothing about the ones that do not.

**Spec tests** (`core/src/pill.rs`, `pi/test/pill-spec.test.ts`): the file parses; every key of section 2 is present; an unknown key, a missing key, an index 0 or above 15 each fail with a message; the clickable rule gives the contract's table (7 to 15, 6 to 14, 3 to 11, 2 to 10, 1 to 9, 5 to 13, 4 to 12, 8 to 8) as `palette/colours.rs:64-88` pins today.

**Existing tests that change** (names read from the files):

- `tui/src/pill_colours.rs`: `static_pill_text_is_inverted_on_colour_7`, `interactive_pill_text_is_inverted_on_colour_15`, `focus_pill_text_is_inverted_on_colour_4`, `section_title_pills_are_inverted_with_caps_in_colour_7`, `job_pills_are_inverted_bright_with_caps_in_their_colour`, `legend_pills_are_inverted_with_caps_in_their_colour` (their `CAP_L/CAP_R` import goes; they read `spec().caps`; the assertions stay).
- `tui/src/palette/colours.rs`: `each_meaning_has_its_decided_base_colour_and_bright_pair` (the table stays; the `bright` row at 83-86 changes to the spec's rule), `job_pills_are_their_status_colour_bright__inverted`, `read_only_pills_are_grey_inverted_blocks`.
- `tui/src/section.rs` (`check` module 91-130 and the tests 142-264), `tui/src/tree/look.rs:28`, `tui/src/widgets/column.rs:797`, `tui/src/plan/flow.rs` tests (648-676 `a_pill_is_cap_icon_key_bar_tier_cap_with_no_padding`, ...), `tui/src/widgets/legend.rs:481,630`: their cap constants come from `spec()` or stay spelled (Q2).
- `core/src/host/tmux/palette.rs`: `title_pills_are_grey_blocks_unfocused_and_blue_blocks_focused`, `the_workers_and_version_pills_are_grey_blocks_inverted`, `selections_menus_and_copy_mode_are_blue_blocks` (the first two use real tmux; Q3).
- `cli/tests/session/pill_lines.rs`: `border_format_title_pills_follow_the_rule`, `status_line_pills_follow_the_rule` (`fg=blue` becomes `fg=colour4`; strings are otherwise identical); `cli/tests/session/host.rs:161` `BORDER_FORMAT_WANT` and `:531` suffix; `cli/tests/tui/tui.rs:132`, `:278`; `cli/tests/session/session.rs:158`; `cli/tests/tooling/harness.rs:580` (the `SERVER_OPTIONS` type).
- pi: none changes (output is byte-identical): `pi/test/pill.test.ts`, `pill-brand.test.ts`, `pill-split.test.ts`, `ask-tabs.test.ts`, `ask-header.test.ts`, `compact-tools/pills.test.ts`, `working-pill.test.ts`, `palette-indices.test.ts` keep passing and become the regression net.

## 6. Job split

Each Job is sized for one Attempt (about 150 turns). `touches` are the files it may change.

| Job | Kind | touches | Needs | Turns (guess) |
|---|---|---|---|---|
| `pill-docs` | docs | `docs/contract.md` (6.1.1 "Where the colours live", "Job pills", the title-pill paragraph at §6.9/§15.9), `docs/implementing.md` (a "The pill spec" paragraph and the new file) | none | 40 |
| `pill-spec-data` | implementation | `pi/lib/pill-spec.json`, `core/src/pill.rs`, `core/src/lib.rs` (one `pub mod`); the tests of `Spec::parse` and of the clickable rule inside `core/src/pill.rs` | none | 40 |
| `pill-pi-ts-reader` | implementation | `pi/lib/pill.ts` (the reader, `pill`, `clickable`, `meaning`), `pi/test/pill-spec.test.ts` (spec tests and the decode parity) | `pill-spec-data` (the file) | 50 |
| `pill-red-tui` | red tests | `tui/src/pill.rs` (stub: `pill`, `cap_style`, `CAPS_COLUMNS`, `unimplemented!`), `tui/src/lib.rs` (one `mod`), `tui/src/pill_colours.rs` (parity table over `spec()`) | `pill-spec-data` | 40 |
| `pill-red-tmux` | red tests | `core/src/host/tmux/pill.rs` (stub), `core/src/host/tmux.rs` (one `mod`), a pure test over `cells()` (no real tmux) | `pill-spec-data` | 40 |
| `pill-impl-tui-atom` | implementation | `tui/src/pill.rs`, `tui/src/palette.rs` (`color`, `color_clickable`, index to `Color`), `tui/src/palette/colours.rs` | `pill-red-tui` | 60 |
| `pill-impl-tui-sites-a` | implementation | `tui/src/section.rs`, `tui/src/dialog/view.rs`, `tui/src/tree/look.rs`, `tui/src/widgets/column.rs` (test consts) | `pill-impl-tui-atom` | 70 |
| `pill-impl-tui-sites-b` | implementation | `tui/src/widgets/legend.rs`, `tui/src/plan/flow.rs`, `tui/src/pill_colours.rs` (imports) | `pill-impl-tui-atom` | 70 |
| `pill-impl-tmux` | implementation | `core/src/host/tmux/pill.rs`, `core/src/host/tmux.rs`, `core/src/host/tmux/palette.rs`, `cli/tests/session/pill_lines.rs`, `cli/tests/session/host.rs`, `cli/tests/session/session.rs`, `cli/tests/tui/tui.rs`, `cli/tests/tooling/harness.rs` (a test names `SERVER_OPTIONS`/`STATUS_FORMAT` and cannot compile otherwise) | `pill-red-tmux` | 110 |
| `pill-impl-pi` | implementation | `pi/lib/render.ts`, `pi/lib/hint.ts`, `pi/compact-tools/extensions/compact-tools-indices.ts` | `pill-pi-ts-reader` | 50 |
| `pill-guard` | tests | `cli/tests/tooling/pill_sites.rs`, `cli/tests/tooling/pill_parity.rs`, `cli/tests/tooling/main.rs` (two `mod` lines) | all implementation Jobs (it fails until the last hand-assembled copy goes) | 40 |

**Order and parallelism.** `pill-spec-data` first (everything reads its file or its `spec()`). Then in parallel, no file shared: `pill-docs`, `pill-pi-ts-reader`, `pill-red-tui`, `pill-red-tmux`. Then in parallel: `pill-impl-tui-atom` (after `pill-red-tui`), `pill-impl-tmux` (after `pill-red-tmux`), `pill-impl-pi` (after `pill-pi-ts-reader`). Then `pill-impl-tui-sites-a` and `-b` in parallel (after the atom; `-b`'s `pill_colours.rs` only calls `section::` functions, whose signatures `-a` keeps). `pill-guard` last: it is the one test that cannot be red-first and mergeable, since it fails until the sites are migrated. `pill-impl-pi` does not wait for tui or tmux: its output is byte-identical to today's.

Conflicts to watch: `pill-impl-tmux` changes the type of `SERVER_OPTIONS`; any Job that reads it (only `cli/tests/...` and `harness.rs`, all in its `touches`) must not run in parallel with it. `tui/src/lib.rs` and `core/src/lib.rs` get one `mod` line each from different Jobs (`pill-red-tui`, `pill-spec-data`): no overlap, but a `Cargo.lock` is untouched by all (no dependency added).

## 7. Open questions for the User

**Q1. Where does the one spec file live?** It must be under `pi/` (finding in 3(b)), so the Rust crates read from the pi adapter's directory.
- (a) `pi/lib/pill-spec.json` (recommended): no copy, no script change, one `include_str!` outside the crate, as `tui/src/logs/mod.rs:138` already does for `docs/`.
- (b) `spec/pill.json` at the root plus a symlink in `pi/lib/` and `cp -RL` in `install-cpd.sh:130`: the file stays out of the adapter tree, at the cost of editing a Gate-checked script and a link that dangles when copied with plain `cp -R`.
- (c) Option 2 or 3 of section 3: no cross-tree read, but a second source.

**Q2. Do tests spell the caps or read them from the spec?**
- (a) Tests may keep spelling U+E0B6/U+E0B4 (recommended): they pin the contract's glyphs, so a deliberate spec change fails the tests and someone updates them on purpose. `cli/tests/tui/*.rs` already spell about 30 rows.
- (b) Tests read `spec().caps`: a spec edit passes silently, but 30 rows stop naming the glyph.
The guard of section 5 allows (a) by scanning non-test sources only.

**Q3. The three tmux tests that start a real tmux** (`core/src/host/tmux/palette.rs:159,186,215`): policy `tests` says no test uses the real tmux.
- (a) Replace them with the pure `cells()` decode (recommended by policy): no process, no socket, but nothing then checks that tmux itself renders `#[reverse]` as claimed.
- (b) Keep one real-tmux smoke test as an exception the policy owner grants.
This Plan's red tests use `cells()` only either way.
