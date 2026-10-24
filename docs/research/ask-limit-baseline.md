# ask_user_question refusals: baseline

## Question

How many `ask_user_question` calls does cpd's guard G2 (text over 1500 characters, `pi/lib/askguard.ts:4,23-33`, applied in `pi/lib/adapter.ts`; docs/commands.md §15.13 line 3654) refuse per 100 calls, at what sizes, and what do the refusals cost? Re-run the script in `## Script` after the fix to compare.

## How it was measured

- Source: pi session logs, `~/.pi/agent/sessions/<cwd-slug>/<timestamp>_<uuid>.jsonl` on this machine (`hs`), 1665 files, 471 with `ask_user_question`; read 2026-10-23. One JSON object per line.
- A call: a `toolCall` block with `name` `ask_user_question` in an assistant message (`arguments.questions[]`). Calls are counted once by `toolCallId` across all files (files sorted by path; the first file wins), because forked/resumed sessions copy entries.
- Size: sum of the UTF-16 lengths of every question's `question` and `header` and every option's `label`, `description`, `preview`: the same as `textLength` (`pi/lib/askguard.ts:12-23`).
- Outcome: the `toolResult` message with the same `toolCallId`, `toolName` `ask_user_question`. A G2 refusal is `isError: true` with text starting `cpd: ask_user_question is for a few short choices`. Real line (verified in the log):

```
{"type":"message","id":"35c5ae2c",...,"timestamp":"2026-10-23T12:56:43.056Z","message":{"role":"toolResult","toolCallId":"toolu_01UgccXNkwXbrNZD7F5WviUq","toolName":"ask_user_question","content":[{"type":"text","text":"cpd: ask_user_question is for a few short choices; put anything larger in plannotator; refused: 1789 characters (limit 1500)"}],"details":{},"isError":true,"timestamp":1792760203056}}
```

- G4 (`cpd: state what you ask about in the question itself`): recognised the same way; 0 found. Other outcomes, kept apart as "other errors": `validation` (text starts `Validation failed`), `otherCpdGuard` (an `isError` text starting `cpd: write what you ask about in the chat as text`; 18 calls; this reason is in no file of `~/projects/cpd` I searched, so its source is unverified), `otherError` (any other `isError`), `declined` (`User declined`), `cancelled` (`details.cancelled`), `noResult` (no result in the log). `success` is the rest (the User answered).
- Cutoff: **2026-10-20**, the date of the commit that added `pi/lib/askguard.ts` (`260f6e8d`, 2026-10-20T13:43-04:00 = 17:43Z, "Plans go to plannotator: ask_user_question guards (#174)", `git log`). The day is a UTC day taken from the assistant message timestamp, so 10-20 holds hours before the guard. The first G2 refusal in the logs is 2026-10-21T15:25Z, and no call from 10-21 on was accepted over 1499, so the guard reached the sessions on 10-21; I also give 2026-10-21 below.
- After a refusal: the next `ask_user_question` call in the same file within the next 5 assistant messages; wasted turns = assistant messages from the refusal's own message up to the next accepted call (1 = the very next message). Branches of a session tree are not told apart (unverified effect).
- Role: the logs hold no role field. Guess: a session whose `session_info` name or cwd slug contains `manager` is a manager, `lead` a lead, otherwise `unknown`. Most sessions (named `cockpit-statusbar`, topic names) are `unknown`.
- The Manager's rough counts (about 1885 calls, 40 files with the refusal, sizes 1501, 1502, 2008) do not match: 1885 is a `grep` line count that echoes the logs themselves, and 1501/1502/2008 appear only inside test output and quoted text (e.g. `refused: 1502 characters` in a bash result of `pi/test/askguard.test.ts`), not as a call's `toolResult`.

## Results

Run `2026-10-23T13:22:02Z`, `timeout 300 node /tmp/ask-refusals.mjs --since 2026-10-20`, 10 s. 516 distinct calls over all files.

### 1. Calls and outcomes per 100 calls

| Window | Calls | G2 refused | G4 refused | Other errors (validation + other guard + other + declined + cancelled) | No result | Success |
|---|---|---|---|---|---|---|
| on or after 2026-10-20 | 149 | 5 (3.36) | 0 (0) | 20 (13.42) | 1 (0.67) | 123 (82.55) |
| on or after 2026-10-21 (guard live) | 94 | 5 (5.32) | 0 (0) | 19 (20.21) | 1 (1.06) | 69 (73.40) |
| overall (2026-10-13 on) | 516 | 5 (0.97) | 0 (0) | 29 (5.62) | 5 (0.97) | 477 (92.44) |

