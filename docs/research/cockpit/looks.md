# Cockpit research: looks and polish

Job `insp-looks`, Plan `tui-visuals`, PR `inspiration`. Research only: no code, nothing here is decided. It builds on [survey.md](survey.md) (ids `I-01`…`I-37`) and [attention.md](attention.md) (ids `A-01`…`A-14`), and does not redo the powerline look (hs-d266: rounded caps, segments, tree icons), the StatusBar's move to tmux's status line (hs-d149), dynamic titles (hs-d257) or the Inbox. Idea ids here are `L-01`…`L-12`, ranked by how much they would make the User **more efficient**: looks are judged by what they let him see, find and hit faster, not by taste. Tags as in the survey: **F** finding what needs him, **P** following progress, **N** getting around, **L** looks and polish. Effort is **S** (a day or less, one view or two tmux values), **M** (a few days, several views, or one view plus a tmux hook) or **L** (a week or more, or crossing the engine).

## 0. The answer in eight lines

1. **What raises the look most is not more decoration, it is less chrome and one signal.** The cockpit's loudest objects are the white heavy pane borders (`docs/contract.md` §6.1, design rule hs-d124); the things that matter (needs-you dots, conflict, failed) are one coloured glyph or word. The best-looking tools make the frame faint, the focus one accent, and spend colour only on meaning (L-01).
2. **The status line is the one place that is always in view**: colour its first segment by the worst state in the cockpit, as Helix and lualine colour the mode, so "does anything need me?" is answered by peripheral vision (L-02).
3. **The cockpit's colours are the terminal's.** `tui/src/palette.rs` uses only named ANSI colours, so the User's Ghostty theme already styles everything and no theme engine is needed. That is a strength, and it has a measured weak spot: under his own themes, "dark grey" text and fills fall below a 3:1 contrast in one, and red and yellow are near-twins in the other (§2.2, L-03).
4. **One pill grammar everywhere.** hs-d266 gives headers, Job pills and the status line rounded caps; the Deploy column's state words (`open ✓ approved conflict`) and the PR box titles stay plain text. Chips for the states that change what he does next (conflict, changes requested, failed) extend the grammar (L-05).
5. **Motion should mean "something changed" or "in progress", nothing else.** A row that just changed flashes for a few seconds and then calms (htop does this, L-04); a second, slower rhythm for "waiting on someone else" separates it from "working" (L-12).
6. **Make the pointer visible.** The views already receive mouse-move events and use none; a faint tint under the pointer and a quiet selected row, instead of full-row reverse video, make a mouse-first cockpit feel alive (L-08).
7. **Empty states are a place for the next click**, not a dim line (L-09).
8. **Two things tmux cannot do**: round the pane borders (no `rounded` in `pane-border-lines`) and hover on the status line. Everything inside the ratatui views and popups can be rounded and can hover (L-07, L-08).

The top three (L-01, L-02, L-03) are S each and need no new data. §5 draws the cockpit at 215×62 with L-01, L-02, L-05, L-06 and L-07 applied; §6 draws the shaded-panel alternative (L-11).

## 1. Method and sources

