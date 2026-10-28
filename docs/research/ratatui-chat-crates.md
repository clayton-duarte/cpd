# ratatui chat: crates survey (skeleton)

Part of Plan `ratatui-chat-research`, Job `crates`, Attempt `crates-a1`. Started 2026-10-28 14:58 EDT.

Status: skeleton; the crate table below is **ran** (crates.io API, `gh api`, `cargo tree` in a scratch crate in `/tmp`). Feature table, per-crate source reading, verdicts and summary follow in later commits.

## Crate facts (ran)

Scratch crates: `ratatui = "0.30"` plus one candidate, `cargo generate-lockfile` then `timeout 100 cargo tree --prefix none 2>/dev/null | sort -u | wc -l`. The baseline (ratatui alone) is 87 lines.

| Crate | Latest (stable) | Published | Downloads | Last push | Licence | ratatui it needs | Tree lines | Delta over ratatui |
|---|---|---|---|---|---|---|---|---|
| tui-textarea | 0.7.0 | 2024-10-22 | 2,912,286 | 2024-12-01 | MIT | ratatui ^0.29 (optional) | 107 | +20 (second ratatui 0.29 and crossterm 0.28) |
| ratatui-textarea | 0.9.3 | 2026-10-05 | 972,530 | 2026-10-08 | MIT | ratatui-core ^0.1, ratatui-widgets ^0.3 | 89 | +2 |
| tui-textarea-2 | 0.13.2 | 2026-08-23 | 207,862 | 2026-10-26 | MIT | ratatui-core ^0.1 | 90 | +3 |
| edtui | 0.11.7 | 2026-08-16 | 363,279 | 2026-10-24 | MIT | ratatui-core ^0.1 | 152 | +65 |
| tui-input | 0.15.5 | 2026-09-26 | 2,340,858 | 2026-09-26 | MIT | ratatui ^0.30.2 (optional) | 89 | +2 |
| rat-text | 3.1.0 | 2026-03-08 | 80,511 | 2026-10-28 | MIT/Apache-2.0 | ratatui-core ^0.1 | 117 | +30 |
| tui-markdown | 0.3.12 | 2026-10-19 | 585,428 | 2026-10-26 | MIT OR Apache-2.0 | ratatui-core ^0.1 | 132 (99 without default features) | +45 (+12) |
| ratatui-markdown | 0.3.6 | 2026-05-21 | 8,940 | 2026-10-26 | MIT OR Apache-2.0 | ratatui ^0.29 | 154 | +67 (second ratatui) |
| pulldown-cmark | 0.13.5 | 2026-10-21 | 189,188,932 | 2026-10-28 | MIT | none | 92 | +5 |
| comrak | 0.59.0 | 2026-10-26 | 9,633,266 | 2026-10-26 | BSD-2-Clause | none | 150 | +63 |
| termimad | 0.35.5 | 2026-09-23 | 8,917,378 | 2026-09-28 | MIT | none (crossterm own) | 116 | +29 |
| syntect | 5.3.0 | 2025-09-27 | 36,249,821 | 2026-10-27 | MIT | none | 119 (118 pure-Rust regex) | +32 |
| two-face | 0.5.2 | 2026-08-07 | 13,726,659 | n/a (codeberg) | MIT OR Apache-2.0 | none | 111 | +24 |
| tree-sitter-highlight | 0.27.1 | 2026-10-08 | 4,532,759 | 2026-10-28 | MIT | none | 105 | +18 |
| synoptic | 2.2.9 | 2024-11-30 | 2,334,406 | 2024-11-30 | MIT | none | 97 | +10 |
| tui-scrollview | 0.6.8 | 2026-09-24 | 513,713 | 2026-10-28 | MIT OR Apache-2.0 | ratatui-core ^0.1 | 88 | +1 |
| ansi-to-tui | 8.0.1 | 2026-01-10 | 7,483,075 | 2026-10-10 | MIT | ratatui-core ^0.1 | 91 | +4 |
| ratatui-image | 11.1.0 (12.0.0-rc.3 pre) | 2026-10-18 (rc) | 1,240,831 | 2026-10-18 | MIT | ratatui ^0.30.1 | 217 | +130 |
