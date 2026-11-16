# Architecture references

The [glossary](../../CONTEXT.md) defines terms; the [contract](../contract.md)
defines behavior; the [command reference](../commands.md) defines operations.

Architecture pages record designs and scenarios, not independent product
requirements. In particular, [CPD as Core + pi Extension + WebSocket
Clients](pi-native-core.md) is an approved target design; its migration and
runtime claims should not be read as proof that each target is implemented.
ADRs preserve decisions at their recorded status. Where an ADR is marked
superseded, follow its successor; do not treat the historical decision text as
current guidance.

## Design and operational references

- [cpd doctor cross-machine probing](doctor-cross-machine.md)
- [The model input cap](model-input-cap.md)
- [CLI and WebUI argv parity](parity.md)
- [Architecture Design: CPD as Core + pi Extension + WebSocket Clients](pi-native-core.md)
- [PR lifecycle scenario](pr-lifecycle-scenario.md)
- [Store repair: architecture](store-repair.md)
- [SWR for the classifier: the Lead completion badge and endpoint health](swr-classifier.md)
- [SWR: the GitHub datasource](swr-github.md)
- [Stale-while-revalidate (SWR): the Jira datasource](swr-jira.md)
- [SWR: the models datasource (`swr_models`)](swr-models.md)
- [Stale-while-revalidate: the Slack datasource (`slack`)](swr-slack.md)
- [Stale-while-revalidate (SWR)](swr.md)
- [cpd upgrade: isolated build, dry run, swap and rollback](upgrade.md)
- [WebUI access](webui-access.md)

## Historical evidence

- [End-game design and unresolved historical gaps](../research/endgame-design.md)
- [WebUI implementation audit at b8021cd90](../research/webui-implementation-audit-b8021cd90.md)

These reports do not define current product behavior or certify the installed
runtime. The core contract and access policy above govern new work.
