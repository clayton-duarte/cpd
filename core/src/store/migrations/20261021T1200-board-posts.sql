-- cpd-migration: additive
-- The message board (hs-d514; `cpd board`): posts that name who they are for. A post appends no Event and never starts a
-- turn; the adapter takes a Session's unread posts at its turn start (`board take`). The history triggers are generated.
CREATE TABLE board_posts (
  id           INTEGER PRIMARY KEY,
  at           TEXT    NOT NULL,
  context      TEXT    NOT NULL REFERENCES contexts(id),
  from_session TEXT,
  to_session   TEXT    NOT NULL REFERENCES sessions(id),
  text         TEXT    NOT NULL CHECK (length(text) BETWEEN 1 AND 2000),
  read_at      TEXT
) STRICT;
CREATE INDEX board_posts_delivery ON board_posts(to_session, read_at);
INSERT INTO history_tables (name, no_delete) VALUES ('board_posts', 1);
