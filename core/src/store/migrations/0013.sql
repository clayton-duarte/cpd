-- Migration 13 (contract §4.5, the PR poll's Inbox kinds, hs-d165, D1, D4):
-- `pull_requests.authored` and `review_requested`, and the history triggers of
-- `pull_requests` dropped and recreated so they list every column. schema.rs
-- runs this text, then generates `pull_requests_hist_ins` and
-- `pull_requests_hist_upd` from the table's columns. The `ALTER`s write no
-- history row: every existing row came from the `author:@me` search, so the
-- defaults give it `authored` 1 and `review_requested` 0. `pull_requests_owned`
-- is unchanged and the table still gets no `_no_delete`.
-- Frozen: the schema hash of version 13 covers this text. Change the schema
-- only with a new migration.

ALTER TABLE pull_requests ADD COLUMN authored INTEGER NOT NULL DEFAULT 1 CHECK (authored IN (0, 1));
ALTER TABLE pull_requests ADD COLUMN review_requested INTEGER NOT NULL DEFAULT 0 CHECK (review_requested IN (0, 1));

DROP TRIGGER pull_requests_hist_ins;
DROP TRIGGER pull_requests_hist_upd;
DROP TRIGGER pull_requests_hist_del;

CREATE TRIGGER pull_requests_hist_del AFTER DELETE ON pull_requests BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'pull_requests',
    json_array(OLD.id), 'delete',
    json_object('id', OLD.id, 'repo', OLD.repo, 'number', OLD.number, 'url', OLD.url, 'title', OLD.title,
                'draft', OLD.draft, 'state', OLD.state, 'review_decision', OLD.review_decision,
                'checks', OLD.checks, 'created_at', OLD.created_at, 'merged_at', OLD.merged_at,
                'changed_at', OLD.changed_at, 'mergeable', OLD.mergeable, 'merge_state', OLD.merge_state,
                'authored', OLD.authored, 'review_requested', OLD.review_requested), NULL);
END;
