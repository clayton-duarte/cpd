-- cpd-migration: additive
-- A PR's own Jira ticket (commands.md §9.4, §9.5): nullable, because work may start without one. A Jira key, parsed at the
-- boundary. `prs_locked` is unchanged: the ticket is not a Locked field and not in the plan fingerprint.
ALTER TABLE prs ADD COLUMN jira TEXT;
