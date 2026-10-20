-- cpd-migration: breaking after 21
-- Stacked PRs and PR needs across Plans (contract §2.2 "The base PR", §3.8 "Stacked PRs", §4.5 `stacked-prs`, hs-d324,
-- hs-d325). Breaking: it rebuilds `pr_needs` (a rebuild copies only the columns its author knew, K4) and relaxes
-- `prs_locked` (A1), and a binary that does not know it would start a PR whose need is a PR of another Plan as if it had
-- no need. It adds no column to `prs`: the base PR is derived from `base`. The runner runs it with `foreign_keys = OFF`
-- and `legacy_alter_table = ON`; the generated history triggers of `pr_needs` are the reconcile's (K4).

-- 1. `pr_needs` carries the needed PR's Plan; every existing need is a need of the same Plan. The copy writes no history.
CREATE TABLE pr_needs_new (
  plan       TEXT NOT NULL,
  pr         TEXT NOT NULL,
  needs_plan TEXT NOT NULL,
  needs      TEXT NOT NULL,
  PRIMARY KEY (plan, pr, needs_plan, needs),
  FOREIGN KEY (plan, pr)            REFERENCES prs(plan, key),
  FOREIGN KEY (needs_plan, needs)   REFERENCES prs(plan, key),
  CHECK (NOT (needs_plan = plan AND needs = pr))
) STRICT;
INSERT INTO pr_needs_new (plan, pr, needs_plan, needs) SELECT plan, pr, plan, needs FROM pr_needs;
DROP TABLE pr_needs;
ALTER TABLE pr_needs_new RENAME TO pr_needs;

-- 2. The hand-written triggers of `pr_needs` (they went with the dropped table), as §4.4 has them.
CREATE TRIGGER pr_needs_locked BEFORE INSERT ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = NEW.plan) NOT IN ('grounding', 'plan')
 AND (SELECT created_txn FROM prs WHERE plan = NEW.plan AND key = NEW.pr) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER pr_needs_no_delete BEFORE DELETE ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER pr_needs_hist_del AFTER DELETE ON pr_needs BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pr_needs',
    json_array(OLD.plan, OLD.pr, OLD.needs_plan, OLD.needs), 'delete',
    json_object('plan', OLD.plan, 'pr', OLD.pr, 'needs_plan', OLD.needs_plan, 'needs', OLD.needs), NULL);
END;

-- 3. `prs_locked` with its second exception: `base` alone may follow GitHub, on an `open` PR (the poll) or on a PR that
-- is stacked, i.e. whose `base` is the `lead_branch` of a PR of its repo (the unstack of a merged base PR).
DROP TRIGGER prs_locked;
CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status = 'merged'
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.base IS OLD.base
          AND OLD.lead_branch = 'cpd/' || OLD.plan
          AND NEW.lead_branch = 'cpd/' || OLD.plan || '/' || OLD.key)
 AND NOT (OLD.status IN ('waiting', 'dispatch', 'open')
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.lead_branch IS OLD.lead_branch
          AND NEW.base IS NOT OLD.base
          AND (OLD.status = 'open'
               OR EXISTS (SELECT 1 FROM prs q WHERE q.repo = OLD.repo AND q.lead_branch = OLD.base)))
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
