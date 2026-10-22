-- cpd-migration: breaking after 42
-- The fourth Plan review status (contract §4.4, §4.5 `review-superseded`): `superseded`, a review cpd closed because
-- nobody can or needs to decide it any more (a newer review, the Plan done, the Plan cancelled). `decided_at` is set,
-- `feedback` stays null.
--
-- `ALTER TABLE` cannot change a CHECK, so `plan_reviews` is rebuilt as `session-title-length` rebuilt `sessions` (the
-- runner applies a migration with foreign keys off and `legacy_alter_table` on): new table with the §4.4 definition,
-- copy of every row and column (keys kept), `DROP`, `RENAME`. `plan_reviews` has no index and no hand-written trigger;
-- its history triggers are derived (§4.5 K4): the runner's reconcile creates them again after the last migration. The
-- copy writes no `history` row.
--
-- Then the reviews already stuck are closed: a pending review that is not its Plan's latest, and every pending review
-- of a done Plan. The latest pending review of a Plan that is not done is a real, open request and stays pending.
-- Last, the trigger that makes a path that forgets to close them fail loudly. No Event is appended.
--
-- Breaking (A1): a rebuild; and an older binary would refuse, at its CHECK, the status `superseded`.

CREATE TABLE plan_reviews_new (
  plan         TEXT NOT NULL REFERENCES plans(id),
  id           TEXT NOT NULL,
  number       INTEGER NOT NULL CHECK (number >= 1),
  fingerprint  TEXT NOT NULL CHECK (length(fingerprint) = 64),
  status       TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'rejected', 'superseded')),
  feedback     TEXT,
  requested_at TEXT NOT NULL,
  decided_at   TEXT,
  title        TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 72),
  PRIMARY KEY (plan, id),
  UNIQUE (plan, number),
  CHECK (id = 'r' || number),
  CHECK ((status = 'pending') = (decided_at IS NULL))
) STRICT;
INSERT INTO plan_reviews_new (plan, id, number, fingerprint, status, feedback, requested_at, decided_at, title)
  SELECT plan, id, number, fingerprint, status, feedback, requested_at, decided_at, title
  FROM plan_reviews;
DROP TABLE plan_reviews;
ALTER TABLE plan_reviews_new RENAME TO plan_reviews;

UPDATE plan_reviews SET status = 'superseded', decided_at = (SELECT at FROM txns WHERE id = cpd_txn())
 WHERE status = 'pending'
   AND (number < (SELECT max(w.number) FROM plan_reviews w WHERE w.plan = plan_reviews.plan)
        OR plan IN (SELECT id FROM plans WHERE phase = 'done'));

CREATE TRIGGER plans_done_no_pending_review BEFORE UPDATE OF phase ON plans
WHEN NEW.phase = 'done' AND OLD.phase <> 'done'
 AND EXISTS (SELECT 1 FROM plan_reviews WHERE plan = NEW.id AND status = 'pending')
BEGIN SELECT RAISE(ABORT, 'review_pending'); END;
