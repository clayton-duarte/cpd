//! The tree's archive row and its fold (commands.md §13.2 "Archive folder row"; contract §6.1.1; hs-d698, hs-d707):
//! one clickable row at the tree root, after the last live Context, `󰬦 archive` folded and `󰬬 archive` expanded,
//! every cell colour 8, present only when at least one Context is archived, folded by default, its fold persisted
//! under `"archived"` in `run/tui/tree-folds.json`.
//!
//! Red until the tree draws them: today's tree has no archive row, so every test here fails at runtime on an
//! assertion, never at build time.
