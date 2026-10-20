# Planning UX usage: plannotator reviews and ask_user_question rounds, measured

Evidence file for Plan `plan-ux` (Job `usage`, Attempt `usage-a1`). It measures how plannotator Plan reviews and `ask_user_question` rounds went in Context `building-cpd`. It states measurements and what they were read from; it makes no recommendation. Anything not checked is labelled "unverified".

Snapshot: everything up to **2026-10-20T20:08:20Z**. The sessions and events are live, so the generator ignores anything later (`CUTOFF` in the script); two runs of it gave byte-identical output (`diff -r` of two output directories printed nothing).

## 0. Scope and caveats (read first)

- **The stores.** The brief points at `~/.plannotator`. That store holds only 9 feedback entries (5 plan approvals of 2026-10-13..15, 4 annotate approvals; one of the annotate approvals is the workflow design of 2026-10-20 at 19:57:10Z, section 6.1) and its `history/`, `plans/`, `drafts/`. The cpd 2 Plan reviews live in **one plannotator store per Plan**: `~/.cpd/run/<plan>/plannotator/{feedback,history,plans,drafts}` (31 of them; `find ~/.cpd/run -path '*plannotator*' -name index.jsonl`). Both were read; between them 93 plannotator decisions exist and only **one** carries User-typed annotations (3 annotations, `tree-menu` `dots.md`, 2026-10-16T18:36Z; section 5, E8).
- **The rendered plan.** Each review's markdown is `~/.cpd/run/<plan>/plan-r<N>.md` (`cpd plan render`). It is the same text plannotator stored as `history/.../NNN.md`: for `plan-ux` r1, `plan-r1.md` is 18372 characters (`wc -m`) and `history/clayton/research-planning-ux-tools-evi-2026-10-20/001.md` is 18382 bytes (`wc -c`), and `cross-repo` r4's `plan-r4.md` is 928415 bytes, the size of `plan-cross-repo-2026-10-19/002.md`. "Plan chars" below is `wc -m` of `plan-r<N>.md`. It is the whole Plan: Locked (already merged or running) Jobs are rendered as context next to the draft Jobs under review (the file says so: "the Jobs marked Locked are shown as context and are not for approval").
- **Event count.** `cpd events list --context building-cpd --kind plan-review --limit 1000` returns **160** events (82 `pending`, 76 `approved`, 2 `rejected`; span 2026-10-15..2026-10-20). The brief says 158 on 2026-10-20; the `plan-ux` r1 pair (20:06:59Z, 20:07:50Z) accounts for the 2 extra (arithmetic only; unverified that the brief's count excluded exactly those). No event has status `gone`.
- **Reviews of other Contexts.** `~/.cpd/run/*/plan-r*.md` has 88 files; 6 belong to Plans of other Contexts (`b2-budget`, `bacon`, `drop-hermes`, `mac-pro-debug` ×2, `speedtest`) and are excluded: they have no event in `building-cpd`. All 82 requests of `building-cpd` are in the table of section 2.
- **Decision time.** Request time = the `pending` event's `at`; decision time = the decision event's `at`. For 75 of the 77 User decisions this agrees within 1 s with the matching entry of the Plan's plannotator `feedback/*/index.jsonl` (generator check; the 2 others are the exceptions below). The exceptions: `cockpit-statusbar` r1 is recorded at 17:04:29Z by the event but 17:05:53.364Z by plannotator and 17:05:53.393Z by the Lead's `cpd-plan-decision` message (84 s later; cause unverified), and `dev-speed` r3 has no plannotator decision at all (section 6.4). The table uses the event time.
- **Ask latency.** `durationMs` of the `ask_user_question` tool result (from the call to the answer). It includes time before the User saw the dialog. It does not say how long the User read or thought.
- **Who wrote a decision text.** The events and the plannotator index do not record an actor (`origin: "pi"` only). Where I name an author I give the evidence in place. `ask_user_question` answers are typed by whoever answers the dialog; I take them as the User's (unverified that no agent ever answered one).
- **What the data cannot show.** Whether the User read a plan before approving it, and what they would have changed. Decision time and size are reported side by side without inference.
- **Forked sessions.** A forked Lead's file starts with its parent's history. Rounds are deduplicated by `toolCall` id, kept in the earliest session file that contains them, so no round is counted twice.

## 1. Method

All commands are read-only. Nothing under `~/.pi`, `~/.plannotator` or `~/.cpd` was written. Working files went to `/tmp/ua1` (removed at the end).

**Inputs, with the commands that produce them**

```sh
# plan-review events (160), request/decision pairs
cpd events list --context building-cpd --kind plan-review --limit 1000 > pr.json
jq '.events|length' pr.json                                   # 160
jq -r '.events[].data.status' pr.json | sort | uniq -c        # 82 pending, 76 approved, 2 rejected

# pair each request with its decision by (plan, review): one TSV row per review
jq -r '.events|sort_by(.id)|group_by(.plan+"/"+.data.review)|.[]
  |{p:.[0].plan,r:.[0].data.review,req:(map(select(.data.status=="pending"))[0]),dec:(map(select(.data.status!="pending"))[0])}
  |[.p,.r,.req.id,.req.at,(.dec.id//""),(.dec.at//""),(.dec.data.status//"undecided"),((.dec.data.feedback//"")|length)]|@tsv' pr.json

# the rendered plan of a review, and its size in characters
wc -m ~/.cpd/run/<plan>/plan-<review>.md

# plannotator's own stores: every decision, with annotation counts and feedback
cat ~/.plannotator/feedback/*/index.jsonl ~/.cpd/run/*/plannotator/feedback/*/index.jsonl | jq -c '{ts,surface,decision,ann:.counts.annotations,fb:((.feedback//"")|length)}'

# sessions (Manager, Leads), with their pi JSONL files
cpd session list --context building-cpd | jq -r '.sessions[]|select(.role!="worker")|[.id,.role,.plan,.file]|@tsv'

# ask_user_question: the calls, with timestamps and arguments ...
jq -c 'select(.type=="message" and .message.role=="assistant")|.timestamp as $t|.message.content[]?
  |select(.type=="toolCall" and .name=="ask_user_question")|{t:$t,id,arguments}' "$FILE"
# ... and the results (answers, kinds option|multi|custom, cancelled, isError, durationMs)
jq -c 'select(.type=="message" and .message.role=="toolResult" and .message.toolName=="ask_user_question")
  |.message|{toolCallId,isError,durationMs,cancelled:.details.cancelled,answers:.details.answers}' "$FILE"

# plan-review calls and the decision messages a Lead received
jq -r -f pr.jq "$FILE"     # pr.jq: select toolCall/toolResult named cpd_plan_review or plannotator
jq -r 'select(.type=="custom_message" and .customType=="cpd-plan-decision")|"\(.timestamp) \(.content)"' "$FILE"

# who submitted a decision to plannotator on the User's behalf
grep -n 'api/approve\|api/deny' "$FILE"

# askguard: refusals, i.e. results of ask_user_question that are neither answers nor schema errors
jq -r 'select(.type=="message" and .message.role=="toolResult" and .message.toolName=="ask_user_question")
  |.message.content[0].text|select(startswith("User has answered")|not)' "$FILE" | sort | uniq -c

# later cancellations of reviewed Jobs, and Context records
cpd events list --context building-cpd --kind job-cancelled --limit 5000
cpd context show --record <id>
```

`pr.jq` is: `select(.type=="message")|.timestamp as $t|.message|if .role=="assistant" then (.content[]?|select(.type=="toolCall" and (.name|test("plan_review|plannotator")))|"\($t) CALL \(.name) \(.arguments|tostring|.[0:300])") elif .role=="toolResult" and (.toolName|test("plan_review|plannotator")) then "\($t) RES \(.toolName) err=\(.isError) \(.content[0].text[0:400])" else empty end`.

**Definitions used in the tables**

- *Review* = one `(plan, review id)`. *n in plan* = its position among that Plan's reviews. *Prior rejections* = User rejections of the same Plan before it. A review is "approved first time" when no earlier review of the same Plan was rejected by the User.
- *Decided* statuses: `approved`; `rejected` (a User rejection); `closed by Lead` (the event says `rejected` but its feedback says the Lead closed the review as superseded; no User decision exists in plannotator's store, section 6.4); `undecided` (a `pending` event and no decision event).
- *Draft Jobs* = the review's "reviews N draft Jobs" text. *Locked Jobs shown* = rows ending `| Locked |` in the rendered plan. *Reviewed Jobs later cancelled* = of the draft Job keys parsed from the rendered plan (rows with a `S|O|H` Tier and status `draft`, or marked `**under review**`), those with a `job-cancelled` event for the same Plan after the decision. This is the only mechanical measure of "did the outcome hold": it counts Jobs withdrawn after approval, whatever the reason (the reasons are in the events' `data.reason`).
- *Ask round* = one `ask_user_question` call. *chars* = characters of all question texts, headers, option labels, option descriptions and option previews the call carried. *Picked first / recommended* = for single-select questions answered with an option: the answer is the first option / its label contains `(Recommended)`. *free text* = answers of kind `custom` (the User typed instead of choosing). *multi* = kind `multi` (multi-select). *Latency* = the result's `durationMs`.
- Scope of the ask table: the Manager session `97de2471` and the 5 Lead sessions with the most rounds (`switch-over` `2e66cebd`, `tui-visuals` `4965bfc8`, `cross-repo` `04598f9d`, `dev-speed` `353dd1a1`, Plan `manager`'s Lead `46b9faa6`). Aggregates are given for that table, for the Manager alone, and for all 28 sessions with at least one round (`asks_all_table.md`, regenerated by the script, has every one of the 313 rounds).

**Reproducing the tables.** Save the script below as `gen.py` and run `timeout 900 python3 gen.py OUT`. It needs `cpd` on `PATH` and python 3 (standard library only). It writes `reviews.md` (section 2), `asks_table.md` (section 3), `review_aggregates.md` and `ask_aggregates.md` (section 4), `invalid.md` and `post_askguard.md` (section 6), `asks_all_table.md` and `asks.json`. Sections 2 to 4 below are those files, pasted unchanged. Sections 5 and 6 were read by hand from the sessions and records named in place.

```python
#!/usr/bin/env python3
"""Regenerates the tables of docs/research/planning-ux-usage.md. Read-only on cpd, ~/.cpd and ~/.pi.
Usage: python3 gen.py OUTDIR   (needs cpd, jq-free; reads cpd via its CLI)"""
import json, os, re, subprocess, sys, statistics as st, datetime as dt, glob

OUT = sys.argv[1]; os.makedirs(OUT, exist_ok=True)
CUTOFF = '2026-10-20T20:08:20Z'  # snapshot: the sessions and events are live; nothing after this instant is counted
RUN = os.path.expanduser('~/.cpd/run')
def cpd(*a):
    return json.loads(subprocess.run(['timeout','120','cpd',*a],check=True,capture_output=True,text=True).stdout)
def ts(s): return dt.datetime.fromisoformat(s.replace('Z','+00:00'))
def med(x): return st.median(x) if x else None
def rng(x, f='{:.1f}'): return f"median {f.format(med(x))}, range {f.format(min(x))}–{f.format(max(x))}, n={len(x)}" if x else 'n=0'

# ---------- plan reviews ----------
ev = cpd('events','list','--context','building-cpd','--kind','plan-review','--limit','1000')['events']
ev = [e for e in ev if e['at'] <= CUTOFF]
cancels = cpd('events','list','--context','building-cpd','--kind','job-cancelled','--limit','5000')['events']
idx = []
for p in glob.glob(f'{RUN}/*/plannotator/feedback/*/index.jsonl'):
    d = p[len(RUN)+1:].split('/')[0]
    idx += [dict(json.loads(l), d=d) for l in open(p)]
byplan = {}
for e in sorted(ev, key=lambda e: e['id']):
    byplan.setdefault((e['plan'], e['data']['review']), []).append(e)
GIST = {  # hand-read gists of every non-empty feedback text (the text itself is in the event's data.feedback)
 ('cockpit-statusbar','r1'): 'rejected only to raise 3 Tiers (docs H2→S2, 2×S1→S3) and add one Background sentence; "everything else is approved"; text cites code names it checked',
 ('cockpit-statusbar','r2'): 'approved: Tiers and sentence as asked; "Lock and dispatch."',
 ('cockpit-parity','r1'):   'approved: gate open (cpd 1 landed six Jobs; cargo 702 passed); "Lock and dispatch."',
 ('dev-speed','r3'):        'Lead closed r3 as superseded by r4 (approved 14:10Z); "kept the Lead blocked"',
}
rows = []
for (plan, rv), es in byplan.items():
    req = next(e for e in es if e['data']['status']=='pending')
    dec = next((e for e in es if e['data']['status']!='pending'), None)
    path = f'{RUN}/{plan}/plan-{rv}.md'
    txt = open(path, encoding='utf-8').read()
    cited = sorted(set(re.findall(r'hs-[dn]\d+', txt)))
    under, locked = txt.count('| **under review** |'), txt.count('| Locked |')
    m = re.search(r'reviews (\d+) draft Jobs', req['text'])
    status = dec['data']['status'] if dec else 'undecided'
    fb = (dec['data'].get('feedback') or '') if dec else ''
    mins = (ts(dec['at'])-ts(req['at'])).total_seconds()/60 if dec else None
    jobkeys = []
    for ln in txt.split('\n'):
        c = [x.strip() for x in ln.strip().strip('|').split('|')]
        if ln.startswith('| `') and len(c) in (6, 7) and re.fullmatch(r'[SOH]\d', c[2]) and (c[3]=='draft' or (len(c)==7 and 'under review' in c[6])):
            if len(c)==6 or 'under review' in c[6]: jobkeys.append(c[0].strip('`'))
    later = [(j['job'], (j['data'].get('reason') or '')[:90]) for j in cancels if dec and j['plan']==plan and j['job'] in jobkeys and j['at']>dec['at']]
    rows.append(dict(plan=plan, rv=rv, req_id=req['id'], req=req['at'], dec_id=dec['id'] if dec else None,
        dec=dec['at'] if dec else None, mins=mins, status=status, fb=len(fb), chars=len(txt), lines=txt.count('\n')+1,
        jobs=int(m.group(1)) if m else None, cited=len(cited), nkeys=len(jobkeys), under=under, locked=locked, later=later))
rows.sort(key=lambda r: r['req'])
# classify: a Lead-closed rejection is not a User decision
def kind(r):
    if r['status']=='rejected' and r['plan']=='dev-speed' and r['rv']=='r3': return 'closed by Lead'
    if r['status']=='rejected': return 'rejected'
    return r['status']
for r in rows: r['kind'] = kind(r)
# per-plan sequence and prior rejections
seen = {}
for r in rows:
    s = seen.setdefault(r['plan'], dict(n=0, rej=0))
    s['n'] += 1; r['n'] = s['n']; r['rej_before'] = s['rej']
    if r['kind']=='rejected': s['rej'] += 1
L = ['| plan | review | requested (UTC) | decided (UTC) | min | status | n in plan | prior rejections | draft Jobs | locked Jobs shown | plan chars | lines | feedback chars | reviewed Jobs later cancelled | gist |','|---|---|---|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---|---|']
for r in rows:
    L.append('| {plan} | {rv} | {rq} | {dc} | {mn} | {st} | {n} | {rj} | {jb} | {lk} | {ch} | {ln} | {fb} | {ci} | {gi} |'.format(
        plan=r['plan'], rv=r['rv'], rq=r['req'][5:16].replace('T',' '), dc=(r['dec'][5:16].replace('T',' ') if r['dec'] else '–'),
        mn=('%.1f'%r['mins'] if r['mins'] is not None else '–'), st=r['kind'], n=r['n'], rj=r['rej_before'],
        jb=r['jobs'] if r['jobs'] is not None else '?', lk=r['locked'], ch=r['chars'], ln=r['lines'], fb=r['fb'] or 0,
        ci=(f"{len(r['later'])} of {r['nkeys']}" if r['kind']=='approved' else '–'), gi=GIST.get((r['plan'],r['rv']), '–')))
open(f'{OUT}/reviews.md','w').write('\n'.join(L)+'\n')

dec_user = [r for r in rows if r['kind'] in ('approved','rejected')]
appr = [r for r in rows if r['kind']=='approved']
A = []
A.append(f"- Plan-review requests: {len(rows)} ({len(ev)} events: {sum(e['data']['status']=='pending' for e in ev)} pending + {sum(e['data']['status']=='approved' for e in ev)} approved + {sum(e['data']['status']=='rejected' for e in ev)} rejected)")
A.append(f"- Outcomes: approved {len(appr)}, rejected by the User {sum(r['kind']=='rejected' for r in rows)}, closed by the Lead {sum(r['kind']=='closed by Lead' for r in rows)}, undecided (no decision event) {sum(r['kind']=='undecided' for r in rows)}, `gone` {sum(r['status']=='gone' for r in rows)}")
A.append(f"- Decision time, minutes, approved reviews: {rng([r['mins'] for r in appr])}")
A.append(f"- Decision time, minutes, all User decisions (approved + rejected): {rng([r['mins'] for r in dec_user])}")
for lim in (1,5,30,60): A.append(f"  - approved within {lim} min: {sum(r['mins']<=lim for r in appr)} of {len(appr)}")
A.append(f"- Plan size shown, characters, all {len(rows)} reviews: {rng([r['chars'] for r in rows],'{:.0f}')}")
A.append(f"- Plan size shown, lines: {rng([r['lines'] for r in rows],'{:.0f}')}")
A.append(f"- Draft Jobs per review: {rng([r['jobs'] for r in rows if r['jobs'] is not None],'{:.0f}')}")
A.append(f"- Reviews whose decision text is non-empty: {sum(r['fb']>0 for r in rows)} of {len(rows)} (chars: {[r['fb'] for r in rows if r['fb']]})")
firsts = [r for r in rows if r['n']==1]
A.append(f"- Plans: {len(firsts)}; plans whose first review was approved: {sum(r['kind']=='approved' for r in firsts)}; plans with a User rejection: {len({r['plan'] for r in rows if r['kind']=='rejected'})}")
A.append(f"- Approved reviews approved with no earlier rejection of the same Plan: {sum(r['rej_before']==0 for r in appr)} of {len(appr)} ({100*sum(r['rej_before']==0 for r in appr)/len(appr):.0f}%)")
A.append(f"- Draft Jobs parsed from the approved reviews' rendered plans: {sum(r['nkeys'] for r in appr)}; of them later cancelled (a `job-cancelled` event for the same Plan and Job key after the decision): {sum(len(r['later']) for r in appr)} ({100*sum(len(r['later']) for r in appr)/max(1,sum(r['nkeys'] for r in appr)):.0f}%); approved reviews with at least one such Job: {sum(bool(r['later']) for r in appr)} of {len(appr)}")
A.append(f"- Reviews showing Locked Jobs as context: {sum(r['locked']>0 for r in rows)} of {len(rows)}; for those, Locked Jobs shown per draft Job: {rng([r['locked']/r['jobs'] for r in rows if r['locked']>0 and r['jobs']],'{:.1f}')}")
A.append(f"- Plan size, characters, reviews that show Locked Jobs ({sum(r['locked']>0 for r in rows)}): {rng([r['chars'] for r in rows if r['locked']>0],'{:.0f}')}; reviews that show none ({sum(r['locked']==0 for r in rows)}): {rng([r['chars'] for r in rows if r['locked']==0],'{:.0f}')}")
big = sorted(rows, key=lambda r: -r['chars'])[:5]
A.append('- Largest plans shown: ' + ', '.join(f"{r['plan']}/{r['rv']} {r['chars']} chars ({r['kind']}, {('%.1f'%r['mins']) if r['mins'] is not None else '–'} min)" for r in big))
# size vs time
sm = [r for r in appr if r['chars']<=med([x['chars'] for x in appr])]; lg = [r for r in appr if r['chars']>med([x['chars'] for x in appr])]
A.append(f"- Approved reviews at or below the median size ({med([x['chars'] for x in appr]):.0f} chars): decision minutes {rng([r['mins'] for r in sm])}; above it: {rng([r['mins'] for r in lg])}")
# plannotator store agreement
def near(r):
    c = [i for i in idx if i['d']==r['plan'] and r['dec']]
    return min((abs((ts(i['ts'])-ts(r['dec'])).total_seconds()) for i in c), default=None)
ag = [near(r) for r in rows if r['kind'] in ('approved','rejected')]
A.append(f"- Decisions whose time agrees within 1 s with a plannotator `feedback/*/index.jsonl` entry of the same Plan's run dir: {sum(x is not None and x<=1 for x in ag)} of {len(ag)}")
open(f'{OUT}/review_aggregates.md','w').write('\n'.join(A)+'\n')

# ---------- ask_user_question ----------
sess = cpd('session','list','--context','building-cpd')['sessions']
S = [s for s in sess if s['role']!='worker']
S.sort(key=lambda s: os.path.basename(s['file']))
calls, results = {}, {}
owner = {}
for s in S:
    for l in open(s['file'], encoding='utf-8'):
        if 'ask_user_question' not in l: continue
        o = json.loads(l)
        if o.get('type')!='message': continue
        m = o['message']
        if m['role']=='assistant':
            for c in m['content']:
                if c.get('type')=='toolCall' and c['name']=='ask_user_question' and c['id'] not in calls and o['timestamp'] <= CUTOFF:
                    calls[c['id']] = (o['timestamp'], c['arguments']); owner[c['id']] = s
        elif m['role']=='toolResult' and m.get('toolName')=='ask_user_question':
            results.setdefault(m['toolCallId'], (o['timestamp'], m))
def label(s): return (s['plan'] or s['role']) + ' ' + s['id'][:8]
rounds, invalid = [], []
for cid, (t, a) in calls.items():
    s = owner[cid]; q = a.get('questions') or []
    res = results.get(cid)
    row = dict(sid=s['id'][:8], who=('Manager' if s['role']=='manager' else (s['plan'] or s['role'])+(' (Plan Lead)' if s['plan']=='manager' else '')), role=s['role'], at=t, nq=len(q), nopt=[len(x.get('options',[])) for x in q],
               chars=sum(len(x.get('question',''))+len(x.get('header',''))+sum(len(o.get('label',''))+len(o.get('description',''))+len(o.get('preview','') or '') for o in x.get('options',[])) for x in q),
               multi=sum(bool(x.get('multiSelect')) for x in q), rec_first=sum(bool(x.get('options')) and '(Recommended)' in x['options'][0].get('label','') for x in q),
               rec_any=sum(any('(Recommended)' in o.get('label','') for o in x.get('options',[])) for x in q))
    if not res: row.update(status='no result'); rounds.append(row); continue
    rt, m = res
    if m.get('isError'): row.update(status='invalid'); invalid.append((row, m['content'][0]['text'].split('\n')[1].strip())); continue
    d = m['details']; an = d['answers']
    row['cancelled'] = bool(d.get('cancelled')); row['lat'] = m.get('durationMs',0)/60000
    kinds = {'option':0,'multi':0,'custom':0}; first = rec = nonfirst = 0; custom_chars = []; rq = rq_rec = rq_other = rq_free = 0
    for ans in an:
        k = ans['kind']; kinds[k] += 1
        qq = q[ans['questionIndex']]; labels = [o['label'] for o in qq.get('options',[])]
        if any('(Recommended)' in x for x in labels):
            rq += 1
            if k=='option' and '(Recommended)' in ans['answer']: rq_rec += 1
            elif k=='option': rq_other += 1
            else: rq_free += 1
        if k=='option':
            if ans['answer'] == labels[0]: first += 1
            else: nonfirst += 1
            if '(Recommended)' in ans['answer']: rec += 1
        elif k=='multi': nonfirst += 0
        else: custom_chars.append(len(ans['answer']))
    row.update(status='cancelled' if row['cancelled'] else 'answered', answered=len(an), kinds=kinds, first=first, rec=rec, nonfirst=nonfirst,
               custom=custom_chars, opt_answers=kinds['option'], rq=rq, rq_rec=rq_rec, rq_other=rq_other, rq_free=rq_free)
    rounds.append(row)
def tbl(rs):
    L = ['| session | at (UTC) | Qs | options per Q | chars | multi Qs | answered | picked first / recommended (of option answers) | free-text answers (chars) | multi answers | cancelled | latency min |','|---|---|---:|---|---:|---:|---:|---|---|---:|---|---:|']
    for r in sorted(rs, key=lambda r:(r['who'], r['at'])):
        if r['status'] not in ('answered','cancelled'): continue
        L.append('| {w} | {at} | {nq} | {no} | {ch} | {mu} | {an} | {f}/{rc} of {oa} | {cu} | {mk} | {ca} | {la:.1f} |'.format(
            w=r['who']+' '+r['sid'], at=r['at'][5:16].replace('T',' '), nq=r['nq'], no='/'.join(map(str,r['nopt'])), ch=r['chars'], mu=r['multi'],
            an=r['answered'], f=r['first'], rc=r['rec'], oa=r['opt_answers'], cu=(','.join(map(str,r['custom'])) or '–'), mk=r['kinds']['multi'],
            ca='yes' if r['cancelled'] else '–', la=r['lat']))
    return '\n'.join(L)+'\n'
TABLE_SESS = {'97de2471'} | {r['sid'] for r in rounds if r['who'] in ('cross-repo','tui-visuals','pi-pin','quality','tui-dots') and False}
cnt = {}
for r in rounds:
    if r['status'] in ('answered','cancelled'): cnt[(r['who'],r['sid'])] = cnt.get((r['who'],r['sid']),0)+1
top5 = [k for k,_ in sorted(((k,v) for k,v in cnt.items() if k[0]!='manager' and k[0]!=''), key=lambda kv:-kv[1])[:5]] if False else None
lead_sorted = sorted(((v,k) for k,v in cnt.items() if k[1]!='97de2471'), reverse=True)[:5]
TABLE_SIDS = {'97de2471'} | {k[1] for v,k in lead_sorted}
open(f'{OUT}/asks_table.md','w').write(tbl([r for r in rounds if r['sid'] in TABLE_SIDS]))
open(f'{OUT}/asks_all_table.md','w').write(tbl(rounds))
def agg(rs, name):
    sids = {r['sid'] for r in rs}
    ninv = sum(1 for r,_ in invalid if r['sid'] in sids)
    ok = [r for r in rs if r['status'] in ('answered','cancelled')]
    qa = sum(r['answered'] for r in ok); oa = sum(r['opt_answers'] for r in ok)
    multi = sum(r['kinds']['multi'] for r in ok); cust = sum(r['kinds']['custom'] for r in ok)
    first = sum(r['first'] for r in ok); rec = sum(r['rec'] for r in ok); nf = sum(r['nonfirst'] for r in ok)
    rec_qs = sum(r['rec_first'] for r in ok)
    nq = sum(r['nq'] for r in ok)
    deviate = sum(1 for r in ok if r['nonfirst'] or r['kinds']['custom'] or r['kinds']['multi'])
    typed = sum(1 for r in ok if r['kinds']['custom'])
    cc = [c for r in ok for c in r['custom']]
    o = [f"**{name}**", f"- Rounds with a User result: {len(ok)} (+ {ninv} refused by schema validation, {sum(r['status']=='no result' for r in rs)} with no result)",
      f"- Answer latency, minutes: {rng([r['lat'] for r in ok])}",
      f"- Answer latency buckets: ≤1 min {sum(r['lat']<=1 for r in ok)}, ≤5 min {sum(r['lat']<=5 for r in ok)}, >60 min {sum(r['lat']>60 for r in ok)} (of {len(ok)})",
      f"- Rounds showing over 1000 / 2000 / 3000 characters: {sum(r['chars']>1000 for r in ok)} / {sum(r['chars']>2000 for r in ok)} / {sum(r['chars']>3000 for r in ok)}",
      f"- Characters shown per round (questions, headers, labels, descriptions, previews): {rng([r['chars'] for r in ok],'{:.0f}')}",
      f"- Questions per round: {rng([r['nq'] for r in ok],'{:.0f}')}; rounds with 4 questions: {sum(r['nq']==4 for r in ok)}",
      f"- Options per question: {rng([n for r in ok for n in r['nopt']],'{:.0f}')}",
      f"- Questions answered: {qa} (option {oa}, multi-select {multi}, free text {cust})",
      f"- Questions whose first option carries '(Recommended)': {rec_qs} of {nq}",
      f"- Questions that offered a '(Recommended)' option: {sum(r['rq'] for r in ok)}; the User picked it {sum(r['rq_rec'] for r in ok)} ({100*sum(r['rq_rec'] for r in ok)/sum(r['rq'] for r in ok):.0f}%), another option {sum(r['rq_other'] for r in ok)} ({100*sum(r['rq_other'] for r in ok)/sum(r['rq'] for r in ok):.0f}%), free text or multi-select {sum(r['rq_free'] for r in ok)} ({100*sum(r['rq_free'] for r in ok)/sum(r['rq'] for r in ok):.0f}%)",
      f"- Of {oa} single-option answers: first option {first} ({100*first/oa:.0f}%), a later option {nf} ({100*nf/oa:.0f}%); one with '(Recommended)' in its label {rec} ({100*rec/oa:.0f}%)",
      f"- Rounds where the User typed free text: {typed} of {len(ok)} ({100*typed/len(ok):.0f}%); free-text answer chars: {rng(cc,'{:.0f}')}",
      f"- Rounds with a non-first option, a multi-select or free text: {deviate} of {len(ok)} ({100*deviate/len(ok):.0f}%)",
      f"- Cancelled (the User declined): {sum(r['status']=='cancelled' for r in ok)}"]
    return '\n'.join(o)+'\n'
open(f'{OUT}/ask_aggregates.md','w').write(agg([r for r in rounds if r['sid'] in TABLE_SIDS], f"The {len(TABLE_SIDS)} sessions in the table ({', '.join(sorted({r['who']+' '+r['sid'] for r in rounds if r['sid'] in TABLE_SIDS}))})")+'\n'
    + agg([r for r in rounds if r['sid']=='97de2471'], 'The Manager session 97de2471 alone')+'\n'
    + agg(rounds, f"All {len({r['sid'] for r in rounds})} non-Worker sessions with a round (deduplicated by tool-call id)")+'\n')
json.dump(dict(rounds=rounds, invalid=[(r, t) for r,t in invalid]), open(f'{OUT}/asks.json','w'), indent=1)
inv = ['| session | at (UTC) | refusal |','|---|---|---|'] + [f"| {r['who']} {r['sid']} | {r['at'][5:16].replace('T',' ')} | {t} |" for r,t in sorted(invalid, key=lambda x:x[0]['at'])]
open(f'{OUT}/invalid.md','w').write('\n'.join(inv)+'\n')
post = [r for r in rounds if r['at'] > '2026-10-20T17:43:29' and r['status'] in ('answered','cancelled')]
open(f'{OUT}/post_askguard.md','w').write(f"Rounds with a User result after PR askguard (#174) merged at 2026-10-20T17:43:29Z: {len(post)}; chars {sorted(r['chars'] for r in post)}; refused: 0 (no non-answer result)\n")
print('rows',len(rows),'rounds',len(rounds),'sessions',len({r['sid'] for r in rounds}))
```

## 2. Plan reviews (all 82 requests)

Columns: *requested* and *decided* are `MM-DD HH:MM` UTC (2026-10-15 to 2026-10-20); *min* = decision minus request; *n in plan* and *prior rejections* as defined in section 1; *draft Jobs* = Jobs under review; *locked Jobs shown* = already-locked Jobs rendered as context; *plan chars* and *lines* = the rendered plan the User was shown; *feedback chars* = length of the decision text; *reviewed Jobs later cancelled* = Jobs of this review cancelled after approval (approved reviews only). The gist is given for the 4 reviews with a decision text and is hand-read; `–` means no text. Who wrote the 4 texts: section 5 E1 (the two `cockpit-statusbar` texts, written and submitted by agents), `cockpit-parity` r1 (written and POSTed to `/api/approve` by Manager session `596d007f`, `…596d007f….jsonl` line 1760, 18:41:25Z) and section 6.4 (`dev-speed` r3, written by the Lead). No text of the other 78 decisions exists to attribute.

| plan | review | requested (UTC) | decided (UTC) | min | status | n in plan | prior rejections | draft Jobs | locked Jobs shown | plan chars | lines | feedback chars | reviewed Jobs later cancelled | gist |
|---|---|---|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| cockpit-statusbar | r1 | 10-15 17:02 | 10-15 17:04 | 2.4 | rejected | 1 | 0 | 3 | 0 | 23911 | 231 | 1461 | – | rejected only to raise 3 Tiers (docs H2→S2, 2×S1→S3) and add one Background sentence; "everything else is approved"; text cites code names it checked |
| cockpit-statusbar | r2 | 10-15 17:05 | 10-15 17:06 | 0.4 | approved | 2 | 1 | 3 | 0 | 24310 | 231 | 80 | 0 of 3 | approved: Tiers and sentence as asked; "Lock and dispatch." |
| cockpit-parity | r1 | 10-15 18:27 | 10-15 18:41 | 13.6 | approved | 1 | 0 | 4 | 0 | 17261 | 252 | 274 | 0 of 4 | approved: gate open (cpd 1 landed six Jobs; cargo 702 passed); "Lock and dispatch." |
| switch-over | r1 | 10-15 20:22 | 10-15 20:23 | 1.2 | approved | 1 | 0 | 6 | 0 | 40459 | 446 | 0 | 0 of 6 | – |
| machine-sync | r1 | 10-15 20:49 | 10-15 20:54 | 5.6 | approved | 1 | 0 | 2 | 0 | 17401 | 151 | 0 | 0 of 2 | – |
| cross-repo | r1 | 10-16 14:04 | 10-16 14:08 | 3.7 | approved | 1 | 0 | 4 | 0 | 37936 | 370 | 0 | 0 of 4 | – |
| dotfiles-sync | r1 | 10-16 15:01 | 10-16 15:11 | 9.9 | approved | 1 | 0 | 2 | 0 | 14541 | 150 | 0 | 0 of 2 | – |
| hardening | r1 | 10-16 15:02 | 10-16 15:15 | 13.0 | approved | 1 | 0 | 7 | 0 | 60209 | 558 | 0 | 0 of 7 | – |
| tree-menu | r1 | 10-16 15:45 | 10-16 16:09 | 24.8 | approved | 1 | 0 | 8 | 0 | 76703 | 613 | 0 | 0 of 8 | – |
| cpd-ci | r1 | 10-16 17:27 | 10-16 18:09 | 41.9 | approved | 1 | 0 | 1 | 0 | 5169 | 82 | 0 | 0 of 1 | – |
| pr-poller | r1 | 10-16 18:24 | 10-16 18:32 | 7.3 | approved | 1 | 0 | 1 | 0 | 8026 | 101 | 0 | 0 of 1 | – |
| tui-visuals | r1 | 10-16 18:59 | 10-16 18:59 | 0.3 | approved | 1 | 0 | 1 | 0 | 4767 | 70 | 0 | 0 of 1 | – |
| plan-reopen | r1 | 10-16 21:06 | 10-16 21:09 | 3.0 | approved | 1 | 0 | 4 | 0 | 31458 | 291 | 0 | 0 of 4 | – |
| manager | r1 | 10-17 22:46 | 10-17 23:14 | 28.0 | approved | 1 | 0 | 4 | 0 | 33168 | 313 | 0 | 0 of 4 | – |
| dev-speed | r1 | 10-17 23:50 | 10-17 23:57 | 6.1 | approved | 1 | 0 | 6 | 0 | 28357 | 356 | 0 | 0 of 6 | – |
| pi-pin | r1 | 10-17 23:59 | 10-18 00:00 | 0.7 | approved | 1 | 0 | 6 | 0 | 52794 | 474 | 0 | 0 of 6 | – |
| tui-dots | r1 | 10-18 01:03 | 10-18 01:04 | 1.0 | approved | 1 | 0 | 6 | 0 | 35502 | 462 | 0 | 0 of 6 | – |
| quality | r1 | 10-18 01:10 | 10-18 01:17 | 7.0 | approved | 1 | 0 | 7 | 0 | 58185 | 559 | 0 | 2 of 7 | – |
| argv-fix | r1 | 10-18 05:10 | 10-18 05:12 | 2.0 | approved | 1 | 0 | 1 | 0 | 8405 | 94 | 0 | 0 of 1 | – |
| inbox | r1 | 10-18 05:13 | 10-18 05:19 | 6.0 | approved | 1 | 0 | 13 | 0 | 83848 | 869 | 0 | 8 of 13 | – |
| pr-conflicts | r1 | 10-18 05:30 | 10-18 05:31 | 0.8 | approved | 1 | 0 | 4 | 0 | 29878 | 275 | 0 | 0 of 4 | – |
| pr-links | r1 | 10-18 05:32 | 10-18 05:34 | 2.3 | approved | 1 | 0 | 4 | 0 | 30730 | 349 | 0 | 0 of 4 | – |
| dev-speed | r2 | 10-18 05:58 | 10-18 06:01 | 3.1 | approved | 2 | 0 | 6 | 11 | 88017 | 953 | 0 | 0 of 6 | – |
| tui-visuals | r2 | 10-18 13:21 | 10-18 13:23 | 2.1 | approved | 2 | 0 | 1 | 37 | 270633 | 3338 | 0 | 0 of 1 | – |
| dev-speed | r3 | 10-18 13:33 | 10-18 14:34 | 60.2 | closed by Lead | 3 | 0 | 1 | 17 | 144063 | 1592 | 148 | – | Lead closed r3 as superseded by r4 (approved 14:10Z); "kept the Lead blocked" |
| dev-speed | r4 | 10-18 13:39 | 10-18 14:10 | 31.3 | approved | 4 | 0 | 2 | 17 | 150402 | 1672 | 0 | 1 of 2 | – |
| pi-pin | r2 | 10-18 13:39 | 10-18 13:57 | 18.2 | approved | 2 | 0 | 2 | 31 | 245117 | 2786 | 0 | 0 of 2 | – |
| tui-visuals | r3 | 10-18 13:57 | 10-18 14:08 | 11.7 | approved | 3 | 0 | 1 | 38 | 311943 | 3812 | 0 | 0 of 1 | – |
| pr-links | r2 | 10-18 14:04 | 10-18 14:05 | 1.0 | approved | 2 | 0 | 3 | 5 | 66887 | 818 | 0 | 0 of 3 | – |
| tui-visuals | r4 | 10-18 14:18 | 10-18 14:27 | 9.9 | approved | 4 | 0 | 1 | 40 | 317062 | 3899 | 0 | 0 of 1 | – |
| pr-conflicts | r2 | 10-18 14:22 | 10-18 14:38 | 15.4 | approved | 2 | 0 | 2 | 6 | 75729 | 801 | 0 | 0 of 2 | – |
| pi-pin | r3 | 10-18 16:53 | 10-18 21:35 | 282.1 | approved | 3 | 0 | 1 | 35 | 247864 | 2843 | 0 | 0 of 1 | – |
| pulse | r1 | 10-18 21:21 | 10-18 21:23 | 1.8 | approved | 1 | 0 | 3 | 0 | 43571 | 330 | 0 | 0 of 3 | – |
| pr-conflicts | r3 | 10-18 21:27 | 10-18 21:29 | 2.4 | approved | 3 | 0 | 1 | 9 | 82736 | 899 | 0 | 0 of 1 | – |
| quality | r2 | 10-18 21:44 | 10-18 21:47 | 3.5 | approved | 2 | 0 | 36 | 24 | 462201 | 5432 | 0 | 9 of 36 | – |
| tui-dots | r2 | 10-18 22:00 | 10-18 22:03 | 3.3 | approved | 2 | 0 | 4 | 23 | 344373 | 4042 | 0 | 0 of 4 | – |
| manager | r2 | 10-18 22:12 | 10-18 22:17 | 4.5 | approved | 2 | 0 | 4 | 40 | 512152 | 5182 | 0 | 0 of 4 | – |
| manager | r3 | 10-18 22:17 | 10-18 22:18 | 0.3 | approved | 3 | 0 | 1 | 44 | 520975 | 5304 | 0 | 0 of 1 | – |
| cross-repo | r2 | 10-18 22:30 | 10-18 22:33 | 2.9 | approved | 2 | 0 | 6 | 60 | 899250 | 9504 | 0 | 1 of 6 | – |
| pi-pin | r4 | 10-18 22:31 | 10-18 22:34 | 2.7 | approved | 4 | 0 | 1 | 36 | 339549 | 3850 | 0 | 1 of 1 | – |
| manager | r4 | 10-19 00:19 | 10-19 00:21 | 1.6 | approved | 4 | 0 | 2 | 45 | 562636 | 5809 | 0 | 0 of 2 | – |
| interface | r1 | 10-19 00:32 | 10-19 00:33 | 1.6 | approved | 1 | 0 | 2 | 0 | 25686 | 306 | 0 | 0 of 2 | – |
| cross-repo | r3 | 10-19 00:38 | 10-19 00:40 | 2.2 | approved | 3 | 0 | 4 | 65 | 942820 | 10036 | 0 | 0 of 4 | – |
| cross-repo | r4 | 10-19 00:40 | 10-19 00:41 | 0.6 | approved | 4 | 0 | 3 | 65 | 927670 | 9830 | 0 | 0 of 3 | – |
| tui-visuals | r5 | 10-19 00:47 | 10-19 00:48 | 1.6 | approved | 5 | 0 | 6 | 44 | 555323 | 6610 | 0 | 4 of 6 | – |
| cross-repo | r5 | 10-19 01:15 | 10-19 01:16 | 1.1 | approved | 5 | 0 | 7 | 68 | 1000742 | 10778 | 0 | 0 of 7 | – |
| tui-visuals | r6 | 10-19 01:58 | 10-19 02:12 | 14.1 | approved | 6 | 0 | 1 | 50 | 284127 | 3440 | 0 | 0 of 1 | – |
| pi-pin | r5 | 10-19 01:58 | 10-19 02:07 | 8.6 | approved | 5 | 0 | 4 | 38 | 244098 | 2752 | 0 | 1 of 4 | – |
| repo-policy | r1 | 10-19 02:03 | 10-19 02:16 | 13.2 | approved | 1 | 0 | 1 | 0 | 10707 | 191 | 0 | 0 of 1 | – |
| tui-visuals | r7 | 10-19 02:27 | 10-19 02:29 | 2.1 | approved | 7 | 0 | 5 | 51 | 312081 | 3797 | 0 | 5 of 5 | – |
| cross-repo | r6 | 10-19 02:48 | 10-19 02:49 | 0.5 | approved | 6 | 0 | 2 | 75 | 530828 | 5256 | 0 | 1 of 2 | – |
| cross-repo | r7 | 10-19 02:49 | 10-19 02:50 | 0.6 | approved | 7 | 0 | 1 | 77 | 534577 | 5297 | 0 | 0 of 1 | – |
| cross-repo | r8 | 10-19 02:58 | 10-19 03:01 | 2.5 | approved | 8 | 0 | 5 | 77 | 560872 | 5568 | 0 | 3 of 5 | – |
| pr-links | r3 | 10-19 03:14 | 10-19 03:15 | 0.6 | approved | 3 | 0 | 4 | 9 | 88590 | 1081 | 0 | 0 of 4 | – |
| tui-visuals | r8 | 10-19 03:16 | 10-19 03:21 | 5.4 | approved | 8 | 0 | 7 | 52 | 316913 | 3945 | 0 | 0 of 7 | – |
| tui-visuals | r9 | 10-19 03:45 | 10-19 03:46 | 1.2 | approved | 9 | 0 | 3 | 59 | 342169 | 4163 | 0 | 0 of 3 | – |
| quality | r3 | 10-19 03:49 | 10-19 03:50 | 1.3 | approved | 3 | 0 | 24 | 64 | 564476 | 6416 | 0 | 0 of 24 | – |
| repo-policy | r2 | 10-19 03:53 | 10-19 03:54 | 0.4 | approved | 2 | 0 | 1 | 1 | 21163 | 358 | 0 | 0 of 1 | – |
| dev-speed | r5 | 10-19 03:56 | 10-19 03:56 | 0.5 | approved | 5 | 0 | 3 | 22 | 128896 | 1441 | 0 | 0 of 3 | – |
| quality | r4 | 10-19 05:58 | – | – | undecided | 4 | 0 | 2 | 89 | 568076 | 6455 | 0 | – | – |
| quality | r5 | 10-19 06:02 | – | – | undecided | 5 | 0 | 3 | 89 | 576813 | 6553 | 0 | – | – |
| cross-repo | r9 | 10-19 13:02 | – | – | undecided | 9 | 0 | 8 | 79 | 580336 | 5821 | 0 | – | – |
| cross-repo | r10 | 10-19 13:15 | 10-19 14:18 | 63.6 | approved | 10 | 0 | 7 | 79 | 571497 | 5760 | 0 | 3 of 7 | – |
| quality | r6 | 10-19 13:17 | 10-19 14:04 | 47.2 | approved | 6 | 0 | 8 | 91 | 633420 | 7123 | 0 | 4 of 8 | – |
| pr-links | r4 | 10-19 13:20 | – | – | undecided | 4 | 0 | 4 | 13 | 125713 | 1440 | 0 | – | – |
| pi-pin | r6 | 10-19 13:47 | 10-19 14:10 | 23.2 | approved | 6 | 0 | 1 | 47 | 277089 | 3178 | 0 | 0 of 1 | – |
| cross-repo | r11 | 10-19 14:19 | 10-19 14:37 | 17.5 | approved | 11 | 0 | 4 | 79 | 559430 | 5598 | 0 | 0 of 4 | – |
| pr-links | r5 | 10-19 14:23 | 10-19 14:37 | 13.5 | approved | 5 | 0 | 7 | 13 | 151624 | 1699 | 0 | 0 of 7 | – |
| repo-policy | r3 | 10-19 14:23 | 10-19 14:37 | 13.9 | approved | 3 | 0 | 1 | 2 | 35870 | 563 | 0 | 0 of 1 | – |
| repo-policy | r4 | 10-19 14:52 | 10-19 15:10 | 17.9 | approved | 4 | 0 | 10 | 3 | 105939 | 1251 | 0 | 0 of 10 | – |
| cross-repo | r12 | 10-19 14:52 | 10-19 15:10 | 17.8 | approved | 12 | 0 | 2 | 83 | 556665 | 5623 | 0 | 0 of 2 | – |
| pi-pin | r7 | 10-19 15:39 | 10-19 16:56 | 77.2 | approved | 7 | 0 | 2 | 48 | 265044 | 3126 | 0 | 0 of 2 | – |
| cross-repo | r13 | 10-19 16:12 | 10-19 16:44 | 31.5 | approved | 13 | 0 | 1 | 85 | 560726 | 5677 | 0 | 0 of 1 | – |
| cross-repo | r14 | 10-19 16:47 | 10-19 16:54 | 6.6 | approved | 14 | 0 | 1 | 86 | 562226 | 5703 | 0 | 0 of 1 | – |
| release | r1 | 10-19 16:57 | 10-19 17:01 | 3.5 | approved | 1 | 0 | 1 | 0 | 6682 | 80 | 0 | 0 of 1 | – |
| quality | r7 | 10-19 17:44 | 10-19 17:47 | 2.4 | approved | 7 | 0 | 5 | 102 | 641554 | 7257 | 0 | 5 of 5 | – |
| release | r2 | 10-19 18:26 | 10-19 18:29 | 3.2 | approved | 2 | 0 | 4 | 1 | 32431 | 303 | 0 | 0 of 4 | – |
| quality | r8 | 10-20 04:39 | 10-20 12:59 | 500.2 | approved | 8 | 0 | 1 | 113 | 649484 | 7496 | 0 | 0 of 1 | – |
| quality | r9 | 10-20 14:17 | 10-20 15:13 | 55.9 | approved | 9 | 0 | 3 | 115 | 648326 | 7575 | 0 | 0 of 3 | – |
| tui-visuals | r10 | 10-20 14:23 | 10-20 14:43 | 20.0 | approved | 10 | 0 | 1 | 68 | 323265 | 4252 | 0 | 0 of 1 | – |
| quality | r10 | 10-20 16:02 | 10-20 16:04 | 2.3 | approved | 10 | 0 | 7 | 123 | 686724 | 8132 | 0 | 3 of 7 | – |
| plan-ux | r1 | 10-20 20:06 | 10-20 20:07 | 0.9 | approved | 1 | 0 | 3 | 0 | 18372 | 175 | 0 | 0 of 3 | – |

## 3. ask_user_question rounds with the User (Manager and 5 Leads: 186 rounds)

Columns: *Qs* = questions in the call; *options per Q* = option counts; *chars* = shown (section 1); *multi Qs* = multi-select questions; *answered* = answers returned; *picked first / recommended of N* = among the N single-select questions answered with an option, how many picked the first option, and how many picked one labelled `(Recommended)`; *free-text answers* = chars of each typed answer (`–` none); *multi answers* = multi-select answers returned; *cancelled*; *latency* in minutes (`durationMs`). Rows are sorted by session then time. The tool's description in the session prompt reads "Ask the user up to 4 structured questions (2-4 options each)" (`…6ec7fcdc….jsonl` line 6).

| session | at (UTC) | Qs | options per Q | chars | multi Qs | answered | picked first / recommended (of option answers) | free-text answers (chars) | multi answers | cancelled | latency min |
|---|---|---:|---|---:|---:|---:|---|---|---:|---|---:|
| Manager 97de2471 | 10-18 13:23 | 3 | 3/3/2 | 1429 | 0 | 3 | 3/3 of 3 | – | 0 | – | 18.4 |
| Manager 97de2471 | 10-18 13:47 | 2 | 3/3 | 746 | 0 | 2 | 1/1 of 2 | – | 0 | – | 0.9 |
| Manager 97de2471 | 10-18 14:33 | 2 | 4/3 | 651 | 1 | 2 | 0/0 of 1 | – | 1 | – | 22.4 |
| Manager 97de2471 | 10-18 15:06 | 2 | 3/2 | 509 | 0 | 2 | 2/2 of 2 | – | 0 | – | 395.4 |
| Manager 97de2471 | 10-18 21:51 | 1 | 2 | 325 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.9 |
| Manager 97de2471 | 10-18 21:57 | 2 | 3/2 | 723 | 0 | 2 | 0/0 of 1 | 101 | 0 | – | 2.9 |
| Manager 97de2471 | 10-18 22:07 | 1 | 3 | 460 | 1 | 1 | 0/0 of 0 | – | 1 | – | 3.3 |
| Manager 97de2471 | 10-18 22:33 | 3 | 2/2/2 | 897 | 0 | 3 | 2/2 of 3 | – | 0 | – | 51.1 |
| Manager 97de2471 | 10-18 23:25 | 3 | 3/3/3 | 1130 | 0 | 3 | 0/0 of 1 | 90,225 | 0 | – | 22.2 |
| Manager 97de2471 | 10-18 23:50 | 4 | 3/2/2/2 | 1215 | 0 | 4 | 3/3 of 4 | – | 0 | – | 25.4 |
| Manager 97de2471 | 10-19 00:18 | 1 | 2 | 310 | 0 | 1 | 1/1 of 1 | – | 0 | – | 3.4 |
| Manager 97de2471 | 10-19 00:30 | 1 | 2 | 291 | 1 | 1 | 0/0 of 0 | – | 1 | – | 16.5 |
| Manager 97de2471 | 10-19 01:20 | 2 | 3/2 | 1128 | 0 | 2 | 2/2 of 2 | – | 0 | – | 4.3 |
| Manager 97de2471 | 10-19 01:45 | 2 | 2/2 | 1044 | 0 | 2 | 0/0 of 1 | 78 | 0 | – | 2.4 |
| Manager 97de2471 | 10-19 01:49 | 1 | 3 | 727 | 0 | 1 | 0/0 of 0 | 294 | 0 | – | 3.6 |
| Manager 97de2471 | 10-19 02:33 | 1 | 2 | 669 | 0 | 1 | 1/1 of 1 | – | 0 | – | 1.0 |
| Manager 97de2471 | 10-19 02:49 | 1 | 4 | 1271 | 1 | 1 | 0/0 of 0 | – | 1 | – | 2.0 |
| Manager 97de2471 | 10-19 03:22 | 1 | 3 | 940 | 1 | 1 | 0/0 of 0 | – | 1 | – | 0.9 |
| Manager 97de2471 | 10-19 03:29 | 1 | 3 | 943 | 0 | 1 | 1/1 of 1 | – | 0 | – | 10.2 |
| Manager 97de2471 | 10-19 03:45 | 2 | 2/2 | 954 | 0 | 2 | 1/1 of 2 | – | 0 | – | 1.8 |
| Manager 97de2471 | 10-19 04:04 | 2 | 2/2 | 1377 | 0 | 2 | 1/1 of 2 | – | 0 | – | 3.4 |
| Manager 97de2471 | 10-19 12:57 | 1 | 3 | 984 | 0 | 1 | 1/1 of 1 | – | 0 | – | 1.8 |
| Manager 97de2471 | 10-19 13:04 | 2 | 2/2 | 1331 | 0 | 2 | 0/0 of 0 | 512,146 | 0 | – | 8.0 |
| Manager 97de2471 | 10-19 13:56 | 2 | 2/2 | 1040 | 0 | 2 | 2/2 of 2 | – | 0 | – | 3.0 |
| Manager 97de2471 | 10-19 14:08 | 1 | 2 | 635 | 0 | 1 | 0/0 of 1 | – | 0 | – | 6.1 |
| Manager 97de2471 | 10-19 15:25 | 2 | 2/2 | 1582 | 0 | 2 | 1/1 of 1 | 91 | 0 | – | 3.0 |
| Manager 97de2471 | 10-19 16:49 | 2 | 2/3 | 1202 | 0 | 2 | 1/1 of 2 | – | 0 | – | 2.8 |
| Manager 97de2471 | 10-19 16:55 | 1 | 3 | 974 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.3 |
| Manager 97de2471 | 10-19 17:00 | 1 | 2 | 792 | 0 | 1 | 1/1 of 1 | – | 0 | – | 8.3 |
| Manager 97de2471 | 10-19 17:16 | 1 | 2 | 641 | 0 | 1 | 1/1 of 1 | – | 0 | – | 1.3 |
| Manager 97de2471 | 10-19 17:19 | 1 | 3 | 1210 | 0 | 1 | 0/0 of 0 | 272 | 0 | – | 2.5 |
| Manager 97de2471 | 10-19 17:39 | 2 | 3/2 | 1111 | 0 | 2 | 0/0 of 2 | – | 0 | – | 1.0 |
| Manager 97de2471 | 10-19 18:09 | 2 | 2/2 | 850 | 0 | 2 | 0/0 of 1 | 128 | 0 | – | 2.0 |
| Manager 97de2471 | 10-19 19:01 | 2 | 3/2 | 1539 | 0 | 2 | 1/1 of 1 | 92 | 0 | – | 44.2 |
| Manager 97de2471 | 10-19 19:50 | 1 | 2 | 662 | 0 | 1 | 0/0 of 0 | 46 | 0 | – | 1.0 |
| Manager 97de2471 | 10-19 20:47 | 2 | 4/2 | 1485 | 1 | 2 | 0/0 of 0 | 33 | 1 | – | 2.3 |
| Manager 97de2471 | 10-19 21:04 | 2 | 3/2 | 1283 | 0 | 2 | 0/0 of 0 | 133,118 | 0 | – | 7.8 |
| Manager 97de2471 | 10-20 01:12 | 3 | 2/2/2 | 1422 | 0 | 3 | 1/1 of 1 | 98,65 | 0 | – | 2.9 |
| Manager 97de2471 | 10-20 02:31 | 1 | 2 | 679 | 0 | 1 | 0/0 of 0 | 39 | 0 | – | 18.1 |
| Manager 97de2471 | 10-20 02:50 | 1 | 2 | 952 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.8 |
| Manager 97de2471 | 10-20 03:13 | 1 | 3 | 965 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.4 |
| Manager 97de2471 | 10-20 03:15 | 1 | 3 | 1230 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.8 |
| Manager 97de2471 | 10-20 15:06 | 2 | 2/2 | 2472 | 0 | 2 | 1/1 of 2 | – | 0 | – | 7.6 |
| Manager 97de2471 | 10-20 16:12 | 1 | 3 | 636 | 0 | 1 | 1/1 of 1 | – | 0 | – | 51.8 |
| Manager 97de2471 | 10-20 17:12 | 2 | 2/2 | 985 | 0 | 2 | 2/2 of 2 | – | 0 | – | 0.9 |
| Manager 97de2471 | 10-20 17:41 | 2 | 2/2 | 651 | 0 | 2 | 0/0 of 1 | 49 | 0 | – | 1.5 |
| Manager 97de2471 | 10-20 18:10 | 1 | 2 | 468 | 0 | 1 | 0/0 of 0 | 107 | 0 | – | 0.6 |
| Manager 97de2471 | 10-20 18:48 | 1 | 3 | 564 | 0 | 1 | 0/0 of 0 | 24 | 0 | – | 0.5 |
| Manager 97de2471 | 10-20 19:07 | 4 | 2/3/2/2 | 1099 | 0 | 4 | 2/2 of 4 | – | 0 | – | 2.6 |
| Manager 97de2471 | 10-20 19:54 | 1 | 2 | 774 | 0 | 1 | 0/0 of 0 | 88 | 0 | – | 1.1 |
| cross-repo 04598f9d | 10-16 13:38 | 4 | 3/2/3/3 | 1944 | 0 | 4 | 1/1 of 2 | 279,173 | 0 | – | 14.0 |
| cross-repo 04598f9d | 10-16 13:54 | 4 | 2/2/2/2 | 3737 | 0 | 4 | 4/0 of 4 | – | 0 | – | 8.8 |
| cross-repo 04598f9d | 10-16 14:09 | 4 | 3/3/3/4 | 2456 | 1 | 4 | 2/2 of 3 | – | 1 | – | 51.1 |
| cross-repo 04598f9d | 10-16 15:14 | 2 | 3/3 | 1121 | 0 | 2 | 0/0 of 0 | 270,208 | 0 | – | 19.7 |
| cross-repo 04598f9d | 10-16 15:34 | 1 | 2 | 1123 | 0 | 1 | 0/0 of 0 | 165 | 0 | – | 33.7 |
| cross-repo 04598f9d | 10-16 16:09 | 1 | 2 | 1990 | 0 | 1 | 1/0 of 1 | – | 0 | – | 33.2 |
| cross-repo 04598f9d | 10-16 17:50 | 1 | 3 | 1498 | 0 | 1 | 0/0 of 1 | – | 0 | – | 9.9 |
| cross-repo 04598f9d | 10-16 18:47 | 1 | 3 | 904 | 0 | 1 | 1/1 of 1 | – | 0 | – | 16.6 |
| cross-repo 04598f9d | 10-16 19:21 | 1 | 3 | 980 | 0 | 1 | 0/0 of 0 | 70 | 0 | – | 1.1 |
| cross-repo 04598f9d | 10-16 20:48 | 1 | 2 | 637 | 0 | 1 | 0/0 of 1 | – | 0 | – | 2.6 |
| cross-repo 04598f9d | 10-16 20:51 | 1 | 2 | 671 | 0 | 1 | 0/0 of 1 | – | 0 | – | 2.8 |
| cross-repo 04598f9d | 10-16 21:08 | 1 | 2 | 506 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.2 |
| cross-repo 04598f9d | 10-17 23:36 | 4 | 2/2/3/2 | 2162 | 0 | 4 | 2/1 of 4 | – | 0 | – | 2.4 |
| cross-repo 04598f9d | 10-17 23:40 | 1 | 2 | 1804 | 0 | 1 | 1/0 of 1 | – | 0 | – | 24.0 |
| cross-repo 04598f9d | 10-18 00:05 | 1 | 2 | 663 | 0 | 1 | 1/1 of 1 | – | 0 | – | 24.0 |
| cross-repo 04598f9d | 10-18 00:30 | 1 | 3 | 1660 | 0 | 1 | 1/0 of 1 | – | 0 | – | 42.0 |
| cross-repo 04598f9d | 10-18 03:11 | 2 | 3/2 | 1576 | 0 | 2 | 0/0 of 1 | 25 | 0 | – | 31.6 |
| cross-repo 04598f9d | 10-18 05:00 | 1 | 2 | 1274 | 0 | 1 | 1/0 of 1 | – | 0 | – | 22.0 |
| cross-repo 04598f9d | 10-18 05:33 | 4 | 2/3/2/2 | 2715 | 0 | 4 | 3/2 of 3 | 100 | 0 | – | 6.5 |
| cross-repo 04598f9d | 10-18 05:41 | 1 | 2 | 1496 | 0 | 1 | 1/0 of 1 | – | 0 | – | 14.5 |
| cross-repo 04598f9d | 10-18 06:00 | 4 | 4/2/2/2 | 2496 | 1 | 4 | 3/1 of 3 | – | 1 | – | 6.1 |
| cross-repo 04598f9d | 10-18 06:16 | 1 | 2 | 1263 | 0 | 1 | 1/0 of 1 | – | 0 | – | 426.4 |
| cross-repo 04598f9d | 10-18 21:37 | 1 | 2 | 832 | 0 | 1 | 1/1 of 1 | – | 0 | – | 7.4 |
| cross-repo 04598f9d | 10-18 22:20 | 3 | 4/4/2 | 1051 | 1 | 3 | 1/1 of 1 | 42 | 1 | – | 2.4 |
| cross-repo 04598f9d | 10-18 22:23 | 1 | 4 | 473 | 0 | 1 | 0/0 of 0 | 304 | 0 | – | 4.9 |
| cross-repo 04598f9d | 10-18 22:28 | 1 | 2 | 855 | 0 | 1 | 0/0 of 1 | – | 0 | – | 0.3 |
| cross-repo 04598f9d | 10-19 00:53 | 1 | 3 | 1198 | 0 | 1 | 1/0 of 1 | – | 0 | – | 21.8 |
| cross-repo 04598f9d | 10-19 02:32 | 3 | 3/3/2 | 1606 | 0 | 3 | 0/0 of 0 | 122,9,9 | 0 | – | 6.6 |
| cross-repo 04598f9d | 10-19 02:39 | 1 | 4 | 866 | 0 | 1 | 0/0 of 0 | 50 | 0 | – | 0.9 |
| cross-repo 04598f9d | 10-19 15:23 | 1 | 2 | 670 | 0 | 1 | 1/1 of 1 | – | 0 | – | 8.7 |
| cross-repo 04598f9d | 10-19 19:09 | 1 | 2 | 619 | 0 | 1 | 1/1 of 1 | – | 0 | – | 38.2 |
| dev-speed 353dd1a1 | 10-17 23:39 | 4 | 3/2/2/2 | 1112 | 0 | 4 | 3/2 of 3 | 76 | 0 | – | 8.3 |
| dev-speed 353dd1a1 | 10-18 01:29 | 2 | 2/2 | 745 | 0 | 2 | 1/0 of 2 | – | 0 | – | 23.0 |
| dev-speed 353dd1a1 | 10-18 02:35 | 1 | 3 | 369 | 0 | 1 | 1/0 of 1 | – | 0 | – | 32.4 |
| dev-speed 353dd1a1 | 10-18 03:27 | 3 | 3/3/3 | 1255 | 0 | 3 | 2/0 of 3 | – | 0 | – | 1.8 |
| dev-speed 353dd1a1 | 10-18 03:33 | 2 | 3/2 | 820 | 0 | 2 | 2/0 of 2 | – | 0 | – | 0.7 |
| dev-speed 353dd1a1 | 10-18 03:34 | 3 | 2/2/2 | 812 | 0 | 3 | 3/0 of 3 | – | 0 | – | 1.7 |
| dev-speed 353dd1a1 | 10-18 03:36 | 1 | 2 | 239 | 0 | 1 | 0/0 of 0 | 100 | 0 | – | 16.5 |
| dev-speed 353dd1a1 | 10-18 03:54 | 1 | 3 | 641 | 0 | 1 | 1/0 of 1 | – | 0 | – | 8.1 |
| dev-speed 353dd1a1 | 10-18 04:02 | 2 | 2/2 | 685 | 0 | 2 | 0/0 of 0 | 32,6 | 0 | – | 13.6 |
| dev-speed 353dd1a1 | 10-18 04:16 | 4 | 2/2/2/3 | 1178 | 0 | 4 | 3/0 of 4 | – | 0 | – | 2.6 |
| dev-speed 353dd1a1 | 10-18 04:28 | 1 | 2 | 520 | 0 | 1 | 0/0 of 0 | 34 | 0 | – | 0.9 |
| dev-speed 353dd1a1 | 10-18 04:35 | 1 | 2 | 513 | 0 | 1 | 0/0 of 0 | 55 | 0 | – | 1.2 |
| dev-speed 353dd1a1 | 10-18 04:41 | 1 | 3 | 333 | 0 | 1 | 1/0 of 1 | – | 0 | – | 0.2 |
| dev-speed 353dd1a1 | 10-18 05:45 | 1 | 3 | 398 | 0 | 1 | 0/0 of 1 | – | 0 | – | 3.3 |
| dev-speed 353dd1a1 | 10-18 06:01 | 1 | 2 | 537 | 0 | 1 | 0/0 of 1 | – | 0 | – | 1.4 |
| dev-speed 353dd1a1 | 10-18 09:59 | 1 | 2 | 365 | 0 | 1 | 1/0 of 1 | – | 0 | – | 187.4 |
| dev-speed 353dd1a1 | 10-18 13:23 | 2 | 2/2 | 552 | 0 | 2 | 2/0 of 2 | – | 0 | – | 9.6 |
| dev-speed 353dd1a1 | 10-18 13:34 | 2 | 2/2 | 646 | 0 | 2 | 2/0 of 2 | – | 0 | – | 3.8 |
| dev-speed 353dd1a1 | 10-18 14:20 | 1 | 2 | 422 | 0 | 1 | 1/0 of 1 | – | 0 | – | 13.8 |
| dev-speed 353dd1a1 | 10-18 15:59 | 1 | 2 | 534 | 0 | 1 | 1/0 of 1 | – | 0 | – | 335.9 |
| dev-speed 353dd1a1 | 10-18 22:18 | 1 | 3 | 510 | 0 | 1 | 0/0 of 1 | – | 0 | – | 2.3 |
| dev-speed 353dd1a1 | 10-19 02:25 | 1 | 2 | 514 | 0 | 1 | 0/0 of 1 | – | 0 | – | 1.2 |
| dev-speed 353dd1a1 | 10-19 03:54 | 1 | 2 | 416 | 0 | 1 | 1/0 of 1 | – | 0 | – | 1.8 |
| manager (Plan Lead) 46b9faa6 | 10-16 20:02 | 4 | 3/2/3/3 | 1509 | 0 | 4 | 4/4 of 4 | – | 0 | – | 1598.7 |
| manager (Plan Lead) 46b9faa6 | 10-17 22:42 | 2 | 2/2 | 522 | 0 | 2 | 2/1 of 2 | – | 0 | – | 0.6 |
| manager (Plan Lead) 46b9faa6 | 10-17 22:44 | 1 | 2 | 156 | 0 | 1 | 1/0 of 1 | – | 0 | – | 0.2 |
| manager (Plan Lead) 46b9faa6 | 10-17 22:47 | 1 | 3 | 423 | 0 | 1 | 1/1 of 1 | – | 0 | – | 9.3 |
| manager (Plan Lead) 46b9faa6 | 10-17 23:52 | 2 | 2/2 | 647 | 0 | 2 | 1/1 of 2 | – | 0 | – | 6.9 |
| manager (Plan Lead) 46b9faa6 | 10-18 00:00 | 1 | 2 | 157 | 0 | 1 | 1/0 of 1 | – | 0 | – | 0.6 |
| manager (Plan Lead) 46b9faa6 | 10-18 00:01 | 1 | 2 | 174 | 0 | 1 | 1/0 of 1 | – | 0 | – | 4.4 |
| manager (Plan Lead) 46b9faa6 | 10-18 00:40 | 3 | 2/2/2 | 980 | 0 | 3 | 3/3 of 3 | – | 0 | – | 12.3 |
| manager (Plan Lead) 46b9faa6 | 10-18 00:53 | 1 | 2 | 157 | 0 | 1 | 1/0 of 1 | – | 0 | – | 0.2 |
| manager (Plan Lead) 46b9faa6 | 10-18 00:55 | 1 | 2 | 202 | 0 | 1 | 1/0 of 1 | – | 0 | – | 1.4 |
| manager (Plan Lead) 46b9faa6 | 10-18 01:30 | 1 | 2 | 180 | 0 | 1 | 1/0 of 1 | – | 0 | – | 17.9 |
| manager (Plan Lead) 46b9faa6 | 10-18 01:48 | 1 | 3 | 556 | 0 | 1 | 1/0 of 1 | – | 0 | – | 72.9 |
| manager (Plan Lead) 46b9faa6 | 10-18 03:12 | 2 | 2/2 | 608 | 0 | 2 | 1/1 of 1 | 184 | 0 | – | 36.4 |
| manager (Plan Lead) 46b9faa6 | 10-18 03:49 | 1 | 2 | 142 | 0 | 1 | 0/0 of 0 | 95 | 0 | – | 4.7 |
| manager (Plan Lead) 46b9faa6 | 10-18 21:49 | 1 | 3 | 380 | 0 | 1 | 1/0 of 1 | – | 0 | – | 6.2 |
| manager (Plan Lead) 46b9faa6 | 10-19 01:59 | 1 | 3 | 478 | 0 | 1 | 0/0 of 1 | – | 0 | – | 8.8 |
| switch-over 2e66cebd | 10-15 20:20 | 1 | 2 | 663 | 0 | 1 | 0/0 of 1 | – | 0 | – | 0.9 |
| switch-over 2e66cebd | 10-16 13:27 | 4 | 3/2/3/3 | 1988 | 0 | 4 | 3/3 of 3 | 1058 | 0 | – | 9.4 |
| switch-over 2e66cebd | 10-16 13:47 | 4 | 2/2/2/3 | 2550 | 0 | 4 | 3/3 of 3 | 107 | 0 | – | 11.8 |
| switch-over 2e66cebd | 10-16 14:30 | 4 | 3/2/2/4 | 1657 | 1 | 4 | 0/0 of 0 | 459,150,158 | 1 | – | 28.0 |
| switch-over 2e66cebd | 10-16 15:13 | 2 | 2/2 | 1203 | 0 | 2 | 0/0 of 2 | – | 0 | – | 17.8 |
| switch-over 2e66cebd | 10-16 15:47 | 2 | 3/3 | 1076 | 0 | 2 | 1/1 of 1 | 219 | 0 | – | 2.0 |
| switch-over 2e66cebd | 10-16 15:50 | 4 | 3/3/3/2 | 1670 | 0 | 4 | 1/1 of 1 | 290,843,687 | 0 | – | 15.7 |
| switch-over 2e66cebd | 10-16 16:06 | 3 | 2/3/3 | 2055 | 0 | 3 | 1/1 of 1 | 70,812 | 0 | – | 35.3 |
| switch-over 2e66cebd | 10-16 16:43 | 4 | 2/3/3/4 | 1718 | 1 | 4 | 3/3 of 3 | – | 1 | – | 14.2 |
| switch-over 2e66cebd | 10-16 16:58 | 2 | 3/3 | 1256 | 0 | 2 | 2/2 of 2 | – | 0 | – | 30.0 |
| switch-over 2e66cebd | 10-16 17:29 | 1 | 2 | 711 | 0 | 1 | 0/0 of 1 | – | 0 | – | 0.4 |
| switch-over 2e66cebd | 10-16 17:33 | 3 | 2/3/2 | 1374 | 0 | 3 | 3/3 of 3 | – | 0 | – | 1.0 |
| switch-over 2e66cebd | 10-16 17:46 | 4 | 3/2/3/3 | 1723 | 0 | 4 | 1/1 of 3 | 133 | 0 | – | 5.7 |
| switch-over 2e66cebd | 10-16 17:53 | 3 | 2/3/2 | 1912 | 0 | 3 | 2/2 of 3 | – | 0 | – | 19.2 |
| switch-over 2e66cebd | 10-16 18:12 | 1 | 2 | 907 | 0 | 1 | 1/1 of 1 | – | 0 | – | 4.8 |
| switch-over 2e66cebd | 10-16 18:41 | 4 | 2/3/2/2 | 1616 | 0 | 4 | 2/2 of 4 | – | 0 | – | 1.5 |
| switch-over 2e66cebd | 10-16 19:33 | 2 | 2/2 | 1347 | 0 | 2 | 2/2 of 2 | – | 0 | – | 27.7 |
| switch-over 2e66cebd | 10-16 20:45 | 3 | 2/2/2 | 1004 | 0 | 3 | 3/3 of 3 | – | 0 | – | 5.2 |
| switch-over 2e66cebd | 10-16 20:56 | 1 | 3 | 589 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.2 |
| switch-over 2e66cebd | 10-17 22:09 | 1 | 2 | 1013 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.3 |
| switch-over 2e66cebd | 10-17 23:14 | 2 | 3/2 | 1170 | 0 | 2 | 2/2 of 2 | – | 0 | – | 9.7 |
| switch-over 2e66cebd | 10-17 23:33 | 2 | 2/2 | 1066 | 0 | 2 | 1/1 of 2 | – | 0 | – | 4.0 |
| switch-over 2e66cebd | 10-18 00:05 | 2 | 2/2 | 1070 | 0 | 2 | 2/2 of 2 | – | 0 | – | 7.0 |
| switch-over 2e66cebd | 10-18 01:46 | 3 | 2/2/3 | 1360 | 0 | 3 | 2/2 of 2 | 153 | 0 | – | 61.1 |
| switch-over 2e66cebd | 10-18 03:16 | 3 | 2/4/3 | 1444 | 1 | 3 | 2/1 of 2 | – | 1 | – | 7.7 |
| switch-over 2e66cebd | 10-18 03:24 | 4 | 3/2/2/3 | 1834 | 0 | 4 | 4/4 of 4 | – | 0 | – | 7.0 |
| switch-over 2e66cebd | 10-18 03:32 | 4 | 2/2/2/2 | 1524 | 0 | 4 | 3/3 of 4 | – | 0 | – | 7.5 |
| switch-over 2e66cebd | 10-18 03:40 | 2 | 3/2 | 1406 | 0 | 2 | 0/0 of 2 | – | 0 | – | 44.0 |
| switch-over 2e66cebd | 10-18 04:32 | 2 | 2/2 | 1077 | 0 | 2 | 1/1 of 1 | 175 | 0 | – | 19.6 |
| switch-over 2e66cebd | 10-18 04:59 | 3 | 4/2/2 | 1740 | 1 | 3 | 1/1 of 2 | – | 1 | – | 17.6 |
| switch-over 2e66cebd | 10-18 05:31 | 1 | 2 | 563 | 0 | 1 | 1/0 of 1 | – | 0 | – | 0.1 |
| switch-over 2e66cebd | 10-18 05:43 | 2 | 2/2 | 705 | 0 | 2 | 0/0 of 2 | – | 0 | – | 0.6 |
| switch-over 2e66cebd | 10-18 05:51 | 2 | 3/2 | 703 | 0 | 2 | 1/1 of 2 | – | 0 | – | 0.5 |
| switch-over 2e66cebd | 10-18 13:30 | 3 | 3/2/2 | 1769 | 0 | 3 | 3/3 of 3 | – | 0 | – | 4.9 |
| tui-visuals 4965bfc8 | 10-16 18:50 | 2 | 3/2 | 882 | 0 | 2 | 1/1 of 1 | 606 | 0 | – | 5.2 |
| tui-visuals 4965bfc8 | 10-16 18:56 | 2 | 2/2 | 717 | 0 | 2 | 0/0 of 1 | 96 | 0 | – | 1.6 |
| tui-visuals 4965bfc8 | 10-16 19:02 | 2 | 3/2 | 1125 | 0 | 2 | 1/1 of 2 | – | 0 | – | 0.8 |
| tui-visuals 4965bfc8 | 10-16 19:04 | 1 | 2 | 342 | 0 | 1 | 1/0 of 1 | – | 0 | – | 91.1 |
| tui-visuals 4965bfc8 | 10-16 20:36 | 1 | 2 | 293 | 0 | 1 | 1/0 of 1 | – | 0 | – | 0.8 |
| tui-visuals 4965bfc8 | 10-16 20:40 | 1 | 2 | 315 | 0 | 1 | 1/1 of 1 | – | 0 | – | 2.4 |
| tui-visuals 4965bfc8 | 10-16 20:45 | 2 | 3/2 | 473 | 0 | 2 | 1/0 of 1 | 60 | 0 | – | 4.1 |
| tui-visuals 4965bfc8 | 10-16 20:50 | 1 | 2 | 423 | 0 | 1 | 0/0 of 1 | – | 0 | – | 1.7 |
| tui-visuals 4965bfc8 | 10-16 20:53 | 1 | 2 | 364 | 0 | 1 | 0/0 of 1 | – | 0 | – | 1.5 |
| tui-visuals 4965bfc8 | 10-17 22:10 | 1 | 2 | 433 | 0 | 1 | 0/0 of 1 | – | 0 | – | 2.2 |
| tui-visuals 4965bfc8 | 10-17 22:32 | 3 | 2/2/3 | 1456 | 0 | 3 | 1/0 of 2 | 125 | 0 | – | 3.4 |
| tui-visuals 4965bfc8 | 10-17 23:17 | 4 | 2/4/2/2 | 1558 | 0 | 4 | 3/1 of 3 | 330 | 0 | – | 6.0 |
| tui-visuals 4965bfc8 | 10-17 23:31 | 2 | 3/2 | 755 | 0 | 2 | 1/1 of 2 | – | 0 | – | 30.0 |
| tui-visuals 4965bfc8 | 10-18 00:03 | 1 | 2 | 380 | 0 | 1 | 1/1 of 1 | – | 0 | – | 2.2 |
| tui-visuals 4965bfc8 | 10-18 00:11 | 2 | 2/2 | 703 | 0 | 2 | 2/2 of 2 | – | 0 | – | 0.8 |
| tui-visuals 4965bfc8 | 10-18 00:12 | 1 | 3 | 530 | 0 | 1 | 1/1 of 1 | – | 0 | – | 1.3 |
| tui-visuals 4965bfc8 | 10-18 00:41 | 1 | 2 | 300 | 0 | 1 | 1/1 of 1 | – | 0 | – | 0.1 |
| tui-visuals 4965bfc8 | 10-18 01:28 | 4 | 2/2/2/2 | 2255 | 1 | 4 | 3/3 of 3 | – | 1 | – | 19.5 |
| tui-visuals 4965bfc8 | 10-18 02:29 | 1 | 3 | 666 | 0 | 1 | 1/1 of 1 | – | 0 | – | 18.6 |
| tui-visuals 4965bfc8 | 10-18 04:13 | 4 | 2/2/2/3 | 1112 | 0 | 4 | 4/2 of 4 | – | 0 | – | 9.2 |
| tui-visuals 4965bfc8 | 10-18 06:42 | 2 | 2/2 | 719 | 0 | 2 | 1/1 of 2 | – | 0 | – | 387.8 |
| tui-visuals 4965bfc8 | 10-18 13:30 | 1 | 3 | 462 | 0 | 1 | 1/0 of 1 | – | 0 | – | 3.2 |
| tui-visuals 4965bfc8 | 10-18 16:01 | 1 | 3 | 471 | 0 | 1 | 1/0 of 1 | – | 0 | – | 331.7 |
| tui-visuals 4965bfc8 | 10-19 00:40 | 4 | 2/2/3/2 | 1603 | 0 | 4 | 4/1 of 4 | – | 0 | – | 3.8 |
| tui-visuals 4965bfc8 | 10-19 02:23 | 2 | 4/2 | 517 | 1 | 2 | 1/0 of 1 | – | 1 | – | 3.1 |
| tui-visuals 4965bfc8 | 10-19 03:10 | 3 | 4/3/3 | 1035 | 1 | 3 | 1/0 of 2 | – | 1 | – | 4.9 |
| tui-visuals 4965bfc8 | 10-19 03:32 | 1 | 3 | 493 | 0 | 1 | 1/1 of 1 | – | 0 | – | 5.8 |
| tui-visuals 4965bfc8 | 10-19 20:02 | 3 | 3/2/2 | 1790 | 0 | 3 | 2/2 of 3 | – | 0 | – | 0.9 |
| tui-visuals 4965bfc8 | 10-19 21:23 | 1 | 2 | 494 | 0 | 1 | 1/0 of 1 | – | 0 | – | 2.8 |
| tui-visuals 4965bfc8 | 10-19 21:32 | 1 | 2 | 644 | 0 | 1 | 0/0 of 0 | 35 | 0 | – | 147.0 |
| tui-visuals 4965bfc8 | 10-19 23:59 | 2 | 3/2 | 939 | 0 | 2 | 0/0 of 1 | 29 | 0 | – | 1.0 |
| tui-visuals 4965bfc8 | 10-20 03:47 | 1 | 2 | 671 | 0 | 1 | 1/0 of 1 | – | 0 | – | 551.1 |

## 4. Aggregates

### 4.1 Plan reviews

- Plan-review requests: 82 (160 events: 82 pending + 76 approved + 2 rejected)
- Outcomes: approved 76, rejected by the User 1, closed by the Lead 1, undecided (no decision event) 4, `gone` 0
- Decision time, minutes, approved reviews: median 3.5, range 0.3–500.2, n=76
- Decision time, minutes, all User decisions (approved + rejected): median 3.5, range 0.3–500.2, n=77
  - approved within 1 min: 14 of 76
  - approved within 5 min: 41 of 76
  - approved within 30 min: 67 of 76
  - approved within 60 min: 72 of 76
- Plan size shown, characters, all 82 reviews: median 197861, range 4767–1000742, n=82
- Plan size shown, lines: median 2226, range 70–10778, n=82
- Draft Jobs per review: median 3, range 1–36, n=82
- Reviews whose decision text is non-empty: 4 of 82 (chars: [1461, 80, 274, 148])
- Plans: 26; plans whose first review was approved: 25; plans with a User rejection: 1
- Approved reviews approved with no earlier rejection of the same Plan: 75 of 76 (99%)
- Draft Jobs parsed from the approved reviews' rendered plans: 323; of them later cancelled (a `job-cancelled` event for the same Plan and Job key after the decision): 51 (16%); approved reviews with at least one such Job: 15 of 76
- Reviews showing Locked Jobs as context: 55 of 82; for those, Locked Jobs shown per draft Job: median 15.5, range 0.2–113.0, n=55
- Plan size, characters, reviews that show Locked Jobs (55): median 342169, range 21163–1000742, n=55; reviews that show none (27): median 28357, range 4767–83848, n=27
- Largest plans shown: cross-repo/r5 1000742 chars (approved, 1.1 min), cross-repo/r3 942820 chars (approved, 2.2 min), cross-repo/r4 927670 chars (approved, 0.6 min), cross-repo/r2 899250 chars (approved, 2.9 min), quality/r10 686724 chars (approved, 2.3 min)
- Approved reviews at or below the median size (197861 chars): decision minutes median 3.4, range 0.3–41.9, n=38; above it: median 4.0, range 0.3–500.2, n=38
- Decisions whose time agrees within 1 s with a plannotator `feedback/*/index.jsonl` entry of the same Plan's run dir: 75 of 77

### 4.2 ask_user_question rounds

**The 6 sessions in the table (Manager 97de2471, cross-repo 04598f9d, dev-speed 353dd1a1, manager (Plan Lead) 46b9faa6, switch-over 2e66cebd, tui-visuals 4965bfc8)**
- Rounds with a User result: 186 (+ 7 refused by schema validation, 2 with no result)
- Answer latency, minutes: median 4.8, range 0.1–1598.7, n=186
- Answer latency buckets: ≤1 min 33, ≤5 min 96, >60 min 12 (of 186)
- Rounds showing over 1000 / 2000 / 3000 characters: 78 / 9 / 1
- Characters shown per round (questions, headers, labels, descriptions, previews): median 874, range 142–3737, n=186
- Questions per round: median 2, range 1–4, n=186; rounds with 4 questions: 24
- Options per question: median 2, range 2–4, n=348
- Questions answered: 348 (option 267, multi-select 16, free text 65)
- Questions whose first option carries '(Recommended)': 234 of 348
- Questions that offered a '(Recommended)' option: 236; the User picked it 137 (58%), another option 53 (22%), free text or multi-select 46 (19%)
- Of 267 single-option answers: first option 203 (76%), a later option 64 (24%); one with '(Recommended)' in its label 137 (51%)
- Rounds where the User typed free text: 51 of 186 (27%); free-text answer chars: median 107, range 6–1058, n=65
- Rounds with a non-first option, a multi-select or free text: 105 of 186 (56%)
- Cancelled (the User declined): 0

**The Manager session 97de2471 alone**
- Rounds with a User result: 50 (+ 4 refused by schema validation, 1 with no result)
- Answer latency, minutes: median 2.8, range 0.3–395.4, n=50
- Answer latency buckets: ≤1 min 11, ≤5 min 34, >60 min 1 (of 50)
- Rounds showing over 1000 / 2000 / 3000 characters: 20 / 1 / 0
- Characters shown per round (questions, headers, labels, descriptions, previews): median 953, range 291–2472, n=50
- Questions per round: median 2, range 1–4, n=50; rounds with 4 questions: 2
- Options per question: median 2, range 2–4, n=84
- Questions answered: 84 (option 56, multi-select 6, free text 22)
- Questions whose first option carries '(Recommended)': 77 of 84
- Questions that offered a '(Recommended)' option: 78; the User picked it 38 (49%), another option 18 (23%), free text or multi-select 22 (28%)
- Of 56 single-option answers: first option 38 (68%), a later option 18 (32%); one with '(Recommended)' in its label 38 (68%)
- Rounds where the User typed free text: 18 of 50 (36%); free-text answer chars: median 95, range 24–512, n=22
- Rounds with a non-first option, a multi-select or free text: 33 of 50 (66%)
- Cancelled (the User declined): 0

**All 28 non-Worker sessions with a round (deduplicated by tool-call id)**
- Rounds with a User result: 313 (+ 8 refused by schema validation, 3 with no result)
- Answer latency, minutes: median 4.9, range 0.1–1598.7, n=313
- Answer latency buckets: ≤1 min 54, ≤5 min 159, >60 min 21 (of 313)
- Rounds showing over 1000 / 2000 / 3000 characters: 132 / 24 / 3
- Characters shown per round (questions, headers, labels, descriptions, previews): median 882, range 120–3737, n=313
- Questions per round: median 2, range 1–4, n=313; rounds with 4 questions: 56
- Options per question: median 2, range 2–4, n=615
- Questions answered: 613 (option 470, multi-select 22, free text 121)
- Questions whose first option carries '(Recommended)': 422 of 615
- Questions that offered a '(Recommended)' option: 425; the User picked it 268 (63%), another option 73 (17%), free text or multi-select 84 (20%)
- Of 470 single-option answers: first option 374 (80%), a later option 96 (20%); one with '(Recommended)' in its label 268 (57%)
- Rounds where the User typed free text: 96 of 313 (31%); free-text answer chars: median 93, range 1–1058, n=121
- Rounds with a non-first option, a multi-select or free text: 173 of 313 (55%)
- Cancelled (the User declined): 1


## 5. Examples (where each tool helped or hurt)

Times are UTC. "Brief" = `~/.cpd/run/plan-ux/plan.md`, which holds the User's questions of 2026-10-20; I did not measure those claims, only what follows.

| # | Tool | Helped / hurt | What the record shows | Source |
|---|---|---|---|---|
| E1 | plannotator | helped | `cockpit-statusbar` r1 (3 draft Jobs, 23911 chars) was rejected for one thing: "Rejected only for the Tiers; everything else is approved as it is, so change nothing else." (1461 chars). The Lead edited the Tiers 5 s after the denial, requested r2 and r2 was approved 0.4 min later with 80 chars. **Both decisions were made by agents:** the text is the final message (17:04:20Z) of a review-fork agent session `01a14084`, and Manager session `596d007f` submitted it with `curl -X POST http://localhost:19432/api/deny` at 17:05:53.309Z and `/api/approve` at 17:06:25.778Z. The User typed neither (unverified whether the User asked for it). | events 24, 26, 32, 34; `…596d007f….jsonl` lines 1594 and 1596; `…01a14084….jsonl` line 1581; `cockpit-statusbar/plannotator/feedback/cpd/index.jsonl` |
| E2 | plannotator | hurt (size) | `cross-repo` r5: 1,000,742 characters, 10,778 lines, for 7 draft Jobs plus 68 Locked Jobs shown as context; approved 1.1 min after the request. r2, r3, r4 are 899,250, 942,820 and 927,670 chars, approved in 2.9, 2.2 and 0.6 min. Across all 82 reviews the median is 197,861 chars and 55 show Locked Jobs. The brief records the User's view that the presentation is "too verbose, hard to skim and hard to argue about"; here the measurement is only the size and the time. | table of section 2; events 12859, 12866 |
| E3 | plannotator | hurt (superseded review) | `dev-speed` r3 (144,063 chars: 1 draft Job and 17 Locked Jobs) stayed pending after r4 was approved at 14:10Z; the Lead closed r3 at 14:34Z (the event's feedback: "r3 had no plannotator page left and kept the Lead blocked"). Record hs-n158: "a superseded Plan review keeps its Lead blocked". | events 8192, 8275, 8779, 9153; hs-n158 |
| E4 | ask_user_question | helped (directed, quick) | `workflow`'s design had 12 questions. The Manager asked 2 in one dialog (2472 chars): accept the 11 technical recommendations, and #10 (update Jobs versus owner policy). The User answered in 7.6 min: "Review each in plannotator" for the 11, "Update Jobs resolve (Recommended)" for #10; #10 became hs-d357 at 15:14Z. The 11 then went to plannotator (section 6.1). | `…97de2471….jsonl` call 15:06:32.457Z, result 15:14:06.945Z; hs-d357 |
| E5 | ask_user_question | helped (the User steers) | Question: "While a flaky test's root fix is in flight, may a PR re-run its CI once?" The User typed: "no flaky tests, ever, they need fixing immediatelly and consider it top priority, they skip the queue". Recorded as hs-d238 at 22:03Z (supersedes hs-d31). | Manager round 2026-10-18 21:57Z (free text, 101 chars); hs-d238 |
| E6 | ask_user_question | helped (a recommendation corrected) | Question: "Do the rounded powerline caps around titles go?" with "No caps (Recommended)" first. The User typed: "my mocku DOES include the rounded caps, they stay". 30 min later, to another question: "the pill caps are all there, none were removed... I'm curious why you never see them when I paste them here". | Manager rounds 2026-10-20 17:41Z and 18:10Z |
| E7 | ask_user_question | helped (free text as a channel) | To "Build that part now?" the User typed: "I was not warned about the plannotator plan waiting for me anywhere! this is another bug". hs-d496 was recorded 31 s later. The answer was not one of the options. | Manager round 2026-10-20 19:54:48Z, result 19:55:53Z; hs-d496 at 19:56:24Z |
| E8 | plannotator | helped (typed annotations) | The only plannotator review of 93 with User-typed annotations (3, 593 chars) is an annotate review of `tree-menu/dots.md` on 2026-10-16: "drop colors by phase, we dont care about those. lets use color only on the cases we describe below". I tell it is the User's by the lower-case, typo'd style and because the text first appears in any session file as a `user`-role message (the tree-menu Lead's `…105f200a….jsonl` line 338, 18:36:36.490Z), 1.5 s after plannotator's submission at 18:36:34.963Z; I searched `~/.pi/agent/sessions/*/*.jsonl` for the phrase "drop colors by phase" (the grep also matches this Worker's own session, which post-dates it). | `tree-menu/plannotator/feedback/cpd/index.jsonl` (surface `annotate`, `counts.annotations` 3) |
| E9 | ask_user_question | hurt (schema refusals) | 8 of the 324 calls were refused by schema validation before the User saw anything: 5 for a header over 16 characters (for example "Manager owns main" and "Reload on install", 17 characters each) and 3 for a missing `question`. 4 of the 8 were the Manager's. | `invalid.md` (section 6.3); toolResult `isError` |
| E10 | ask_user_question | hurt / used as a plan viewer | Largest round: `cross-repo` 2026-10-16 13:54Z, 4 questions, 3737 chars, answered in 8.8 min, first option picked 4 of 4. 24 of 313 rounds exceed 2000 chars, 3 exceed 3000. hs-d468 quotes the User on 2026-10-20: "many Leads have been using ask_user_question to present plans while they should have been using plannotator". | section 3 table; section 4; hs-d468 |

## 6. Notable failures

### 6.1 A review the User was never warned about (hs-d497, session `pn-6d8537`)

| Time (UTC, 2026-10-20) | Event | Source |
|---|---|---|
| 15:06:32.457 | Manager asks 2 questions (E4) | `…97de2471….jsonl` |
| 15:14:06.945 | The User answers: "Review each in plannotator" | same |
| 15:14:56.7 | Manager calls `plannotator {"action":"annotate","target":"/home/clayton/.cpd/run/handoffs/workflow-design-questions.md","gate":true}` | same |
| 15:14:57.8 | Result: `Session: pn-6d8537 … http://localhost:19432 … End your turn now and wait`. No Inbox item (only `cpd plan review` opens `plan-review` items) | same; hs-d496 |
| 15:14–19:57 | The review stays open. The Manager's dialogs in the same window were answered 7 times (latency 0.5 to 51.8 min, median 1.1 min) | `asks.json`, Manager rounds between the two instants |
| 19:54:48.9 | Manager asks whether to build the open Job now, noting the review is "blocked on your plannotator review since ~15:10" | same |
| 19:55:53.3 | The User answers in free text: "I was not warned about the plannotator plan waiting for me anywhere! this is another bug" | same |
| 19:56:06 | `plannotator list`: "pn-6d8537 · annotate · workflow-design-questions.md · http://localhost:19432 · 4 h · open · unsent: 0" | same |
| 19:56:24.967 | hs-d496 recorded (every waiting review must reach the Inbox) | `cpd context show --record hs-d496` |
| 19:57:10.700 | plannotator store: `surface annotate`, `decision approved`, `annotations 0`, target the same file | `~/.plannotator/feedback/cpd/index.jsonl`, last line |
| 19:57:25.303 | hs-d497 recorded: "approved as written", every recommendation stands | `cpd context show --record hs-d497` |

- Open to approval: 15:14:57 to 19:57:10 = **282 min (4 h 42 min)**. Per hs-d496 the URL was `localhost`, which the User's Macs cannot reach; `192.168.3.3:19432` answered 200 (the record's own check; unverified by me).
- Size shown: the file is 3949 characters for 11 questions (`wc -m ~/.cpd/run/handoffs/workflow-design-questions.md`; plannotator kept it as `history/cpd/annotate-workflow-design-questions-md-8b834db6/001.md`). Feedback: none (0 annotations, no text). The outcome: no later record mentions hs-d497 (`cpd context show` → `jq` for the id found none); unverified that none of its 12 items changed since.
- The 19:57:25 `plannotator close` call returned "No open Plannotator review pn-6d8537": the approval had landed 14 s earlier.

### 6.2 Plan reviews that never got a decision

`undecided` = a `pending` event and no decision event. There are 4, and no review has status `gone`.

- `pr-links` r4 (2026-10-19T13:20:19Z): the review did not open. The Lead's own message at 13:20:53Z: "PR `titles`' Plan review (r4) didn't open in plannotator. pr-links and Plan quality were both given the same review port, 19438, and quality's Lead still holds it from its own earlier review." `ss -ltnp` in the session showed pid 3037725 listening on 0.0.0.0:19438. r5 was requested at 14:23:59Z (63 min later) and approved at 14:37:30Z. Source: `…a0e8398b….jsonl`, 13:20:29 to 13:20:53Z.
- `cross-repo` r9 (13:02:16Z): drafted for hs-d282; hs-d283 (13:13:30Z) "supersedes hs-d282 (1)-(3)"; r10 was requested at 13:15:01Z and approved at 14:18:40Z (63.6 min). 3 of r10's 7 draft Jobs were later cancelled (section 2). Source: events 16065, 16095, 16578; records hs-d282, hs-d283.
- `quality` r4 (2026-10-19T05:58:18Z) and r5 (06:02:21Z): no decision event. The Lead's next review is r6 at 13:17:37Z (approved 14:04:50Z). Why r4 and r5 got no decision: unverified (the Lead's 06:02:29Z message says the plan "waits for your Plan review").

