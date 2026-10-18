-- Migration 7 (contract §4.5, the PR poll's mergeable state, hs-d188, D1, D6):
-- `pull_requests.mergeable` and `merge_state`, and the history triggers of
-- `pull_requests` dropped and recreated so they list every column. schema.rs
-- runs this text, then generates `pull_requests_hist_ins` and
-- `pull_requests_hist_upd` from the table's columns. The `ALTER`s write no
-- history row: existing rows read `unknown` until their next poll. `pull_requests_owned` is unchanged and the
-- table still gets no `_no_delete`.
-- Frozen: the schema hash of version 7 covers this text. Change the schema
-- only with a new migration.

ALTER TABLE pull_requests ADD COLUMN mergeable TEXT NOT NULL DEFAULT 'unknown' CHECK (mergeable IN ('mergeable', 'conflicting', 'unknown'));
ALTER TABLE pull_requests ADD COLUMN merge_state TEXT NOT NULL DEFAULT 'unknown' CHECK (merge_state IN ('behind', 'blocked', 'clean', 'dirty', 'draft', 'has_hooks', 'unknown', 'unstable'));

DROP TRIGGER pull_requests_hist_ins;
DROP TRIGGER pull_requests_hist_upd;
DROP TRIGGER pull_requests_hist_del;

CREATE TRIGGER pull_requests_hist_del AFTER DELETE ON pull_requests BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pull_requests',
    json_array(OLD.id), 'delete',
    json_object('id', OLD.id, 'repo', OLD.repo, 'number', OLD.number, 'url', OLD.url, 'title', OLD.title,
                'draft', OLD.draft, 'state', OLD.state, 'review_decision', OLD.review_decision,
                'checks', OLD.checks, 'created_at', OLD.created_at, 'merged_at', OLD.merged_at,
                'changed_at', OLD.changed_at, 'mergeable', OLD.mergeable, 'merge_state', OLD.merge_state), NULL);
END;
