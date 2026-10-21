-- cpd-migration: breaking after 34
-- Needs and base PRs of one Plan only (contract §3.8, §4.5 `local-needs`, hs-d522). Breaking: it rebuilds `pr_needs`
-- without `needs_plan`, and a binary that does not know it would write rows the table no longer has a column for. The
-- runner runs it with `foreign_keys = OFF` and `legacy_alter_table = ON`; the generated history triggers of `pr_needs`
-- are the reconcile's (K4).

-- 0. A need of another Plan fails the migration (the NOT NULL of the guard), it is never dropped.
CREATE TEMP TABLE local_needs_guard (cross_plan_need INTEGER NOT NULL);
INSERT INTO local_needs_guard (cross_plan_need) SELECT NULL FROM pr_needs WHERE needs_plan <> plan;
DROP TABLE local_needs_guard;

-- 1. `pr_needs` without `needs_plan`. The copy writes no history.
CREATE TABLE pr_needs_new (
  plan  TEXT NOT NULL,
  pr    TEXT NOT NULL,
  needs TEXT NOT NULL,
  PRIMARY KEY (plan, pr, needs),
  FOREIGN KEY (plan, pr)    REFERENCES prs(plan, key),
  FOREIGN KEY (plan, needs) REFERENCES prs(plan, key),
  CHECK (needs <> pr)
) STRICT;
INSERT INTO pr_needs_new (plan, pr, needs) SELECT plan, pr, needs FROM pr_needs;
DROP TABLE pr_needs;
ALTER TABLE pr_needs_new RENAME TO pr_needs;

-- 2. The hand-written triggers of `pr_needs` (they went with the dropped table).
CREATE TRIGGER pr_needs_locked BEFORE INSERT ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = NEW.plan) NOT IN ('grounding', 'plan')
 AND (SELECT created_txn FROM prs WHERE plan = NEW.plan AND key = NEW.pr) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER pr_needs_no_delete BEFORE DELETE ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;
CREATE TRIGGER pr_needs_hist_del AFTER DELETE ON pr_needs BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pr_needs',
    json_array(OLD.plan, OLD.pr, OLD.needs), 'delete',
    json_object('plan', OLD.plan, 'pr', OLD.pr, 'needs', OLD.needs), NULL);
END;
