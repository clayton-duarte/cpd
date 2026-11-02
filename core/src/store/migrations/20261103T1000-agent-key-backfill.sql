-- cpd-migration: breaking after 101
-- Name every Manager, Lead (not a fork) and Worker by its agent key (pro-d345): `<context>`, `<context>:<plan>`,
-- `<context>:<plan>:<job>`. Data only; a fork keeps its free-text name.
UPDATE sessions SET name = CASE
    WHEN role = 'manager' THEN context
    WHEN role = 'lead' THEN context || ':' || plan
    WHEN role = 'worker' THEN context || ':' || plan || ':' || (SELECT job FROM attempts WHERE attempts.plan = sessions.plan AND attempts.id = sessions.attempt)
  END
WHERE role IN ('manager', 'worker') OR (role = 'lead' AND forked_from IS NULL);