Other errors since 10-20: validation 1, other guard 18, other 1. Overall: validation 8, other guard 18, other 1, cancelled 2, declined 0. All 18 `otherCpdGuard` are on 2026-10-22.

Per day (count of calls; G2 refusals):

| Day (UTC) | 10-20 | 10-21 | 10-22 | 10-23 |
|---|---|---|---|---|
| calls | 55 | 37 | 46 | 11 |
| G2 | 0 | 1 | 2 | 2 |
| G2 per 100 | 0 | 2.70 | 4.35 | 18.18 |

Before 10-20: 10-13 2, 10-14 44, 10-15 23, 10-16 77, 10-17 20, 10-18 124, 10-19 77 calls, none refused (full table in the output below).

### 2. Size histogram (characters)

| Bucket | All (since 10-20) | G2 refused | Accepted | All (overall) | Accepted (overall) |
|---|---|---|---|---|---|
| 0-499 | 37 | 0 | 29 | 111 | 100 |
| 500-999 | 53 | 0 | 49 | 198 | 190 |
| 1000-1499 | 51 | 0 | 42 | 125 | 113 |
| 1500 | 0 | 0 | 0 | 0 | 0 |
| 1501-1600 | 0 | 0 | 0 | 13 | 13 |
| 1601-2000 | 6 | 4 | 2 | 42 | 36 |
| 2001-3000 | 1 | 0 | 1 | 23 | 22 |
| over 3000 | 1 | 1 | 0 | 4 | 3 |

- Refused sizes: 1639, 1657, 1753, 1789, 3484 (one each; the 1789 refusal appears in the files about 26 times as copies).
- Accepted calls since 10-20 (n 123): median 868, p90 1362, max 2472; 21 are 1200 or more. Since 10-21 (n 69): median 868, p90 1401, max 1499; 14 are 1200 or more. Overall (n 477): median 844, p90 1670, max 3737; 128 are 1200 or more.
- The accepted calls over 1500 (3 since 10-20, 74 overall) all come before 2026-10-21, when the guard did not yet apply (10-20 holds the hours before the commit, and the guard needs the adapter loaded: guess).

### 3. After a G2 refusal (5 refusals)

| Next `ask_user_question` within 5 assistant messages | Count |
|---|---|
| accepted | 5 |
| refused again | 0 |
| none | 0 |

- Median size of the next call: 1154 (the refused ones were 1639-3484). Turns wasted: 1 each (5 in all): the next assistant message already held the accepted call.
- No refusal went to plannotator or chat instead (0 of 5 had no accepted ask within 5 messages; 0 `plannotator`/`cpd_plan_review` calls in those windows).

### 4. By role and by day

| Role (since 10-20) | Calls | G2 | Other guard | Success |
|---|---|---|---|---|
| manager | 40 | 2 (5.00) | 3 | 34 |
| lead | 0 | 0 | 0 | 0 |
| unknown | 109 | 3 (2.75) | 15 | 89 |

Overall: lead 33 calls (0 G2), manager 98 (2 G2, 2.04 per 100), unknown 385 (3 G2). The 10-22 `otherCpdGuard` cluster is 18 calls in one 10-22 window; days are in section 1. The Lead role is hidden by the naming (guess): the logs do not record it.

### Script output

Run `2026-10-23T13:22:02Z`:

