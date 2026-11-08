# Tier J verification — Job `j-verify`, attempt `j-verify-a1`

Scratch CPD_HOME verification of Tier letter J, the decide role at JX, and the J model key
(`openrouter/typesafe/jev-router`). No product behaviour changed; this file is the transcript.

Status: in progress.

| Check | Command | Expected | Actual | Verdict |
|---|---|---|---|---|
| Build | `timeout 900 scripts/cargo.sh build --workspace` | exit 0 | (pending) | — |
| `tiers` lists J | `CPD_HOME=$tmp target/debug/cpd tiers` | J row with efforts, decide role at JX | (pending) | — |
| `config tiers set --decide JX` | `CPD_HOME=$tmp target/debug/cpd config tiers set --worker H3 --lead S3 --manager O3 --distill G1 --decide JX` | accepted, written to `$tmp/config.toml` | (pending) | — |
| Read back | read `$tmp/config.toml` | decide key = JX | (pending) | — |
| `--decide J4` refused | `CPD_HOME=$tmp target/debug/cpd config tiers set --decide J4` | refused, names efforts J offers | (pending) | — |
