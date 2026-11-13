-- cpd-migration: breaking after 137
-- bug-issue backfill (pro-d587): the issue of the open bugs, split out of 20261113T0800-bug-issue because an UPDATE of existing rows is not additive (contract §4.5 A1). Idempotent.
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