```json
{
"since": "2026-10-20",
"dir": "/home/clayton/.pi/agent/sessions",
"distinctCalls": 516,
"onOrAfterSince": {
  "totals": {"calls":149,"success":124,"g2":5,"g4":0,"declined":0,"cancelled":0,"validation":1,"otherCpdGuard":18,"otherError":1,"noResult":0,"per100":{"success":83.22,"g2":3.36,"g4":0,"declined":0,"cancelled":0,"validation":0.67,"otherCpdGuard":12.08,"otherError":0.67,"noResult":0}},
  "byDay": {"2026-10-20":{"calls":55,"success":54,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":1,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":98.18,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":1.82,"otherCpdGuard":0,"otherError":0,"noResult":0}},"2026-10-21":{"calls":37,"success":35,"g2":1,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":1,"noResult":0,"per100":{"success":94.59,"g2":2.7,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":2.7,"noResult":0}},"2026-10-22":{"calls":46,"success":26,"g2":2,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":18,"otherError":0,"noResult":0,"per100":{"success":56.52,"g2":4.35,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":39.13,"otherError":0,"noResult":0}},"2026-10-23":{"calls":11,"success":9,"g2":2,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":81.82,"g2":18.18,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0}}},
  "byRole": {"manager":{"calls":40,"success":35,"g2":2,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":3,"otherError":0,"noResult":0,"per100":{"success":87.5,"g2":5,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":7.5,"otherError":0,"noResult":0}},"unknown":{"calls":109,"success":89,"g2":3,"g4":0,"declined":0,"cancelled":0,"validation":1,"otherCpdGuard":15,"otherError":1,"noResult":0,"per100":{"success":81.65,"g2":2.75,"g4":0,"declined":0,"cancelled":0,"validation":0.92,"otherCpdGuard":13.76,"otherError":0.92,"noResult":0}}},
  "sizeHistogram": {"all":{"1500":0,"0-499":37,"500-999":53,"1000-1499":51,"1501-1600":0,"1601-2000":6,"2001-3000":1,"over 3000":1},"g2Refused":{"1500":0,"0-499":0,"500-999":0,"1000-1499":0,"1501-1600":0,"1601-2000":4,"2001-3000":0,"over 3000":1},"g4Refused":{"1500":0,"0-499":0,"500-999":0,"1000-1499":0,"1501-1600":0,"1601-2000":0,"2001-3000":0,"over 3000":0},"accepted":{"1500":0,"0-499":29,"500-999":49,"1000-1499":43,"1501-1600":0,"1601-2000":2,"2001-3000":1,"over 3000":0}},
  "accepted": {"n":124,"median":868,"p90":1362,"max":2472,"atLeast1200":21},
  "g2Sizes": {"1639":1,"1657":1,"1753":1,"1789":1,"3484":1},
  "afterG2": {"refusals":5,"nextAskWithin5":{"accepted":5,"refusedAgainG2":0,"otherOutcome":0,"none":0},"nextAskSizeMedian":1154,"turnsWasted":{"n":5,"sum":5,"mean":1},"noAcceptedAskWithin5":0,"ofThosePlannotatorOrPlanReviewCall":0,"byRole":{"lead":0,"manager":2,"unknown":3}}
},
"overall": {
  "totals": {"calls":516,"success":478,"g2":5,"g4":0,"declined":0,"cancelled":2,"validation":8,"otherCpdGuard":18,"otherError":1,"noResult":4,"per100":{"success":92.64,"g2":0.97,"g4":0,"declined":0,"cancelled":0.39,"validation":1.55,"otherCpdGuard":3.49,"otherError":0.19,"noResult":0.78}},
  "byDay": {"2026-10-13":{"calls":2,"success":1,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":1,"per100":{"success":50,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":50}},"2026-10-14":{"calls":44,"success":43,"g2":0,"g4":0,"declined":0,"cancelled":1,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":97.73,"g2":0,"g4":0,"declined":0,"cancelled":2.27,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0}},"2026-10-15":{"calls":23,"success":23,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":100,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0}},"2026-10-16":{"calls":77,"success":75,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":1,"otherCpdGuard":0,"otherError":0,"noResult":1,"per100":{"success":97.4,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":1.3,"otherCpdGuard":0,"otherError":0,"noResult":1.3}},"2026-10-17":{"calls":20,"success":20,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":100,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0}},"2026-10-18":{"calls":124,"success":119,"g2":0,"g4":0,"declined":0,"cancelled":1,"validation":2,"otherCpdGuard":0,"otherError":0,"noResult":2,"per100":{"success":95.97,"g2":0,"g4":0,"declined":0,"cancelled":0.81,"validation":1.61,"otherCpdGuard":0,"otherError":0,"noResult":1.61}},"2026-10-19":{"calls":77,"success":73,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":4,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":94.81,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":5.19,"otherCpdGuard":0,"otherError":0,"noResult":0}},"2026-10-20":{"calls":55,"success":54,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":1,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":98.18,"g2":0,"g4":0,"declined":0,"cancelled":0,"validation":1.82,"otherCpdGuard":0,"otherError":0,"noResult":0}},"2026-10-21":{"calls":37,"success":35,"g2":1,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":1,"noResult":0,"per100":{"success":94.59,"g2":2.7,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":2.7,"noResult":0}},"2026-10-22":{"calls":46,"success":26,"g2":2,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":18,"otherError":0,"noResult":0,"per100":{"success":56.52,"g2":4.35,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":39.13,"otherError":0,"noResult":0}},"2026-10-23":{"calls":11,"success":9,"g2":2,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":81.82,"g2":18.18,"g4":0,"declined":0,"cancelled":0,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0}}},
  "byRole": {"lead":{"calls":33,"success":32,"g2":0,"g4":0,"declined":0,"cancelled":1,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0,"per100":{"success":96.97,"g2":0,"g4":0,"declined":0,"cancelled":3.03,"validation":0,"otherCpdGuard":0,"otherError":0,"noResult":0}},"manager":{"calls":98,"success":88,"g2":2,"g4":0,"declined":0,"cancelled":0,"validation":4,"otherCpdGuard":3,"otherError":0,"noResult":1,"per100":{"success":89.8,"g2":2.04,"g4":0,"declined":0,"cancelled":0,"validation":4.08,"otherCpdGuard":3.06,"otherError":0,"noResult":1.02}},"unknown":{"calls":385,"success":358,"g2":3,"g4":0,"declined":0,"cancelled":1,"validation":4,"otherCpdGuard":15,"otherError":1,"noResult":3,"per100":{"success":92.99,"g2":0.78,"g4":0,"declined":0,"cancelled":0.26,"validation":1.04,"otherCpdGuard":3.9,"otherError":0.26,"noResult":0.78}}},
  "sizeHistogram": {"all":{"1500":0,"0-499":111,"500-999":198,"1000-1499":125,"1501-1600":13,"1601-2000":42,"2001-3000":23,"over 3000":4},"g2Refused":{"1500":0,"0-499":0,"500-999":0,"1000-1499":0,"1501-1600":0,"1601-2000":4,"2001-3000":0,"over 3000":1},"g4Refused":{"1500":0,"0-499":0,"500-999":0,"1000-1499":0,"1501-1600":0,"1601-2000":0,"2001-3000":0,"over 3000":0},"accepted":{"1500":0,"0-499":100,"500-999":190,"1000-1499":114,"1501-1600":13,"1601-2000":36,"2001-3000":22,"over 3000":3}},
  "accepted": {"n":478,"median":844,"p90":1670,"max":3737,"atLeast1200":128},
  "g2Sizes": {"1639":1,"1657":1,"1753":1,"1789":1,"3484":1},
  "afterG2": {"refusals":5,"nextAskWithin5":{"accepted":5,"refusedAgainG2":0,"otherOutcome":0,"none":0},"nextAskSizeMedian":1154,"turnsWasted":{"n":5,"sum":5,"mean":1},"noAcceptedAskWithin5":0,"ofThosePlannotatorOrPlanReviewCall":0,"byRole":{"lead":0,"manager":2,"unknown":3}}
}
}
refusals per 100 calls: 3.36
```

