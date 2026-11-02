# Web-socket audit measurements

Raw numbers measured for the web-socket audit, each produced by a command run now on hs.
Every section states the exact command, the raw output (or a faithful excerpt), the run count,
and the time in America/New_York with the zone. Anything not measured is under `Not measured`
with the reason. No message was sent to and no `submit` made into any live Session; the
durable-smoke Session (`1c2bf5cb-4fba-45d6-ba15-b08aa414616e`) was only read through its host
socket (`hello` + `attach` + `ping`, all read-only) and its state is unchanged.

Machine: hs. `$CPD_HOME` default `~/.cpd` (`/home/clayton/.cpd`).

## Skeleton — sections to be filled by measurements 1–5

1. `hello`+`attach` handshake cost and attach-to-first-frame latency over the durable host socket — TODO
2. Per-event push latency vs the 1 s poll — TODO
3. Snapshot size of a long Session — TODO
4. CLI process-spawn cost vs a socket round-trip per send — TODO
5. The frames route's poll behaviour — TODO

## Not measured

- (to be filled as measurements land)
