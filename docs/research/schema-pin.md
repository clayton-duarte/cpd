# The schema pin without a shared line (design, for review)

Draft in progress (Job pin-design, Plan quality). Measurements first; options and recommendation follow.

## 1. What the pin protects today

(draft) Two pins: `SCHEMA_HASH` (`core/src/store/schema.rs`) and `PINNED` (`core/src/events/kind.rs`).

### Measured (2026-10-12 .. 2026-10-20, the whole history of the repo)

- PRs whose own commits edited `SCHEMA_HASH`: #75 #94 #95 #107 #118 #119 #122 #136 #138 #139 #143 #152.
- PRs whose own commits edited `PINNED`: #28 #30 #37 #105 #107 #118 #136 #143 #152.
- Replayed merge commits of those PRs with `git merge-tree`: 14 distinct merges conflicted on the `SCHEMA_HASH` line, 2 also on `PINNED`.