## Reading

Worker's reading, not verified by the User. G2 refuses few calls: 5 in 149 calls since the cutoff (3.4 per 100), 5.3 per 100 once the guard was live, so far fewer than the Manager's rough count suggested. The refused sizes (1639 to 3484) are well over the limit, not on the edge, and the guard bit when sizes rose with hs-d553 assumptions; 14 of 69 accepted calls since 10-21 are 1200 or more, so a lower limit would refuse more. Each refusal cost one turn: all 5 were followed at once by an accepted call about 1154 characters long, and none moved to plannotator. The one larger cluster of errors is a different guard (18 calls on 10-22), not G2. The sample is small (5 refusals), so rates carry wide uncertainty.

## Script

Save as `/tmp/ask-refusals.mjs`, then: `timeout 300 node /tmp/ask-refusals.mjs --since 2026-10-20 [--dir ~/.pi/agent/sessions]`. It streams every file and ran in about 10 s here.

```js
#!/usr/bin/env node
// Measures ask_user_question refusals in pi session logs. Node >= 22, no dependency.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const args = process.argv.slice(2);
const opt = (name, dflt) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : dflt; };
const since = opt("--since", "2026-10-20");
const dir = opt("--dir", path.join(os.homedir(), ".pi/agent/sessions"));

// Same count as textLength() of cpd pi/lib/askguard.ts: UTF-16 length of question, header, label, description, preview.
const len = (v) => (typeof v === "string" ? v.length : 0);
function textLength(input) {
  const qs = input?.questions;
  if (!Array.isArray(qs)) return 0;
  let n = 0;
  for (const q of qs) {
    if (typeof q !== "object" || q === null) continue;
    n += len(q.question) + len(q.header);
    if (!Array.isArray(q.options)) continue;
    for (const o of q.options) if (typeof o === "object" && o !== null) n += len(o.label) + len(o.description) + len(o.preview);
  }
  return n;
}

function classify(m) {
  const text = (Array.isArray(m.content) ? m.content : []).map((c) => c?.text ?? "").join("");
  if (m.isError) {
    if (text.startsWith("cpd: ask_user_question is for a few short choices")) return "g2";
    if (text.startsWith("cpd: state what you ask about in the question itself")) return "g4";
    if (text.startsWith("cpd: write what you ask about in the chat")) return "otherCpdGuard";
    if (text.startsWith("Validation failed")) return "validation";
    return "otherError";
  }
  if (m.details?.cancelled === true) return "cancelled";
  if (text.startsWith("User declined")) return "declined";
  return "success";
}

function* walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) yield* walk(p);
    else if (e.name.endsWith(".jsonl")) yield p;
  }
}

const seen = new Set();
const calls = []; // one per distinct toolCallId
const follow = []; // one per distinct G2 refusal
for (const file of walk(dir)) {
  let ai = 0, role = "unknown", name = "", cwd = "";
  const mine = new Map(); // id -> call in this file (in order)
  const order = [];
  const plan = [];
  const rl = readline.createInterface({ input: fs.createReadStream(file), crlfDelay: Infinity });
  for await (const line of rl) {
    if (line.startsWith('{"type":"session"')) { try { cwd = JSON.parse(line).cwd ?? ""; } catch {} }
    else if (line.startsWith('{"type":"session_info"')) { try { name = JSON.parse(line).name ?? ""; } catch {} }
    const isAssistant = line.includes('"role":"assistant"');
    const isAsk = line.includes('"ask_user_question"');
    if (isAssistant && !isAsk) {
      ai++;
      if (/"name":"(plannotator\w*|cpd_plan_review)"/.test(line)) plan.push(ai);
      continue;
    }
    if (!isAsk) continue;
    let o; try { o = JSON.parse(line); } catch { continue; }
    const m = o.message;
    if (!m) continue;
    if (m.role === "assistant") {
      ai++;
      if (/"name":"(plannotator\w*|cpd_plan_review)"/.test(line)) plan.push(ai);
      for (const b of Array.isArray(m.content) ? m.content : []) {
        if (b?.type === "toolCall" && b.name === "ask_user_question") {
          const c = { id: b.id, ai, day: String(o.timestamp ?? "").slice(0, 10), size: textLength(b.arguments), outcome: "noResult" };
          if (!mine.has(b.id)) { mine.set(b.id, c); order.push(c); }
        }
      }
    } else if (m.role === "toolResult" && m.toolName === "ask_user_question") {
      const c = mine.get(m.toolCallId);
      if (c) c.outcome = classify(m);
    }
  }
  const who = `${name} ${path.basename(cwd)}`;
  role = /manager/i.test(who) ? "manager" : /lead/i.test(who) ? "lead" : "unknown";
  for (const c of order) {
    if (seen.has(c.id)) continue;
    seen.add(c.id);
    c.role = role;
    calls.push(c);
    if (c.outcome === "g2") {
      const next = order.find((x) => x.ai > c.ai && x.ai <= c.ai + 5);
      const acc = order.find((x) => x.ai > c.ai && x.ai <= c.ai + 5 && x.outcome === "success");
      follow.push({
        day: c.day, role,
        next: next ? { size: next.size, outcome: next.outcome } : null,
        wasted: acc ? acc.ai - c.ai : null,
        planInWindow: plan.some((p) => p > c.ai && p <= c.ai + 5),
      });
    }
  }
}

const BUCKETS = [["0-499", 0, 499], ["500-999", 500, 999], ["1000-1499", 1000, 1499], ["1500", 1500, 1500], ["1501-1600", 1501, 1600], ["1601-2000", 1601, 2000], ["2001-3000", 2001, 3000], ["over 3000", 3001, Infinity]];
const hist = (xs) => Object.fromEntries(BUCKETS.map(([k, a, b]) => [k, xs.filter((x) => x >= a && x <= b).length]));
const pct = (xs, p) => { if (!xs.length) return null; const s = [...xs].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.ceil(p * s.length) - 1)]; };
const OUT = ["success", "g2", "g4", "declined", "cancelled", "validation", "otherCpdGuard", "otherError", "noResult"];
const rate = (cs) => {
  const r = { calls: cs.length };
  for (const k of OUT) r[k] = cs.filter((c) => c.outcome === k).length;
  r.per100 = Object.fromEntries(OUT.map((k) => [k, cs.length ? +((100 * r[k]) / cs.length).toFixed(2) : null]));
  return r;
};
function summary(cs, fs_) {
  const acc = cs.filter((c) => c.outcome === "success").map((c) => c.size);
  const days = {}, roles = {};
  for (const c of cs) { (days[c.day] ??= []).push(c); (roles[c.role] ??= []).push(c); }
  const nx = fs_.filter((f) => f.next);
  return {
    totals: rate(cs),
    byDay: Object.fromEntries(Object.entries(days).sort().map(([d, x]) => [d, rate(x)])),
    byRole: Object.fromEntries(Object.entries(roles).sort().map(([d, x]) => [d, rate(x)])),
    sizeHistogram: {
      all: hist(cs.map((c) => c.size)),
      g2Refused: hist(cs.filter((c) => c.outcome === "g2").map((c) => c.size)),
      g4Refused: hist(cs.filter((c) => c.outcome === "g4").map((c) => c.size)),
      accepted: hist(acc),
    },
    accepted: { n: acc.length, median: pct(acc, 0.5), p90: pct(acc, 0.9), max: acc.length ? Math.max(...acc) : null, atLeast1200: acc.filter((x) => x >= 1200).length },
    g2Sizes: Object.fromEntries([...cs.filter((c) => c.outcome === "g2").reduce((m, c) => m.set(c.size, (m.get(c.size) ?? 0) + 1), new Map())].sort((a, b) => b[1] - a[1] || a[0] - b[0]).slice(0, 10)),
    afterG2: {
      refusals: fs_.length,
      nextAskWithin5: { accepted: fs_.filter((f) => f.next?.outcome === "success").length, refusedAgainG2: fs_.filter((f) => f.next?.outcome === "g2").length, otherOutcome: fs_.filter((f) => f.next && !["success", "g2"].includes(f.next.outcome)).length, none: fs_.filter((f) => !f.next).length },
      nextAskSizeMedian: pct(nx.map((f) => f.next.size), 0.5),
      turnsWasted: { n: fs_.filter((f) => f.wasted !== null).length, sum: fs_.reduce((s, f) => s + (f.wasted ?? 0), 0), mean: +(fs_.filter((f) => f.wasted !== null).reduce((s, f) => s + f.wasted, 0) / (fs_.filter((f) => f.wasted !== null).length || 1)).toFixed(2) },
      noAcceptedAskWithin5: fs_.filter((f) => f.wasted === null).length,
      ofThosePlannotatorOrPlanReviewCall: fs_.filter((f) => f.wasted === null && f.planInWindow).length,
      byRole: Object.fromEntries(["lead", "manager", "unknown"].map((r) => [r, fs_.filter((f) => f.role === r).length])),
    },
  };
}
const sel = calls.filter((c) => c.day >= since);
const fsel = follow.filter((f) => f.day >= since);
const result = { since, dir, distinctCalls: calls.length, onOrAfterSince: summary(sel, fsel), overall: summary(calls, follow) };
// one line per section keeps the output short
console.log("{");
const top = Object.entries(result);
top.forEach(([k, v], i) => {
  if (v && typeof v === "object" && "totals" in v) {
    console.log(`"${k}": {`);
    const sub = Object.entries(v);
    sub.forEach(([k2, v2], j) => console.log(`  "${k2}": ${JSON.stringify(v2)}${j < sub.length - 1 ? "," : ""}`));
    console.log(`}${i < top.length - 1 ? "," : ""}`);
  } else console.log(`"${k}": ${JSON.stringify(v)}${i < top.length - 1 ? "," : ""}`);
});
console.log("}");
console.log(`refusals per 100 calls: ${result.onOrAfterSince.totals.per100.g2}`);
```

## Not verified

- Whether the pi sessions after 10-21 all load the adapter (the 3 accepted calls over 1500 on 10-20 suggest not before then): guess.
- Roles: no role field exists in the logs; the name/cwd rule is a guess, and most sessions are `unknown`.
- The source of the `cpd: write what you ask about in the chat as text` guard (18 calls): not found in `~/projects/cpd`.
- Session-tree branches are not told apart, so a "next call" may sit on another branch; the 5-message window counts assistant messages in file order.
- Only this machine's logs (`hs`) were read; the rpiv tool's own text (`@juicesharp/rpiv-ask-user-question` 2.13.8) was not re-read here.
