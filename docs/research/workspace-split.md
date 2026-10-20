# Research: workspace split by ownership area (Plan quality, Job cy-split, hs-n300 Phase 4, hs-d350)

Proposal only; no product code moves. Skeleton, filled in below.

## 1. Proposed crates
## 2. Hot files' split
## 3. Measured gain
## 4. Test selection by reverse dependencies
## 5. Migration plan and decision

## Working notes (replaced by the sections above as they are written)

- Base of every number: main `1e12a94b` (with #159 speed and #160 schema-pin). Scratch worktrees `/tmp/cy-split-a1/{today,split}`; prototype branch `scratch/cy-split-a1/engine` (local, never pushed): `cpd-engine` = `core/src/watch` + `core/src/doctor`, and a `cpd::engine` test binary (`watch`, `watch_inbox`, `doctor`). Test count 2,438 on both.
- core's module graph has one strongly connected component of 17 modules (args attempt config context driver env events git host job merge plan policy pr session spec store); ask, doctor, inbox, metrics, watch are outside it.
- Conflict history: 268 merge commits on `cpd/*` branches, 154 of them conflict when replayed with `git merge-tree` (63 of the 67 the grep finds).
