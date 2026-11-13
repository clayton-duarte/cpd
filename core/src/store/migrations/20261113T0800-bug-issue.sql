-- cpd-migration: additive
-- bug-issue (pro-d587): `bug_records.issue`, the GitHub issue of a bug (https://github.com/<owner>/<repo>/issues/<N>),
-- null until the bug is mitigated; and `workflows.bug_id`, the bug a Workflow fixes, null for a Workflow of no bug.
-- The open bugs get their issue from the mapping below.
ALTER TABLE bug_records ADD COLUMN issue TEXT;
ALTER TABLE workflows ADD COLUMN bug_id TEXT REFERENCES bug_records(id);
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1156' WHERE id = 'b6' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1157' WHERE id = 'b11' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1158' WHERE id = 'b12' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1159' WHERE id = 'b13' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1160' WHERE id = 'b16' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1161' WHERE id = 'b17' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1162' WHERE id = 'b18' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1163' WHERE id = 'b19' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1164' WHERE id = 'b21' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1165' WHERE id = 'b25' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1166' WHERE id = 'b28' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1167' WHERE id = 'b29' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1168' WHERE id = 'b32' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1169' WHERE id = 'b55' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1170' WHERE id = 'b56' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1171' WHERE id = 'b57' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1172' WHERE id = 'b58' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1173' WHERE id = 'b59' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1175' WHERE id = 'b60' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1176' WHERE id = 'b61' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1178' WHERE id = 'b62' AND status = 'open' AND issue IS NULL;
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1181' WHERE id = 'b63' AND status = 'open' AND issue IS NULL;
