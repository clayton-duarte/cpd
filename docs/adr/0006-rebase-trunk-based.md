---
status: accepted
---

# Approved Attempts are rebased onto the Lead Branch and fast-forwarded

The User works trunk-based and values a linear history. An approved Attempt's commits are kept (no squash), rebased onto the current Lead Branch HEAD and fast-forwarded into it, one at a time (worktrunk `wt merge --no-squash`). The Lead Branch stays linear, and the PR is rebased onto the default branch only when conflicts require it.

## Consequences

- Rebased commits get new SHAs; references to an Attempt's commits are recorded after the merge, not before.
- Merges are serialised; a concurrent merge fails its compare-and-swap ref update and retries after the next rebase.

## Considered Options

- `--no-ff` merge commits: keeps original SHAs but adds merge commits; rejected for trunk-based linear history.
