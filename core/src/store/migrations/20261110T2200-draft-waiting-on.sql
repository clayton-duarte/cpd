-- cpd-migration: additive
-- draft-waiting-on (pro-d549): `jobs.waiting_on`, null when the Job is not waiting or not a draft,
-- else the target the draft is waiting on ('lead', 'user', 'explore:<job>').
ALTER TABLE jobs ADD COLUMN waiting_on TEXT;
