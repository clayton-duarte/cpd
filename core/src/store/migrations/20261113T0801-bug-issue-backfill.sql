-- cpd-migration: breaking after 137
-- bug-issue backfill (pro-d587): the issue of the open bugs, split out of 20261113T0800-bug-issue because an UPDATE of existing rows is not additive (contract §4.5 A1). Idempotent.
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1156' WHERE id = 'b6' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI Settings rejects valid Tiers with ';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1157' WHERE id = 'b11' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI preview runs a 42-commit-old check';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1158' WHERE id = 'b12' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI shows cpd''s bug-protocol notice as';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1159' WHERE id = 'b13' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'run/webui-token disappears while a WebUI';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1160' WHERE id = 'b16' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'CPD.app still relaunches on cpd install ';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1161' WHERE id = 'b17' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Plan reaches done with announced follow-';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1162' WHERE id = 'b18' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI ports not served across machines: ';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1163' WHERE id = 'b19' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Prod 3333 serves the Electron renderer t';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1164' WHERE id = 'b21' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'mac-air stuck on cpd 0.24: every install';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1165' WHERE id = 'b25' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'mac-air: S1 Jobs accepted at registratio';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1166' WHERE id = 'b28' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI sends fail on mac-pro and homeserv';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1167' WHERE id = 'b29' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'PR #1085 merged unreviewed at 00:29 EST ';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1168' WHERE id = 'b32' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Open PR ready to post stuck as waiting i';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1169' WHERE id = 'b55' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Adapter model switch fails with bad_outp';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1170' WHERE id = 'b56' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI chat messages do not clear a tripp';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1171' WHERE id = 'b57' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'SWR GitHub pass stopped committing PR ro';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1172' WHERE id = 'b58' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Approval dialog has no answer UI in the ';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1173' WHERE id = 'b59' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI Esc does not abort the running age';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1175' WHERE id = 'b60' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'WebUI messages sent during a turn queue ';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1176' WHERE id = 'b61' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Engine raises ''checks failed'' for Danger';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1178' WHERE id = 'b62' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'cpd doctor reports tamper after a cpd jo';
UPDATE bug_records SET issue = 'https://github.com/clayton-duarte/cpd/issues/1181' WHERE id = 'b63' AND status = 'open' AND issue IS NULL AND substr(title,1,40) = 'Stop-for-replacement Attempt resets the ';
