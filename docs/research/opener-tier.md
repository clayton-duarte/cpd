# The opener Job's default Tier

**Recommended default: `H3`.**

The opener is the `open` (and `update`) Job a PR's last Worker runs: it
merges `origin/<base>` into the Lead Branch only when the two conflict,
resolves the conflicts, then submits, which runs the repo Gate, pushes and
opens the pull request (`docs/contract.md` §2.2 "Job kinds", §7.7). The model
work is a handful of turns of conflict reasoning and coordination; the Gate is
CPU-bound and its cost is Tier-independent.

Of the three candidate Tiers `H1`, `H2` and `H3`, only `H3` can be the default
at all:

- cpd's Tier rules allow a Worker exactly `H3` and `S0`–`S3`
  (`docs/contract.md` §9 "Tier rules", hs-d554; `core/src/config/tier.rs`,
  `Tier::is_worker_allowed`).
- `open_tier` must be a Tier a Worker may have, else the config is refused
  `config_invalid` (§8.2), and engine E3 never starts an Attempt on a Job whose
  Tier is outside the range (§3.7). `H1` and `H2` would therefore neither load
  as the default nor run: they are not recommendations, they are refused input.

So `H3` is the only one of the three that is a legal opener Tier. The rest of
this note gives the live data that says a low-cost Tier is the right end of the
range for this Job.

## The live store

All numbers come from `$CPD_HOME/cpd.db` read-only, joining `attempts` to
`jobs` on `(plan, key)` for `kind = 'open'`. As of the run below the store held
150 such Attempts across 140 Jobs and 68 Plans (started
`2026-10-21T13:13:01Z` … `2026-10-25T18:27:59Z`).

```sql
WITH o AS (
  SELECT a.tier AS tier, a.outcome AS outcome, a.cause AS cause,
         json_extract(a.metrics, '$.cost_usd')   AS cost_usd,
         json_extract(a.metrics, '$.turns')      AS turns,
         json_extract(a.metrics, '$.duration_s') AS duration_s
  FROM attempts a
  JOIN jobs j ON a.plan = j.plan AND a.job = j.key
  WHERE j.kind = 'open'
),
ranked AS (
  SELECT tier, cost_usd, turns, duration_s,
    ROW_NUMBER() OVER (PARTITION BY tier ORDER BY cost_usd)   AS rn_c,
    COUNT(cost_usd)   OVER (PARTITION BY tier)                AS n_c,
    ROW_NUMBER() OVER (PARTITION BY tier ORDER BY turns)      AS rn_t,
    COUNT(turns)      OVER (PARTITION BY tier)                AS n_t,
    ROW_NUMBER() OVER (PARTITION BY tier ORDER BY duration_s) AS rn_d,
    COUNT(duration_s) OVER (PARTITION BY tier)                AS n_d
  FROM o
),
pct AS (
  SELECT tier,
    AVG(CASE WHEN rn_c IN ((n_c + 1) / 2, (n_c + 2) / 2) THEN cost_usd   END) AS med_cost,
    AVG(CASE WHEN rn_c = (9 * n_c + 9) / 10                THEN cost_usd   END) AS p90_cost,
    AVG(CASE WHEN rn_t IN ((n_t + 1) / 2, (n_t + 2) / 2) THEN turns      END) AS med_turns,
    AVG(CASE WHEN rn_t = (9 * n_t + 9) / 10                THEN turns      END) AS p90_turns,
    AVG(CASE WHEN rn_d IN ((n_d + 1) / 2, (n_d + 2) / 2) THEN duration_s END) AS med_dur,
    AVG(CASE WHEN rn_d = (9 * n_d + 9) / 10                THEN duration_s END) AS p90_dur
  FROM ranked
  GROUP BY tier
),
agg AS (
  SELECT tier,
    COUNT(*)                      AS attempts,
    SUM(outcome = 'merged')       AS merged,
    SUM(outcome = 'failed')       AS failed,
    SUM(outcome = 'cancelled')    AS cancelled,
    SUM(cause = 'merge_conflict') AS merge_conflict,
    (SELECT group_concat(cause || ':' || n, ', ') FROM (
       SELECT cause, COUNT(*) AS n
       FROM o o2
       WHERE o2.tier = o.tier AND o2.outcome = 'failed'
       GROUP BY cause
    )) AS failed_causes
  FROM o
  GROUP BY tier
)
SELECT a.tier, a.attempts, a.merged, a.failed, a.cancelled, a.merge_conflict,
       COALESCE(a.failed_causes, '-') AS failed_causes,
       ROUND(p.med_cost, 4)           AS med_cost_usd,
       ROUND(p.p90_cost, 4)           AS p90_cost_usd,
       ROUND(p.med_turns, 1)          AS med_turns,
       ROUND(p.p90_turns, 1)          AS p90_turns,
       ROUND(p.med_dur, 1)            AS med_duration_s,
       ROUND(p.p90_dur, 1)            AS p90_duration_s
FROM agg a
JOIN pct p USING (tier)
ORDER BY a.tier;
```