### 6.3 Refused asks

- **askguard** (hs-d468; PR `askguard`, #174, merged 2026-10-20T17:43:29Z). Result: **no refusal exists in any of the 32 non-Worker sessions.** The only non-answer results of `ask_user_question` are the 8 schema refusals below and 1 decline. After the merge there are 5 rounds with a User result (chars 335, 468, 564, 774, 1099), all answered (`post_askguard.md`). Whether the running adapters had loaded the guard by then is unverified. I did not read the guard's source (out of scope).
- **Schema refusals** (the tool's own validation, before any dialog):

| session | at (UTC) | refusal |
|---|---|---|
| switch-over 2e66cebd | 10-16 13:47 | - questions.0.header: must not have more than 16 characters |
| cross-repo 04598f9d | 10-18 05:00 | - questions.0.question: must have required properties question |
| switch-over 2e66cebd | 10-18 13:30 | - questions.0.header: must not have more than 16 characters |
| Manager 97de2471 | 10-19 03:22 | - questions.0.question: must have required properties question |
| Manager 97de2471 | 10-19 04:03 | - questions.1.header: must not have more than 16 characters |
| Manager 97de2471 | 10-19 17:16 | - questions.0.header: must not have more than 16 characters |
| Manager 97de2471 | 10-19 17:39 | - questions.1.question: must have required properties question |
| quality 62b1c19b | 10-20 16:02 | - questions.2.header: must not have more than 16 characters |


### 6.4 `dev-speed` r3: a "rejection" that no one in plannotator made

Event 9153 (2026-10-18T14:34:05Z) has status `rejected` and this feedback: "Superseded by r4 (approved 2026-10-18T14:10Z). Closed by the Lead at the User's request: r3 had no plannotator page left and kept the Lead blocked." The author is the Lead, not the User (the text says so). The Plan's plannotator index has no denial; the Plan's plannotator index has an approval at 14:10:17.610Z (version 3 of that day's slug), which matches r4's approval event 8779 (14:10:17.741Z). The 60.2 min between request and "decision" is excluded from the decision-time statistics and the event is not counted as a User rejection.

### 6.5 Asks without an answer, and long waits

- **Declined (cancelled): 1.** `pr-view-sync` (`a0eb7d2c`), 2026-10-18T01:11:44Z, after 7.2 min: the dialog returned "User declined to answer questions" with `cancelled: true` and the typed text "looking good, up to date" (question: "After you merged #19 and #90, what did the Deploy/Dispatch column's PRs section show?").
- **No result: 3 calls** have a `toolCall` and no matching `toolResult` in the session file: `reopen-done` `e52815a2` 2026-10-16T21:08:44Z (1 question), `dev-speed` `353dd1a1` 2026-10-18T13:06:49Z (1 question), Manager `97de2471` 2026-10-18T13:49:07Z (4 questions). Why: unverified (a session ended or was restarted; not read further).
- **Longest waits.** Ask: Manager 2026-10-16T20:02:59Z, 1598.7 min (26.6 h); `tui-visuals` 551.1 min and `release` 548.9 min (both called 2026-10-20 ~03:50Z); `repo-policy` 451.8 min; Manager 2026-10-18T15:06Z, 395.4 min. Plan reviews: `quality` r8, 500.2 min (2026-10-20T04:39Z to 12:59Z); `cross-repo` r10 63.6 min; `dev-speed` r4 31.3 min. No cause is recorded for any of them (unverified: the User's hours are not in the data).
