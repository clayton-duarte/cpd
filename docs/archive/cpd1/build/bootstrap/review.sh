#!/usr/bin/env bash
# Bootstrap reviewer: fresh read-only S4 session. Usage: review.sh <job> <attempt-id>
set -euo pipefail
JOB=$1; ATT=$2
LEAD=~/projects/cpd.cpd-v0
WT=~/projects/cpd.cpd-v0--$ATT
R=/tmp/cpd-build/reviews
git -C "$LEAD" diff "cpd/v0...cpd/v0--$ATT" > "$R/$ATT.diff"
git -C "$LEAD" diff --stat "cpd/v0...cpd/v0--$ATT" > "$R/$ATT.diffstat"
set +e; (cd "$WT" && ./verify.sh) > "$R/$ATT.gate.txt" 2>&1; GATE=$?; set -e
echo "exit=$GATE" >> "$R/$ATT.gate.txt"
cat > "$R/$ATT.md" <<BRIEF
# Review brief: Attempt $ATT of Job $JOB

You are a **read-only Reviewer** in CPD. You cannot change files; you judge whether this Attempt meets its Spec.

Inputs (read all of them):
- Common rules for Workers: $WT/docs/build/specs/_common.md
- The Job Spec (newest version, on main): $HOME/projects/cpd/docs/build/specs/$JOB.md
- Reviewer feedback the Worker was given for this Attempt, if any: $HOME/projects/cpd/docs/build/feedback/$ATT.md
- Normative contract (newest version, on main): $HOME/projects/cpd/docs/cli-contract.md (only the sections the Spec names)
- The diff of this Attempt against the Lead Branch: $R/$ATT.diff (stat: $R/$ATT.diffstat)
- Gate output (./verify.sh, run by the Lead in the Attempt worktree; last line is the exit code): $R/$ATT.gate.txt
- The Worker's report: /tmp/cpd-build/reports/$ATT.md
- The worktree, to read full files: $WT

Rubric: approve only if
1. the Gate passed (exit=0);
2. every acceptance item of the Spec is met, with evidence (a test or a transcript in the report). Spot-check the most important tests by reading them;
3. only files the Spec lists were changed (plus new tests where the Spec allows);
4. the code follows the contract and the common rules (zod at boundaries, argv subprocesses, no silent fallbacks, JSON output and exit codes);
5. no obvious defects.

Reject with specific, actionable feedback that a fresh Worker can act on without seeing this conversation.

End your reply with exactly one fenced json block as the last thing:
\`\`\`json
{"verdict": "approve" | "reject", "feedback": "<specific, actionable>"}
\`\`\`
BRIEF
cd "$WT"
env -u HERDR_ENV -u HERDR_PANE_ID -u HERDR_SOCKET_PATH -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID -u HERDR_BIN_PATH \
  pi -p -ne --no-session --tools read,grep,find,ls --model github-copilot/claude-sonnet-5.5:xhigh \
  "Read the review brief at $R/$ATT.md and follow it exactly." > "$R/$ATT.out" 2>&1
python3 - "$R/$ATT.out" <<'PY'
import sys,re,json
t=open(sys.argv[1]).read()
blocks=re.findall(r"```json\s*(\{.*?\})\s*```", t, re.S)
if not blocks: print(json.dumps({"verdict":"invalid","feedback":"no verdict block"})); sys.exit(0)
print(blocks[-1])
PY
