-- cpd-migration: additive
-- The reaction queue's link to its PR and its refusal (Plan merged-emoji; contract §4.4 `slack_reactions`): `plan` and `pr` name
-- the PR whose merge the reaction is (both NULL for any other), `refused_at` and `refusal` park a reaction Slack refused
-- (both NULL or both set; never with `reacted_at`). Each CHECK sits on the later column of its pair.
ALTER TABLE slack_reactions ADD COLUMN plan TEXT;
ALTER TABLE slack_reactions ADD COLUMN pr TEXT CHECK ((plan IS NULL) = (pr IS NULL));
ALTER TABLE slack_reactions ADD COLUMN refused_at TEXT;
ALTER TABLE slack_reactions ADD COLUMN refusal TEXT CHECK ((refused_at IS NULL) = (refusal IS NULL) AND (reacted_at IS NULL OR refusal IS NULL));

-- Backfill: the rows queued by a merge are the ones whose emoji is the post's `reaction`.
UPDATE slack_reactions
   SET plan = (SELECT p.plan FROM pr_posts p WHERE p.channel = slack_reactions.channel AND p.ts = slack_reactions.ts AND p.reaction = slack_reactions.emoji),
       pr   = (SELECT p.pr   FROM pr_posts p WHERE p.channel = slack_reactions.channel AND p.ts = slack_reactions.ts AND p.reaction = slack_reactions.emoji)
 WHERE EXISTS (SELECT 1 FROM pr_posts p WHERE p.channel = slack_reactions.channel AND p.ts = slack_reactions.ts AND p.reaction = slack_reactions.emoji);
