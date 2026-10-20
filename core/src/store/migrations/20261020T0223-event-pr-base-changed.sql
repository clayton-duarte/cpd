-- cpd-migration: additive
-- The Event kind `pr-base-changed` (contract §5.2, §4.5 `event-pr-base-changed`): cpd follows GitHub's repoint of a
-- stacked pull request, or unstacks a PR whose base PR merged before it had a pull request.
INSERT INTO event_kinds (name) VALUES ('pr-base-changed');
