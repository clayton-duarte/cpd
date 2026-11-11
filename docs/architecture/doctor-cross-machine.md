# cpd doctor cross-machine probing

**Status: target, not implemented.** No code runs this check yet. `cpd doctor` runs only local checks today (`engine/src/doctor/mod.rs`). The normative contract is `web/CONTRACT.md` §31; this page is the overview and the output format it defines. Where the two differ, §31 wins and this page is wrong.

## What it does

`cpd doctor` gets one more check, `cross_machine`. It reads the other fleet machines from `$CPD_HOME/config.toml`, sends one HTTP `GET` to each machine on the two WebUI ports, and reports which of them answered:

- port `3333`: the production WebUI server (`webui-network-ports.md`);
- port `33333`: the preview Vite server.

The check is a reachability probe. It does not open the WebSocket endpoint, send an `Origin` or authenticate. A reachable port shows that something answered; it does not show that the answer is cpd.

## Configuration (target)

```toml
machine = "pro"

[fleet.air]
host = "192.168.3.5"

[fleet.hs]
host = "homeserver"
```

No `[fleet]` table means no `cross_machine` line in the report. The fleet is not discovered from dotfiles, SSH config or DNS.

## Output (target)

`cpd doctor` keeps its shape (`checks`, each with `name`, `ok`, `severity` and, when set, `found`, `expected` and `message`; `docs/commands.md` §14.1). The new entry, when one machine is down:

```json
{"name": "cross_machine", "ok": false, "severity": "warn",
 "found": [
   {"machine": "air", "host": "192.168.3.5", "port": 3333, "reachable": true, "status": 200},
   {"machine": "air", "host": "192.168.3.5", "port": 33333, "reachable": false, "reason": "timeout"},
   {"machine": "hs", "host": "homeserver", "port": 3333, "reachable": false, "reason": "refused"},
   {"machine": "hs", "host": "homeserver", "port": 33333, "reachable": false, "reason": "refused"}],
 "expected": "every fleet machine answers on 3333 and 33333",
 "message": "3 of 4 fleet probes unreachable: air:33333 timeout, hs:3333 refused, hs:33333 refused"}
```

`severity` is `warn`, so an unreachable machine never makes `cpd doctor` fail. Each probe has a 10 s deadline and all probes run at once. The reasons are `refused`, `unresolved`, `timeout`, `no_http_response` and `other`; the full rules are in `web/CONTRACT.md` §31.3 and §31.4.

## Not decided

Listed in `web/CONTRACT.md` §31.6: whether the engine also runs the probe, how a DNS name with several addresses is probed, and whether the 10 s deadline suits a real slow link.