Run verbatim (it exits 0):

```
timeout 300 sqlite3 -readonly "$CPD_HOME/cpd.db" "<the query above>"
```

```
S1|28|28|0|0|0|-|0.0037|0.0607|12.5|48.0|403.1|879.7
S2|121|111|1|9|0|crashed:1|0.0292|0.1227|4.0|16.0|344.5|738.1
S3|1|1|0|0|0|-|0.0015|0.0015|7.0|7.0|239.8|239.8
```

### Aggregates per open-Job Attempt Tier

| Tier | Attempts | merged | failed | cancelled | merge_conflict | failed causes | median cost_usd | p90 cost_usd | median turns | p90 turns | median duration_s | p90 duration_s |
|------|---------:|-------:|-------:|----------:|---------------:|---------------|----------------:|-------------:|-------------:|----------:|------------------:|---------------:|
| S1   | 28       | 28     | 0      | 0         | 0              | —             | 0.0037          | 0.0607       | 12.5         | 48.0      | 403.1             | 879.7          |
| S2   | 121      | 111    | 1      | 9         | 0              | crashed:1     | 0.0292          | 0.1227       | 4.0          | 16.0      | 344.5             | 738.1          |
| S3   | 1        | 1      | 0      | 0         | 0              | —             | 0.0015          | 0.0015       | 7.0          | 7.0       | 239.8             | 239.8          |

Method: `attempts`, `merged`, `failed`, `cancelled`, `merge_conflict` and
`failed_causes` cover every Attempt; the percentiles use `json_extract` of
`metrics`. Median is the mean of the two middle values (the middle one when the
count is odd); p90 is the nearest-rank value at `ceil(0.9 * n)`. Nine S2
Attempts are `cancelled` before any metrics were written, so S2's percentiles
rest on 112 of its 121 Attempts. The single S2 failure is `cause = 'crashed'`
(a driver that died), not conflict reasoning.

## What the store says

- **The opener does not fail on conflict.** Across all 150 recorded open-Job
  Attempts there is not one `cause = 'merge_conflict'`. The only failure is a
  single `crashed`; every `failed` and `merge_conflict` count otherwise is zero.
  The Job this research is about—merging a conflicting base—has never been the
  thing that failed.
- **A low-cost Tier is already the norm.** `S1` (28 Attempts) and `S3` (1) both
  ran with zero failures. `S1`'s median cost is an order of magnitude below
  `S2`'s (`$0.0037` vs `$0.0292`), which is the lever the Tier choice pulls.
- **The store has no `H` Attempt at all.** There is no measured `H1`, `H2` or
  `H3` opener to point at; the aggregate table has exactly three rows (`S1`,
  `S2`, `S3`). This is labelled: the `H3` recommendation rests on the Tier rule
  above (which is code, verified in `core/src/config/tier.rs` and
  `docs/contract.md` §9) plus the S-tier evidence that the opener's model work
  does not fail — not on an `H`-tier measurement, which does not exist.

## The trade-off

A cheaper default buys a failed Attempt as a possibility: the Job's ladder then
retries at a higher effort, up to `retry_cap` (default 2). For `H3` the ladder
is `H3 → S3` and then end (`docs/contract.md` §9 "Ladder"), so the worst case
is bounded: at most one step up, inside the allowed range, before the Job
fails. For `H1` and `H2` there is no ladder at all — a Tier a Worker may not
have has `next = none`, and E3 will not start it — so the apparent saving of a
lower effort would not even be spent. `H3` is the lowest opener Tier that is
allowed, and the data gives no sign that its work is the fragile part.
