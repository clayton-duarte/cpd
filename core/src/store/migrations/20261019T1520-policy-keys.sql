-- cpd-migration: breaking after 18
-- The policy keys (contract §4.5 "The policy keys", §4.4, §8.3, hs-d288): the
-- Context's owner and the PR's repo name, the Event kind `context-owner`
-- (§5.2) and the fill of both columns. The history triggers of `contexts` and
-- `prs` are derived (K4): the reconcile regenerates them with the new columns.

ALTER TABLE contexts ADD COLUMN owner TEXT CHECK (owner IS NULL OR length(owner) BETWEEN 1 AND 39);
ALTER TABLE prs ADD COLUMN name TEXT CHECK (name IS NULL OR length(name) BETWEEN 1 AND 100);
INSERT INTO event_kinds (name) VALUES ('context-owner');
-- a PR's name from its own pull request's owner/name, when linked
UPDATE prs SET name = lower((SELECT substr(r.repo, instr(r.repo, '/') + 1) FROM pull_requests r WHERE r.id = prs.pull_request))
 WHERE pull_request IS NOT NULL;
-- a Context's owner from the owner part of the pull requests of its PRs, when they all agree; else NULL
UPDATE contexts SET owner = (SELECT min(lower(substr(r.repo, 1, instr(r.repo, '/') - 1)))
                               FROM plans p JOIN prs ON prs.plan = p.id JOIN pull_requests r ON r.id = prs.pull_request
                              WHERE p.context = contexts.id)
 WHERE (SELECT count(DISTINCT lower(substr(r.repo, 1, instr(r.repo, '/') - 1)))
          FROM plans p JOIN prs ON prs.plan = p.id JOIN pull_requests r ON r.id = prs.pull_request
         WHERE p.context = contexts.id) = 1;
