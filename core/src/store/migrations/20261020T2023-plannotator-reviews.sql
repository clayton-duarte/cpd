-- cpd-migration: additive
-- The plannotator sessions a cpd Session has open (hs-d496; `cpd review`): one row per session, opened and closed by
-- the pi adapter's tracker. E8's `review_waiting` turns an open row into a magenta `review-waiting` Inbox item.
-- The key is `(session, pn_id)`: a pn id is unique only within one pi process. The history triggers are generated.
CREATE TABLE plannotator_reviews (
  session   TEXT    NOT NULL REFERENCES sessions(id),
  pn_id     TEXT    NOT NULL CHECK (pn_id GLOB 'pn-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]' AND length(pn_id) = 9),
  kind      TEXT    NOT NULL CHECK (kind IN ('annotate', 'review', 'last', 'plan')),
  subject   TEXT    NOT NULL CHECK (length(subject) BETWEEN 1 AND 200),
  port      INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
  opened_at TEXT    NOT NULL,
  closed_at TEXT,
  ended     TEXT    CHECK (ended IN ('decided', 'lost')),
  PRIMARY KEY (session, pn_id),
  CHECK ((closed_at IS NULL) = (ended IS NULL))
) STRICT, WITHOUT ROWID;
INSERT INTO history_tables (name, no_delete) VALUES ('plannotator_reviews', 1);
INSERT INTO event_kinds (name) VALUES ('review-opened'), ('review-closed');
