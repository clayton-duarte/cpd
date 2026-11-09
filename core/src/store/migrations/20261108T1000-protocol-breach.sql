-- cpd-migration: additive
-- The Event kind `protocol-breach` (contract §5.2, §3.7 E25; Plan `protocol-enforce`, D14, air-d119): one
-- kind for every protocol's breach, `data.protocol` naming it. It replaces `bug-breach`, which
-- `20261108T0900-bug-protocol-breach` added and which E25 no longer writes. Events are append-only, so past
-- `bug-breach` Events keep their kind and the `bug-breach` row of `event_kinds` stays as history, as the retired
-- `pr-*` rows do. The Inbox kinds are unchanged (`refusal-alert` and `bug` both stay in `inbox_items`' kind
-- check): a kind exists when its `event_kinds` row does (§4.4 K1), so this is one additive statement.
INSERT INTO event_kinds (name) VALUES ('protocol-breach');
