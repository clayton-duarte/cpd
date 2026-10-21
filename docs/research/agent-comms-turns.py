# Lead and Manager turns, calls and cost per day, from pi session files + cpd's sessions table (read-only).
import json, os, sqlite3, collections, sys
db = sqlite3.connect('file:' + os.path.expanduser('~/.cpd/cpd.db') + '?mode=ro', uri=True)
role = {sid: r for sid, r in db.execute("select id, role from sessions where role in ('lead','manager')")}
days = sys.argv[1:] or ['2026-10-19', '2026-10-20']
T = collections.defaultdict(lambda: collections.Counter())
for d, _, fs in os.walk(os.path.expanduser('~/.pi/agent/sessions')):
    for f in fs:
        if not f.endswith('.jsonl'): continue
        sid = f.rsplit('_', 1)[-1][:-6]
        r = role.get(sid)
        if not r: continue
        prev = None
        for line in open(os.path.join(d, f), encoding='utf-8', errors='replace'):
            try: o = json.loads(line)
            except Exception: continue
            if o.get('type') == 'custom_message': prev = 'custom'; continue
            if o.get('type') != 'message': continue
            m = o.get('message') or {}
            day = (o.get('timestamp') or '')[:10]
            ro = m.get('role')
            if day in days and ro == 'assistant':
                c = T[(day, r)]
                c['calls'] += 1
                u = m.get('usage') or {}
                cost = u.get('cost') or {}
                c['cost'] += cost.get('total', 0) if isinstance(cost, dict) else 0
                if prev not in ('assistant', 'toolResult'): c['turns'] += 1
            prev = ro
for k in sorted(T):
    c = T[k]; print(k[0], k[1], 'turns=%d calls=%d cost=$%.0f' % (c['turns'], c['calls'], c['cost']))