- Started from the survey's lookers (btop, bosun, superfile, crush, posting, lazygit, zellij) and read deeper through `gh` (`gh api repos/<owner>/<repo>/readme`, `contents/<path>`), not the web. New tools read for this theme: [Catppuccin](https://github.com/catppuccin/catppuccin) and its [tmux port](https://github.com/catppuccin/tmux), [starship](https://github.com/starship/starship) presets, [Powerlevel10k](https://github.com/romkatv/powerlevel10k), [Helix](https://github.com/helix-editor/helix) (statusline and themes docs), [lualine.nvim](https://github.com/nvim-lualine/lualine.nvim), [yazi](https://github.com/sxyazi/yazi)'s default theme, [zellij](https://github.com/zellij-org/zellij)'s default config and themes, [opencode](https://github.com/sst/opencode)'s theme docs, [htop](https://github.com/htop-dev/htop), [tachyonfx](https://github.com/ratatui/tachyonfx), [cli-spinners](https://github.com/sindresorhus/cli-spinners), [Nerd Fonts](https://github.com/ryanoasis/nerd-fonts), Ghostty's [`Config.zig`](https://github.com/ghostty-org/ghostty/blob/HEAD/src/config/Config.zig) and the [lipgloss](https://github.com/charmbracelet/lipgloss) README.
- Read on this machine: `tmux(1)` of tmux 3.8 (`tmux -V`; the cockpit's minimum is 3.4, so every tmux feature here is **unverified on 3.4** unless said otherwise); the ratatui 0.30.2 / ratatui-widgets 0.3.2 and crossterm 0.29.0 sources in the local cargo registry (cpd pins `ratatui = "0.30"`, `Cargo.toml`); the User's dotfiles, `~/dotfiles/home/dot_config/ghostty/{config,themes/CPD,themes/Vesper}`; and cpd's own `docs/contract.md` §6.1, `docs/commands.md` §13.2 and §17, `tui/src/palette.rs`, `core/src/host/tmux.rs`. The powerline plan text (hs-d266) was read from the Plan's records, since it is not on `main` yet.
- Screenshots cannot be seen. Every look is described from README, docs, config or source; **inferred** marks a guess from names, code paths or behaviour. Nothing here was seen on a screen, including cpd's own look: it is read from the docs and source.
- The contrast and colour-distance numbers in §2.2 are **my own calculations** from the two theme files (WCAG 2 contrast ratio; CIE76 ΔE in CIELAB, D65). They are exact for those hex values and say nothing about how a screen renders them.
- Mockups: colour is lost. The conventions of attention.md carry over: `●` red, `◐` yellow, `◆` magenta, `○` white or idle, `⠹` a working spinner (dot colours are the Session dot rule, `docs/commands.md` §17). `◖text◗` stands for a rounded-cap segment (U+E0B6 and U+E0B4 in the real thing). `⌂` stands for the Context icon, `⎇` for the Plan Lead icon, `⑂` for the fork icon. In §5, `┃ ━ ┣ ┫` are a **colour stand-in for the accent (blue) edge of the focused pane**: in tmux every border has one weight (inferred: `pane-border-lines` is one option, with no per-pane form in the man page), the difference is colour only. Content is invented. Full-screen mockups are 215×62 cells with today's geometry (tree 43 columns, plan and chat 127, Deploy 43, two separators, one status row; `docs/commands.md` §13.1, as attention.md used).

## 2. Where cpd's look stands today

### 2.1 What is there, from the docs and source

| Element | Today | Where |
|---|---|---|
| Pane borders | tmux `pane-border-lines heavy`, white (top-level separators are white by User rule), the focused pane's border blue | `docs/contract.md` §6.1 (table), `docs/commands.md` §13.2 design rule (hs-d124) |
| Pane headers | the title in a reversed block in the border's colour; hs-d266 adds a rounded cap each side | §6.1 "The pane border format"; Plan text hs-d266 |
| Section headers inside a slot | `──[ PRs ]──`, grey (colour 8), title a reversed grey block, not bold | §13.2 design rule |
| Colour | **named ANSI colours only**: `Color::White/DarkGray/Green/Yellow/Red/Magenta/Cyan/Black`; no `Rgb`, no `Indexed`; a test fails when another file names a colour | `tui/src/palette.rs` (`color()`, `ui()`, `pill()`); `grep -rn "Color::\(Rgb\|Indexed\)" tui/src` finds nothing |
| Job pills | black text on the status colour, ` <icon> key │S1 `; `running` icon spins on one shared 100 ms clock (9 frames) | `palette::pill`; §13.2 "Chart" |
| Dots | one glyph `󰪥` in four colours for "needs you" (red, yellow, magenta, white), a 9-frame spinner for "working" | §13.2 "tree"; §17 dot rule |
| PR boxes | `Borders::ALL` (plain `┌┐`), colour 8, with arrows drawn as `┬`, `╭╮`, `▼`: two corner styles in one picture | §13.2 "PR boxes" (its own mockup shows `┌ ┐` boxes with `╭──╮` arrows) |
| Tree menu | `Block::new().borders(Borders::ALL)`, plain | `tui/src/tree/menu.rs:187` |
| Popups | `display-popup -E -T ' legend '` (default border) | §13.2 "legend", "logs" |
| Selection | tree cursor is a full inverted row, keyboard only; "a click never moves or shows it" | §13.2 "Cursor" |
| Hover | none. Views enable mouse capture, which (crossterm 0.29) writes `?1003h`, any-motion tracking; the tree's test sends `MouseEventKind::Moved` and expects nothing | crossterm-0.29.0 `src/event.rs:329`; `tui/src/tree/view.rs:1388` |
| Empty states | one dim line: `no Plan yet`, `no PR yet: cpd pr add`, ` no open PRs`, `loading… (first fetch)`, `No Lead here: pick a Plan in the session tree` | §13.2 |
| Truecolour | not asked for: `terminal-features` appends only `extkeys` and `hyperlinks` entries. `infocmp -x xterm-ghostty` on this machine lists `Tc`, so tmux can pass RGB; the Mac's terminfo is **unverified** | §6.1; `core/src/host/tmux.rs:103` |

### 2.2 The terminal theme is the theme: measured on the User's two Ghostty themes

The User's dotfiles manage Ghostty: `config` says `theme = Vesper`, and a `themes/CPD` file with a muted 16-colour palette sits beside it, unused by the config as read here (the Mac's copies are **unverified**; same chezmoi source). Because the cockpit draws with ANSI names, each theme restyles every view. Ghostty also offers `light:`/`dark:` pairs and `minimum-contrast` (a WCAG contrast floor, default 1 = off) ([Config.zig](https://github.com/ghostty-org/ghostty/blob/HEAD/src/config/Config.zig), `theme`, `minimum-contrast`). What the two palettes do to cpd's meanings (my calculation, §1):

| Measure | Vesper (`theme =` today) | CPD theme |
|---|---|---|
| Background / foreground | `#101010` / `#ffffff` | `#262626` / `#bcbcbc` |
| Colour 8 (DarkGray: Nothing, Done, grey headers, zero counters) on background | **4.69** : 1 | **2.60** : 1 |
| Done pill text (black on colour 8) | 4.69 | **2.92** |
| Section header block (background-coloured text on grey 8) | 4.69 | **2.60** |
| Pill text black on red / green / yellow / blue / magenta / cyan / white | 9.4 / 8.7 / 10.7 / 7.9 / 9.0 / 7.5 / 7.3 | 4.50 / 4.50 / 4.53 / 4.52 / 4.52 / 4.48 / 4.50 |
| Red vs yellow, ΔE | **18** (`#f5a191` hue 10°, `#e6b99d` hue 23°: two peach tones) | 40 (hue 0° vs 60°) |
| Red vs magenta, yellow vs magenta, red vs green ΔE | 34, 39, 50 | 44, 47, 55 |

Reading it: under Vesper the pills and text are easy to read, but **red and yellow are close**, and cpd leans on exactly that pair (the "needs you" dot is red, yellow, magenta or white with one precedence; PR states `open` is yellow and `conflict` and `closed` are red). Under the CPD theme red and yellow are far apart, but everything drawn in colour 8 sits at 2.6 : 1, below the 3 : 1 that WCAG 2.1 asks of user-interface components (WCAG 1.4.11; a figure from the standard, not fetched here). I infer that a small dot of near-twin hues is hard to tell apart at a glance; I have not seen it on screen. This is evidence for L-03 and for attention.md's A-09 (shape as well as colour).

### 2.3 What would raise it most (from the above)

1. The frame is louder than the content (L-01), and the headline state is not in the line of sight (L-02).
2. The meaning colours are only as good as the theme under them, and nobody checks (L-03).
3. State words are the least designed text on screen (L-05); the corner styles disagree (L-07); the pointer does nothing visible (L-08).
4. What is already good, to keep: one margin rule (a blank column each side, a blank row on top), right-aligned counters with dim zeros, one shared spinner clock, a single palette file, ids as labels. These match what the best tools do and are not touched below.

## 3. What makes a TUI look sleek: the patterns found

| Pattern | Where it shows up | Source |
|---|---|---|
| A **semantic token set**, small: text / muted / faint, border / faint border, selected bg, success / warning / error | gh-dash defines `text.primary, secondary, inverted, faint, warning, success`, `background.selected`, `border.primary, secondary, faint` and requires all of them; opencode names `primary, secondary, accent, error, warning, success, info, text, textMuted, …`; btop has `main_fg`, `hi_fg`, `inactive_fg`, `selected_bg/fg` | [gh-dash theme.mdx](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/theme.mdx), [opencode themes.mdx](https://github.com/sst/opencode/blob/HEAD/packages/web/src/content/docs/themes.mdx), [btop adapta.theme](https://github.com/aristocratos/btop/blob/HEAD/themes/adapta.theme) |
| **Follow the terminal** instead of shipping a palette | opencode's `system` theme: "uses ANSI colors (0-15)", "generates gray scale based on your terminal's background color", "`none` for text and background"; zellij ships an `ansi.kdl` theme; btop's `main_bg=""` is "terminal default … need to be empty if you want transparent background" | opencode themes.mdx; [zellij themes](https://github.com/zellij-org/zellij/tree/HEAD/zellij-utils/assets/themes); btop adapta.theme |
| A **neutral ladder** for surfaces | Catppuccin's style guide: Base = pane background; Crust and Mantle = secondary panes; Surface 0–2 = surface elements; Overlay 0–2 = overlays and "subtle"; Subtext = labels. Its own warning: "Legibility always comes first" | [style-guide.md](https://github.com/catppuccin/catppuccin/blob/HEAD/docs/style-guide.md) |
| **Rounded corners as an option** everywhere | btop `rounded_corners`, lazygit `gui.border: rounded`, zellij `rounded_corners`, ratatui `BorderType::Rounded`, tmux `popup-border-lines rounded` | btop [README config](https://github.com/aristocratos/btop#configurability); lazygit [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md); zellij [default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl); ratatui-widgets 0.3.2 `src/borders.rs` (read locally; `Plain, Rounded, Double, Thick`, dashed, `QuadrantInside/Outside`); tmux(1) 3.8 |
| **Powerline segments with a grammar** | Catppuccin tmux's module = left cap + icon block + middle separator + text block + right cap, all from options; starship ships `pastel-powerline`, `catppuccin-powerline`, `tokyo-night` presets, and a `no-nerd-font` preset as the fallback; lualine's sections A B C … X Y Z with section and component separators; yazi's status bar has `sep_left`/`sep_right` glyph pairs | [catppuccin/tmux `status_module.conf`](https://github.com/catppuccin/tmux/blob/HEAD/utils/status_module.conf); [starship presets](https://github.com/starship/starship/tree/HEAD/docs/public/presets/toml); [lualine README](https://github.com/nvim-lualine/lualine.nvim#usage-and-customization); [yazi theme-dark.toml](https://github.com/sxyazi/yazi/blob/HEAD/yazi-config/preset/theme-dark.toml) |
| **A status segment coloured by state** | Helix: `ui.statusline.normal`, `.insert`, `.select` when `editor.color-modes` is on; lualine themes have a colour set per mode (`custom_gruvbox.normal.c.bg`) | [Helix themes.md](https://github.com/helix-editor/helix/blob/HEAD/book/src/themes.md), [editor.md](https://github.com/helix-editor/helix/blob/HEAD/book/src/editor.md); lualine README |
| **Gradients on meters** | btop: "All graphs and meters can be gradients … Use start, mid and end for a three colour gradient" (`cpu_start/mid/end`, `temp_*`, `free_*`) | btop adapta.theme |
| **Change highlight that expires** | htop `-H`/`--highlight-changes[=DELAY]`: "Highlight new and old processes", 5 s by default | [htop.1.in](https://github.com/htop-dev/htop/blob/HEAD/htop.1.in) |
| **Three states per list** | gh-dash's table has `EmptyState`, `loadingMessage` and a spinner, drawn by `isLoading`, `len(Rows)==0`, else rows | [table.go](https://github.com/dlvhdr/gh-dash/blob/HEAD/internal/tui/components/table/table.go) lines 23–26, 297–302 |
| **Spinners with their own rhythm** | cli-spinners: 70+ spinners, each `{interval, frames}` (`dots` 80 ms); lazygit's spinner is configurable (`frames: ●∙∙ ∙●∙ ∙∙●`, `rate: 180`) | [cli-spinners](https://github.com/sindresorhus/cli-spinners); lazygit Config.md |
| **Shade instead of lines** | bosun: "Opencode aesthetic. Borderless, subtly shaded panels, bold accents, modal dialogs with left accent bars" | [bosun README](https://github.com/yetidevworks/bosun), as read by the survey (I-28) |

Not found useful for cpd: ASCII fallbacks (starship `plain-text-symbols`, lazygit's empty `nerdFontsVersion`) since the Nerd Font is installed on every machine (hs-d266; dotfiles `font-meslo-lg-nerd-font`); Powerlevel10k's transient prompt ([README](https://github.com/romkatv/powerlevel10k#transient-prompt): trims old prompts so scrollback stays dense), because the cockpit has no scrollback of its own; fancy borders such as Double or QuadrantOutside, which make the frame louder, against L-01.

## 4. The ideas, ranked

Ranked by efficiency for the User first (what he sees, finds and hits faster), then by effort. Each idea: **pattern** → **source** → **cpd mapping** → **effort**, with a mockup where layout matters. The full-cockpit mockup of §5 shows L-01, L-02, L-05, L-06, L-07 and L-09 together.

### L-01. Quiet chrome, loud signal (pain: L, F)

- **Pattern:** the frame recedes, the focus is one accent, colour is spent on meaning. Inactive borders and hints take a "faint" token; only the focused edge is bright.
- **Source:** lazygit's `gui.theme`: `activeBorderColor: green, bold` against `inactiveBorderColor: default` ([Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)); gh-dash's required `text.faint`, `border.faint`, `border.secondary` ([theme.mdx](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/theme.mdx)); btop's `inactive_fg` "Color of inactive/disabled text" ([adapta.theme](https://github.com/aristocratos/btop/blob/HEAD/themes/adapta.theme)); opencode's `textMuted` ([themes.mdx](https://github.com/sst/opencode/blob/HEAD/packages/web/src/content/docs/themes.mdx)).
- **cpd mapping:** *Chrome.* Today the top-level separators are white and heavy and the focused one blue (`docs/contract.md` §6.1; hs-d124). White heavy lines are the brightest objects in a 215×62 screen of mostly text, and they sit right next to the dots, pills and words that carry meaning. Proposal: `pane-border-style fg=colour8` (the same grey as the in-slot headers, so top-level and in-slot separators differ by weight, not by brightness), keep `pane-active-border-style fg=blue`, and consider `pane-border-lines single` (it is one option for all panes, inferred from the man page). Headers keep the reversed block (hs-d105, hs-d266 caps): grey block unfocused, blue block focused, so the focused pane is found by its header, not by a bright frame. *Signal.* Reserve bold and fill for "needs you / failed / conflict" and for the focused header; a row that is calm uses default or grey text. The mockup in §5 shows the geometry; colour is described in its table. **This reverses a User rule (hs-d124: white top-level separators): present it as a proposal for him to look at, not for the Lead to apply.** Check the grey against both of his themes first (L-03): colour 8 is 4.69 : 1 on Vesper and 2.60 : 1 on the CPD theme (§2.2); lines are fine at 2.6, small text is not.
- **Effort:** S. Two values in `SERVER_OPTIONS` (`core/src/host/tmux.rs`), one paragraph of `docs/contract.md` §6.1 and §13.2, and the exact-string test in `cli/tests/host.rs`.

### L-02. The status line's first segment is the cockpit's worst state (pain: F, L)

- **Pattern:** a segment at the start of the status line whose colour is the current state, so it is read without reading.
- **Source:** Helix `ui.statusline.normal / .insert / .select` when `editor.color-modes` is true ([themes.md](https://github.com/helix-editor/helix/blob/HEAD/book/src/themes.md), [editor.md](https://github.com/helix-editor/helix/blob/HEAD/book/src/editor.md)); lualine themes hold a colour set per mode ([README](https://github.com/nvim-lualine/lualine.nvim#usage-and-customization)); yazi's `[status]` `sep_left`/`sep_right` ([theme-dark.toml](https://github.com/sxyazi/yazi/blob/HEAD/yazi-config/preset/theme-dark.toml)); agent-deck's waiting strip in the tmux status line ([README](https://github.com/asheshgoplani/agent-deck#notification-bar), survey I-23).
- **cpd mapping:** the StatusBar (tmux status line after hs-d149; hs-d266's `pl-tmux` already gives the right side a segment around `@cpd_workers`, a status option cpd writes). Add a lead segment after the buttons, `◖● 3 need you◗`, filled with the colour of the **worst Session dot** of the cockpit by the existing precedence `red > yellow > magenta > white` (§17 dot rule), and calm grey `◖○ all quiet◗` when none: the User learns that the bottom-left colour *is* the answer to "does anything need me?". This is only the look; the numbered, clickable list of who waits is attention.md's A-01 and the Inbox owns the items. The segment can share A-01's click ranges (tmux `range=user|X`).
- **Effort:** S: a format string, one more option the watcher already knows how to set, and a test of the exact string. **Depends on** the status-line PR (`statusline`) and, for the count, on the Inbox read model; the dot-colour version needs neither.

```
needs you        ⧉ ? ▤  ◖● 3 need you◗                          workers 6/6 · 4 waiting · 9 queued   (segment red)
in review        ⧉ ? ▤  ◖◐ 1 in review◗                         workers 6/6 · 4 waiting · 9 queued   (yellow)
all calm         ⧉ ? ▤  ◖○ all quiet◗                           workers 2/6 · 0 waiting · 0 queued   (grey, dim)
```

### L-03. Audit the palette against the User's themes; never colour alone (pain: L, F)

- **Pattern:** apps use roles on top of the terminal's 16 colours; the terminal theme carries the taste; the theme's author guarantees legibility ("Legibility always comes first").
- **Source:** opencode's `system` theme (ANSI 0–15 plus a derived grey scale, [themes.mdx](https://github.com/sst/opencode/blob/HEAD/packages/web/src/content/docs/themes.mdx)); zellij's `ansi.kdl`; Catppuccin's [style guide](https://github.com/catppuccin/catppuccin/blob/HEAD/docs/style-guide.md) (its "ANSI Color Generation" section derives the 16 colours from the palette); Ghostty `theme`, `light:…,dark:…` and `minimum-contrast` ([Config.zig](https://github.com/ghostty-org/ghostty/blob/HEAD/src/config/Config.zig)).
- **cpd mapping:** no theme engine (see §7: survey I-32 is not recommended). Instead three small things. (1) A **palette audit**: a throwaway script or test that reads a Ghostty theme file and prints, for every `Meaning` and `Ui` colour, its contrast against the background and the ΔE between each pair that must not be confused (red/yellow/magenta/white of the dot rule; Failed/Review/Done of the PR states). The numbers in §2.2 are that audit done once by hand. (2) **Fix what the audit finds, in the palette, not the theme**: under the CPD theme, colour 8 text is 2.6 : 1 (Done pills 2.92) — either raise it to the theme's colour 15/7 with `DIM`, or keep 8 only for lines; under Vesper, red and yellow are ΔE 18, so make the two dot states differ **also by glyph or weight** (attention.md A-09), and print `conflict`/`failed` bold. (3) Ghostty `minimum-contrast = 3` in dotfiles is a one-line global fallback for the pill text (`palette::pill` is black on colour), at the cost of nudging colours in other programs (unverified how it treats a fill that is a cell's background: the doc says it concerns foreground against background).
- **Effort:** S. The audit is about fifty lines and needs no runtime code; the fixes are a few palette lines and one glyph rule. Not a per-user setting: convention over configuration.

### L-04. A change flashes, then calms (pain: P, F)

- **Pattern:** a row that just appeared or changed is highlighted for a few seconds, then returns to normal; the delay is a setting.
- **Source:** htop `--highlight-changes[=DELAY]` "Highlight new and old processes", default 5 s ([htop.1.in](https://github.com/htop-dev/htop/blob/HEAD/htop.1.in)); tachyonfx composes fades and sweeps over a rendered buffer: "50+ unique effects", `fx::fade_from`, `fx::coalesce` ([README](https://github.com/ratatui/tachyonfx); `fx` functions read in tachyonfx 0.25.2 in the local registry; it needs only `ratatui-core 0.1.2`, which ratatui 0.30 uses; not a cpd dependency today).
- **cpd mapping:** every ratatui view already redraws on a 100 ms clock for the spinners. When a snapshot differs from the last one, the cells that changed (a Job pill flipping status, a counter, a PR's check or review word, a new Deploy row, a Pulse bucket) are drawn **reversed for 1.5 s, bold for 3 s more, then normal**. ANSI named colours cannot be interpolated, so steps, not a fade (a smooth fade needs truecolour: L-11). So when the User glances at the cockpit after ten minutes of chat, the movement is findable. It is the progress twin of attention.md's A-07 (new versus seen: loud until looked at); A-07 is about needs, this is about everything that moved. Keep it off the tree's dots (they already move).
- **Effort:** M. Each of the plan, tree and widgets views keeps its previous snapshot, diffs it, and stores a `changed_at` per row or card; `palette` gets one `flash(age)` helper. No data change. Cost to watch: redraws on a timer only while something is flashing.

```
t+0.0 s   ◖⠹ insp-looks │S3◗ ──▶ ◖✓ insp-looks │S3◗      (the pill that flipped is reversed)
t+1.5 s   same pill, bold
t+4.5 s   same pill, normal
```

### L-05. One chip grammar for every state word (pain: F, L)

- **Pattern:** a state is a small capped segment with the same silhouette everywhere, so "what state" reads by shape and fill before it is read as text.
- **Source:** Catppuccin tmux modules (left cap, icon block, middle separator, text block, right cap, all configurable: [status_module.conf](https://github.com/catppuccin/tmux/blob/HEAD/utils/status_module.conf)); starship's [`pastel-powerline`](https://github.com/starship/starship/blob/HEAD/docs/public/presets/toml/pastel-powerline.toml) preset; lazygit's branches panel shows a GitHub icon whose colour is the pull request's state ([README](https://github.com/jesseduffield/lazygit#show-github-pull-requests), via the survey).
- **cpd mapping:** hs-d266 gives pane headers, section headers, Job pills and the StatusBar the caps. Still plain text: the Deploy row's `open|draft ✓ approved|changes requested|no review conflict` (`docs/commands.md` §13.2 "widgets") and the PR box title's `#31 open ↗`. One helper, `palette::chip(meaning, text)`, shared with `pl-pills`, draws a state as a filled capped chip. **A rule to keep it calm (L-01):** fill only a state that changes what he does next: `conflict`, `changes requested`, `failed`, `draft`; `open`, `approved`, `no review` stay coloured words. In the 43-column Deploy column the second line `  open ✓ approved conflict  <title>` is 28 columns, so the title (cut first by the existing rule) loses four more cells to two caps; acceptable.
- **Effort:** S–M. The chip helper is small; touching the Deploy row, the box title and the legend sample means three views and their snapshot tests.

```
today                                        with chips
 cpd#412                         ↗            cpd#412                         ↗
  open ✓ approved conflict  Plan inbox…        open ✓ approved ◖conflict◗  Plan i…
 cpd#401                         ↗            cpd#401                         ↗
  draft ✗ no review  Pulse sparkline            ◖draft◗ ✗ no review  Pulse spark…
```

### L-06. The header is a small status line (pain: F, P, L)

- **Pattern:** a title with a right-hand badge: a count, or a state.
- **Source:** gh-dash `ui.sectionsShowCount` "whether the number of results show up next to each section's title" ([theme.mdx](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/theme.mdx)); zellij's default `pane_frame_style "titles"` is a title line above each pane, which is what cpd's tmux headers are ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl)); lualine's two ends of the line, A B C … X Y Z ([README](https://github.com/nvim-lualine/lualine.nvim#usage-and-customization)).
- **cpd mapping:** the pane headers say only where you are (` Context `, ` Plan `, ` Deploy `, the Lead's title). Add a right-aligned capped badge: Context `2 need you`; Plan `tui-visuals · 6/16 merged`; Deploy `1 conflict`; Lead `◐ waiting for you`. It is the always-visible cause marker of attention.md's A-08 and the Inbox section's count; it also gives the empty header line a job. tmux cannot read the data itself, so cpd sets one more pane option per view (a second deliberate exception to "pane options identify panes, never state", contract §6.2, the first being `@cpd_title`, which hs-d257 already makes dynamic). Right alignment inside `pane-border-format`: `#[align=right]` is a style in tmux(1) 3.8's STYLES list ("align text … if appropriate"), documented there without saying which formats honour it, so whether a pane border does is **unverified**; the fallback is the `p` pad modifier (`#{p10:…}`, documented in the same man page, its minimum tmux version **unverified**).
- **Effort:** M. A pane option per view, a writer in each view or the engine, and the format. Depends on hs-d257's title plumbing.

### L-07. Rounded inside, square outside, one weight each (pain: L)

- **Pattern:** corner style is a deliberate choice, applied everywhere a box is drawn.
- **Source:** btop `rounded_corners`; lazygit `border: rounded`; zellij `rounded_corners` ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl) also has per-side styles `border_top "double"`); lipgloss `RoundedBorder` ([README](https://github.com/charmbracelet/lipgloss)); ratatui `BorderType::Rounded` (ratatui-widgets 0.3.2 `src/borders.rs`, read locally); tmux `popup-border-lines rounded` and `menu-border-lines` (tmux(1) 3.8, read locally). tmux's `pane-border-lines` takes only `single`, `double`, `heavy`, `simple`, `number`: **pane borders cannot be rounded**.
- **cpd mapping:** `docs/commands.md` §13.2's own PR-box picture mixes `┌┐` boxes and `╭╮` arrows, and the tree menu is a plain box (`tui/src/tree/menu.rs:187`): move PR boxes and menus to `BorderType::Rounded` so every box in a view matches the arrows and the pills' caps; give `display-popup` for the legend and logs `-b rounded` (`display-popup -b border-lines`, and the option `popup-border-lines`, whose values include `rounded`, `padded` and `none`: tmux(1) 3.8; the minimum version is **unverified** against the cockpit's 3.4). The tmux frame stays square, which reads as "the frame" against "the things in it" (inferred). Lines inside a slot stay light, the frame one weight; no `═`, no mixed weights in one border.
- **Effort:** S. A `BorderType` argument in two views and one tmux flag; the PR chart fixture `tui/src/plan/fixtures/cpd-chart.txt` and the box tests change in the same commit.

### L-08. Tint the selection, and light up what the pointer is on (pain: N, L)

- **Pattern:** the selected row is a quiet background tint, not reverse video; the row under the pointer gets a lighter one.
- **Source:** lazygit `selectedLineBgColor` ("If you don't like the default behaviour of highlighting the selected line with a blue background … just embolden the selected line") and a separate `inactiveViewSelectedLineBgColor` ([Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)); btop `selected_bg`/`selected_fg` ([adapta.theme](https://github.com/aristocratos/btop/blob/HEAD/themes/adapta.theme)); Catppuccin's neutral ladder, Surface 0–2 for "surface elements" ([style-guide.md](https://github.com/catppuccin/catppuccin/blob/HEAD/docs/style-guide.md)); crossterm 0.29 `EnableMouseCapture` writes `?1003h` (any-motion), already enabled by the tree and plan views.
- **cpd mapping:** the cockpit is mouse-first, and clicking is the main verb, yet **nothing says what is clickable**: a click never shows the cursor, the cursor is full-row reverse, and mouse moves are received and dropped. Proposal: a hover tint on rows and cards that a click acts on (tree Sessions, Pulse rows, Job pills, owned Deploy PR rows, menu items; not dead PR rows, which are drawn dim and ignore clicks), and the keyboard cursor as a stronger tint with a left accent bar `▌` instead of the full reverse row. The tint is a 256-colour index (`Color::Indexed`) or an RGB neutral, so `palette.rs` gains one `tint(level)` (the colour test keeps it the only place). Not possible on tmux chrome (the status line has no hover: survey I-08); whether tmux forwards motion to a pane app that asked for it is **unverified** here (the `MouseMove` key does not exist in tmux(1) 3.8's key list; forwarding is to the pane, not to a binding).
- **Effort:** S for the selection tint; M for hover (a `hover` field per view, redraw on motion events, coalesced so ssh does not carry a redraw per cell).

### L-09. Empty and loading states are designed, and carry a click (pain: N, L)

- **Pattern:** every list has three drawn states: loading (spinner and message), empty (message), data.
- **Source:** gh-dash [table.go](https://github.com/dlvhdr/gh-dash/blob/HEAD/internal/tui/components/table/table.go) (lines 23–26 declare `EmptyState`, `loadingMessage`, `loadingSpinner`; 297–302 pick loading, empty or rows). Others are plain text, which is the comparison: lazygit's empty list is the string "No changed files" ([english.go](https://github.com/jesseduffield/lazygit/blob/HEAD/pkg/i18n/english.go)).
- **cpd mapping:** each message today is one dim line at a pane's bottom (§2.1). Draw them as a small centred block: a dim headline, one line of what to do, and, where an action exists that needs no more than a name, a chip that is a button running the command the menu already runs (`+ New Plan` is the tree's `n` and the Context menu's *New Plan…*; `+ New Context` is the empty-area menu item, `docs/commands.md` §13.2). The loading state uses the shared spinner clock and says what it waits for. Messages that need a path or a PR key (`cpd pr add`) stay text.
- **Effort:** S. Message rendering and one click target per view; no data.

```
today (Plan view, no Lead)                  proposed
                                                         no Lead in the centre
 no Lead session in the centre             pick a Plan in the Context column, or
                                                        ◖ + New Plan ◗
```

### L-10. Colour ramps on meters and ages (pain: P)

- **Pattern:** a meter's colour moves along a start / mid / end ramp with the value.
- **Source:** btop: "All graphs and meters can be gradients … Use 'start', 'mid' and 'end' for three colour gradient" (`cpu_start/mid/end`, `temp_*`: [adapta.theme](https://github.com/aristocratos/btop/blob/HEAD/themes/adapta.theme)); ratatui `Sparkline` and `LineGauge` (ratatui-widgets 0.3.2 `src/sparkline.rs`, `src/gauge.rs`, read locally). Survey I-11 already proposes the braille sparkline; this is its colour.
- **cpd mapping:** the Deploy column's host section (CPU and memory %, coming with `deploy-host`), the Pulse sparkline (green for a merge already) and the ages (`quiet 41m`, PR age): three ramp steps from the existing meanings, `InProgress` green → `Review` yellow → `Failed` red, so a hot machine or a stalled Plan is seen in colour before the number is read. ANSI-only means three steps, not a smooth gradient; a smooth one needs RGB (L-11).
- **Effort:** S. One `ramp(value, lo, hi) -> Meaning` function in the palette and the host row's drawing.

```
 ⌂ hs
 cpu ▰▰▰▱▱▱▱▱▱▱ 38%   mem ▰▰▰▰▰▰▱▱▱▱ 61%      (green, yellow)
 cpu ▰▰▰▰▰▰▰▰▰▱ 91%   mem ▰▰▰▰▰▰▱▱▱▱ 61%      (red, yellow)
```

### L-11. Shaded panels: a neutral ladder instead of lines (pain: L)

- **Pattern:** panels are told apart by a slightly different background; borders disappear; titles float as pills; dialogs get a left accent bar.
- **Source:** bosun, "Borderless, subtly shaded panels, bold accents, modal dialogs with left accent bars and drop shadow" ([README](https://github.com/yetidevworks/bosun); survey I-28); Catppuccin's ladder: Base pane, Crust/Mantle "secondary panes", Surface 0–2 ([style-guide.md](https://github.com/catppuccin/catppuccin/blob/HEAD/docs/style-guide.md)); opencode's `system` theme builds the greys from the terminal's own background ([themes.mdx](https://github.com/sst/opencode/blob/HEAD/packages/web/src/content/docs/themes.mdx)).
- **cpd mapping:** the most "designed" look, and the biggest change. In tmux it is per-pane `window-style`: the side columns (Context, Deploy) one step darker than the centre (Plan, chat), the separators becoming one-cell gutters in the darkest tone; views draw with the default background so the pane's tint shows. §6 sketches it. Costs and doubts: (a) **it needs RGB**: today's `terminal-features` has no RGB entry (the local `xterm-ghostty` terminfo has `Tc`; the Mac's is **unverified**); (b) it contradicts hs-d124 (white top-level separators) and changes the reversed-title headers; (c) the chat pane is pi's: whether pi paints its own background over a tinted pane is **unverified**; (d) the 16 ANSI colours were chosen by the theme to sit on its own background, and on a tinted one the contrast of §2.2 shifts. A *proposal for the User to look at*; do it after L-01 and L-03, and only if he wants more than L-01 gives.
- **Effort:** M. Palette work, a tint per `@cpd_view` set at pane creation, a pass over each view and header; no behaviour change.

### L-12. A motion budget: spin for working, pulse for waiting (pain: P, L)

- **Pattern:** every spinner has its own frames and interval; animation means something.
- **Source:** [cli-spinners](https://github.com/sindresorhus/cli-spinners) (each entry `{interval, frames}`; `dots` is 80 ms); lazygit's configurable spinner (`rate: 180`, [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)); tachyonfx for one-shot transitions ([README](https://github.com/ratatui/tachyonfx)).
- **cpd mapping:** today everything that is running spins on one 9-frame, 100 ms clock, and everything that waits is static (`⇥` merging, a yellow PR). Rule to write down: **motion only for "in progress and will end"; two rhythms.** Fast spin = a Lead or Worker is working. A slow pulse (1 Hz, two glyphs) for "waiting on someone else" (review, CI), so the User can tell "working: leave it" from "parked: this is waiting for a person" by rhythm alone, without reading. Optional second step, not recommended without a request: a 150–200 ms slide or fade (`slide_in`, `coalesce`) when the Plan view changes Plan. Costs: effects need 30+ fps redraws where today is 10, and the views run through tmux over ssh.
- **Effort:** S for the rule and the slow pulse (a second frame set on the existing clock); M for transitions (a new dependency and a faster redraw).

## 5. The cockpit with the S-effort ideas applied (215×62)

Today's columns (tree 43, plan and chat 127, Deploy 43). Applied: L-01 (quiet frame; the accent edge is the focused chat), L-02 (the `◖● 3 need you◗` segment), L-05 (chips on the Deploy rows and the PR box titles), L-06 (badges in the headers), L-07 (rounded boxes). L-09 is drawn in §4. The Inbox section is the in-flight one (`docs/commands.md` §21); its look follows attention.md. Colour, by element:

| Element | Colour in the picture | Idea |
|---|---|---|
| `─ │` separators and the unfocused header blocks | grey (colour 8) | L-01 |
| `━ ┃ ┣ ┫` and the focused header block `◖ tui-visuals ◗` | accent (blue) | L-01 |
| Dots, the `◖● 3 need you◗` segment | red, yellow, magenta, white by the dot rule | L-02 |
| `◖conflict◗`, `◖draft◗` chips | red; draft in the Nothing grey | L-05 |
| Job pills `◖✓ … ◗`, `◖⠹ … ◗`, `◖○ … ◗`, `◖✗ … ◗` | the status colour, text in black, as today | hs-d266 |
| Header badges `◖ 2 need you ◗`, `◖ 1 conflict ◗` | grey block; the badge turns red or yellow when it counts needs | L-06 |
| Counters `0⋅1⋅0⋅0⋅16` | zeros dim, as today | — |

```text
─◖ Context ◗────────────────◖ 2 need you ◗─┬─◖ Plan ◗────────────────────────────────────────────────────────────────────────────────────────◖ tui-visuals · 6/16 merged ◗─┬─◖ Deploy ◗────────────────◖ 1 conflict ◗──
                                           │                                                                                                                               │
 ▾ ● ⌂ building-cpd                        │  ╭─ inspiration · cpd → main · ◖#31 open◗ ↗ ────────────────────────────────────────────────────────╮                         │ ⌂ hs
⏵  ├ ◐ ⎇ tui-visuals           0⋅1⋅0⋅0⋅16  │  │  ◖✓ insp-survey │S2◗──┬──◖✓ insp-attention │S3◗──┬──◖⠹ insp-looks │S3◗──┬──◖○ insp-proposal │O3◗ │                         │ cpu 38% ⣀⣤⣶⣿⣷⣤⣀⣠   mem 61% ⣀⣠⣴⣶⣶⣶
   ├ ⠹ ⎇ cross-repo            0⋅3⋅0⋅0⋅12  │  │                      └──◖⠹ insp-navigation │S3◗─┴──◖⠹ insp-progress │S3◗─┘                       │                         │
   ├ ⠴ ⎇ quality                2⋅2⋅0⋅1⋅4  │  ╰────────────────────────────────────┬─────────────────────────────────────────────────────────────╯                         │──◖ PRs ◗─────────────────────────────────
   ├ ○ ⎇ inbox                  0⋅0⋅0⋅0⋅9  │                                ╭──────┴──────────╮                                                                            │
   └ ○ ⑂ cross-repo·7f3a                   │                                ▼                 ▼                                                                            │ cpd#412  ↗
 ▸ ○ ⌂ dotfiles                            │  ╭─ powerline · cpd → main · waiting ───────╮   ╭─ docs · dotfiles → main · ◖#88 conflict◗ ↗ ────╮                            │  ◖open◗ ✓ ◖approved◗ ◖conflict◗
 ▸ ○ ⌂ notes                               │  │  ◖○ pl-docs │S3◗──◖○ pl-tmux │S3◗        │   │  ◖✗ readme │H2│a2◗                             │                            │ cpd#409  ↗
 …                                         │  ╰──────────────────────────────────────────╯   ╰────────────────────────────────────────────────╯                            │  ◖open◗ ◐ ◖no review◗
                                           │                                                                                                                               │ dotfiles#88  ↗
                                           ┣━◖ tui-visuals ◗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━◖ ◐ waiting for you ◗━┫  ◖open◗ ✓ ◖approved◗
                                           ┃                                                                                                                               ┃ cpd#401  ↗
                                           ┃  you    Is the Inbox strip clickable yet?                                                                                     ┃  ◖draft◗ ✗ ◖no review◗
                                           ┃                                                                                                                               ┃
                                           ┃  agent  Not yet. The strip is in the Inbox PR; the status line only has the three buttons and workers.                        ┃
                                           ┃         Two things need you in this Plan: the batch review (plannotator, r3) and one question from insp-looks:                ┃
                                           ┃         "Is the powerline look final?" I answered it from hs-d266.                                                            ┃
                                           ┃                                                                                                                               ┃
                                           ┃  you    ok, go on                                                                                                             ┃
                                           ┃                                                                                                                               ┃
                                           ┃  → cpd job list --plan tui-visuals                                                                                            ┃
                                           ┃  ← 16 jobs: 6 merged, 3 running, 7 queued                                                                                     ┃
                                           ┃                                                                                                                               ┃
                                           ┃  agent  Three Workers are running. insp-looks is at 3 min, insp-navigation at 9 min.                                          ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
──◖ Inbox 3 ◗───────────────────────────── ┃                                                                                                                               ┃
 ● review     tui-visuals/inspiration      ┃                                                                                                                               ┃
 ◐ conflict   cpd#412                      ┃                                                                                                                               ┃
 ◆ question   quality                      ┃                                                                                                                               ┃
                                           ┃ ╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ ┃
                                           ┃ │ type to talk to the Lead…                                                                                                 │ ┃
                                           ┃ │                                                                                                                           │ ┃
                                           ┃ ╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ ┃
                                           ┃ ~/projects/cpd            claude-opus        ctx 34%                                                                          ┃
 ⧉ ? ▤  ◖● 3 need you◗                                                                                                                                                              workers 6/6 · 4 waiting · 9 queued
```

## 6. The shaded-panel alternative (L-11), cropped to 100×24

Same content, no separators: one-cell gutters, header pills floating at the top of each panel, an accent bar `▌` on Inbox items. Tint, which ASCII cannot draw: Context and Deploy panels **Mantle** (one step darker), Plan and chat **Base**, gutters **Crust** (darkest); the Lead's header and the input box take the accent. Needs RGB and a tinted `window-style` per pane (L-11).

```text
 ◖ Context ◗  ◖ 2 need you ◗    ◖ Plan ◗  ◖ tui-visuals · 6/16 merged ◗

  ▾ ● ⌂ building-cpd             ╭─ inspiration · cpd → main · ◖#31 open◗ ↗ ──────────────────────╮
  ├ ◐ ⎇ tui-visuals 0⋅1⋅0⋅0⋅16   │  ◖✓ insp-survey │S2◗──┬──◖✓ insp-attention │S3◗──…           │
  ├ ⠹ ⎇ cross-repo  0⋅3⋅0⋅0⋅12   │                      └──◖⠹ insp-navigation │S3◗──…           │
  └ ⠴ ⎇ quality      2⋅2⋅0⋅1⋅4   ╰──────────────────────────────────────────────────────────────╯
  ▸ ○ ⌂ dotfiles

                                ◖ tui-visuals ◗  ◖ ◐ waiting for you ◗

                                 you    Is the Inbox strip clickable yet?

                                 agent  Not yet. The strip is in the Inbox PR; the status line only
                                        has the three buttons and workers.



 ◖ Inbox 3 ◗
 ▌● review    tui-visuals
 ▌◐ conflict  cpd#412
 ▌◆ question  quality            ╭──────────────────────────────────────────────────────────────╮
                                 │ type to talk to the Lead…                                     │
                                 ╰──────────────────────────────────────────────────────────────╯

```

## 7. All ideas in one list

Cockpit area: **Tree** (Context column), **Plan** (Plan view), **Chat**, **Deploy**, **Bar** (StatusBar on tmux's status line), **Pop** (popups), **tmux** (options and layout), **Palette** (`tui/src/palette.rs`).

| Id | Idea | Area | Pain | Effort | Source | Depends on / note |
|---|---|---|---|---|---|---|
| L-01 | Quiet chrome, loud signal | tmux | L, F | S | lazygit, gh-dash, btop, opencode | reverses hs-d124 (white separators): the User decides |
| L-02 | Status segment coloured by the worst state | Bar | F, L | S | Helix, lualine, yazi, agent-deck | `statusline` PR; count wants the Inbox |
| L-03 | Palette audit against his themes; never colour alone | Palette | L, F | S | opencode, zellij, Catppuccin, Ghostty | pairs with attention.md A-09 |
| L-04 | A change flashes, then calms | Tree, Plan, Deploy | P, F | M | htop, tachyonfx | twin of attention.md A-07 |
| L-05 | One chip grammar for every state word | Deploy, Plan | F, L | S–M | Catppuccin tmux, starship, lazygit | `pl-pills` helper (hs-d266) |
| L-06 | The header is a small status line (badge) | tmux | F, P, L | M | gh-dash, zellij, lualine | hs-d257 title plumbing; generalises A-08 |
| L-07 | Rounded inside, square outside | Plan, Tree, Pop | L | S | btop, lazygit, zellij, lipgloss, ratatui, tmux | tmux cannot round pane borders |
| L-08 | Tint the selection, light up the pointer | Tree, Plan, Deploy | N, L | S / M | lazygit, btop, Catppuccin, crossterm | hover: motion forwarding unverified |
| L-09 | Empty and loading states with a click | all views | N, L | S | gh-dash, lazygit | |
| L-10 | Colour ramps on meters and ages | Deploy, Plan | P | S | btop, ratatui | `deploy-host` CPU/memory |
| L-11 | Shaded panels instead of lines | tmux, all views | L | M | bosun, Catppuccin, opencode | RGB; reverses hs-d124; pi's background unverified |
| L-12 | A motion budget: spin for working, pulse for waiting | Palette, all views | P, L | S / M | cli-spinners, lazygit, tachyonfx | |

By pain point: **F** L-01, L-02, L-03, L-04, L-05, L-06. **P** L-04, L-06, L-10, L-12. **N** L-08, L-09. **L** all twelve.

Small and independent (S, no dependency): L-01, L-03, L-07, L-09, L-10, the S halves of L-08 and L-12.

## 8. Considered and left out

- **A theme chooser with live preview** (survey I-32, crush, posting, bosun). The User already chooses a theme in one line of his dotfiles (`~/dotfiles/home/dot_config/ghostty/config`, `theme = Vesper`), the cockpit follows it by design (§2.2), and Ghostty adds light/dark pairs and a contrast floor. A second theme system inside cpd would restate what the terminal owns. If anything: L-03.
- **A truecolour-only palette.** It would make L-04's fade and L-10's gradients smooth, but would take the cockpit off the User's theme (§2.2) and needs `terminal-features` RGB on the Mac (**unverified**). Steps with ANSI colours get most of the value.
- **An ASCII or no-Nerd-Font fallback** (starship `no-nerd-font`, lazygit `nerdFontsVersion`). The font is installed on every machine by dotfiles (hs-d266); YAGNI. Note for the powerline PR: Nerd Fonts ships mono-width and double-width variants of each font ([README](https://github.com/ryanoasis/nerd-fonts#readme)); which one dotfiles installs, and whether Ghostty constrains icons to one cell, is **unverified**; the tree's `󰪥` and the caps depend on it.
- **Double or quadrant borders, drop shadows, big banners.** They make the frame louder, against L-01.
- **A theme-aware `NO_COLOR`/light-mode path.** Ghostty's `light:`/`dark:` pair already swaps the palette under ANSI colours; Helix's mode-2031 detection ([themes.md](https://github.com/helix-editor/helix/blob/HEAD/book/src/themes.md)) is only needed for RGB themes.
- **Transient collapse of finished work** (Powerlevel10k's transient prompt). The tree already folds archived rows; nothing else scrolls.

## 9. What is unverified

- Everything about how any tool *looks*: no screenshot was seen; all from READMEs, docs, config and source.
- The contrast and ΔE numbers (§2.2) are my calculation from two theme files; the Mac's and the work Mac's Ghostty themes, and which theme is active, are **unverified**; that red and yellow are hard to tell apart in Vesper is inferred from ΔE 18, not seen.
- tmux: that `pane-border-lines` has no per-pane form; that `#[align=right]` and the `p` modifier work in `pane-border-format`; that `popup-border-lines`/`display-popup -b` exist in the cockpit's minimum tmux 3.4; that tmux forwards mouse motion to a pane that enabled `?1003h`; that per-pane `window-style` tints a pane under pi. All from the 3.8 man page on this machine, none run on 3.4.
- That the User's Ghostty reaches RGB through tmux (`Tc` is in the local `xterm-ghostty` terminfo; the Mac's is not checked); whether Ghostty `minimum-contrast` applies to a filled cell's text.
- That the tachyonfx effects named in L-04 and L-12 work with ANSI named colours (its examples use `Color::Cyan` and `Color::Gray` in `fx::fade_to`, from the README; the interpolation of named colours was not run).
- Effort figures are sizes, not measurements; no code was written or run.
