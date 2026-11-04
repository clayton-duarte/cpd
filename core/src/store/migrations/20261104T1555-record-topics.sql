-- cpd-migration: additive
-- The provisional topic of an uncurated record (decision pro-d11): the topic kev-4b chose among the Context's topics in
-- force at write time, with the model that chose it. One row per record, written once; a curation's topic wins over it.
-- `no_delete` 0: `context delete` removes the rows with the Context's records (the record foreign key).
CREATE TABLE record_topics (
  context TEXT NOT NULL REFERENCES contexts(id),
  record  TEXT NOT NULL,
  topic   TEXT NOT NULL CHECK (length(topic) BETWEEN 1 AND 40),
  model   TEXT NOT NULL CHECK (length(model) >= 1),
  at      TEXT NOT NULL,
  PRIMARY KEY (context, record),
  FOREIGN KEY (context, record) REFERENCES context_records(context, id)
) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('record_topics', 0);
