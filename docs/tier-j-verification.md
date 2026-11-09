# Tier J verification — Job `j-verify`, attempt `j-verify-a1`

Scratch CPD_HOME verification that the built cpd accepts Tier letter J, the decide role at JX and the
J model key, and refuses an effort the Jev model does not offer. No product behaviour changed; this
file is the only output of the Job. Run 2026-11-08 on this machine; binary `target/debug/cpd`,
workspace build from this worktree (tier-j-impl-core and tier-j-impl-ui merged).

## Setup

```bash
timeout 900 scripts/cargo.sh build --workspace
tmp=$(mktemp -d) && export CPD_HOME=$tmp
mkdir -p $tmp/policy
cat > $tmp/policy/host.toml <<'EOF'
[model]
O = "openrouter/deepseek/deepseek-v4-pro"
S = "openrouter/deepseek/deepseek-v4.1-flash"
H = "openrouter/deepseek/deepseek-v4.1-flash"
G = "openrouter/deepseek/deepseek-v4.1-flash"
J = "openrouter/typesafe/jev-router"
EOF
printf 'version = 1\nmachine = "hs"\n' > $tmp/config.toml
```

The binary demanded `$CPD_HOME/config.toml` first (`config_missing`, then `config_invalid` asking for
`version`); the two-line `version = 1` / `machine = "hs"` file above satisfied it. All five `[model]`
keys are one provider (`openrouter`), per the one-provider rule.

## Checks

| # | Check | Command (in the scratch `CPD_HOME`) | Expected | Actual output (verbatim, exit code) | Verdict |
|---|---|---|---|---|---|
| 1 | Build | `timeout 900 scripts/cargo.sh build --workspace` | exit 0 | `Finished \`dev\` profile [unoptimized] target(s) in 0.72s` — exit 0 | pass |
| 2 | `tiers` lists the J row | `timeout 60 target/debug/cpd tiers` | a J row with its efforts | `{"ok":true,"pi_version":"1.3.0","tiers":{"G":{"efforts":[0,1,3],"model":"openrouter/deepseek/deepseek-v4.1-flash"},"H":{"efforts":[0,1,3],"model":"openrouter/deepseek/deepseek-v4.1-flash"},"J":{"efforts":[0,1,2,3],"model":"openrouter/typesafe/jev-router"},"O":{"efforts":[0,3,4],"model":"openrouter/deepseek/deepseek-v4-pro"},"S":{"efforts":[0,1,3],"model":"openrouter/deepseek/deepseek-v4.1-flash"}}}` — exit 0. The J row is `openrouter/typesafe/jev-router` with efforts `[0,1,2,3]`. `tiers` prints no roles (its output shape is per-letter model + efforts, §14.3); the decide role's value is checked by 3, 4 and 5 below. | pass |
| 3 | Land `decide_tier` in the file (JX is the default, so the spec's command alone would not write it) | `timeout 60 target/debug/cpd config tiers set --decide J0 --auxiliar G1 --worker H3 --lead S3 --manager O3` | accepted, `decide_tier: "J0"` in the returned config and `changed` includes `decide_tier` | `{"ok":true,"already":false,"changed":["decide_tier","context_distill_tier","lead_tier","manager_tier"],"config":{…,"context_distill_tier":"G1","decide_tier":"J0",…}}` — exit 0 | pass |
| 4 | `config tiers set` accepts `--decide JX` | `timeout 60 target/debug/cpd config tiers set --decide JX --auxiliar G1 --worker H3 --lead S3 --manager O3` (flags as `config tiers set --help` shows; there is no `--distill` flag — the auxiliar role's flag is `--auxiliar`) | accepted, `decide_tier` back at JX | `{"ok": true, "already": false, "changed": ["decide_tier"], "decide_tier": "JX"}` — exit 0 | pass |
| 5 | The decide value is readable back from the scratch `config.toml` | `cat $tmp/config.toml` and `timeout 60 target/debug/cpd config` | the file holds `decide_tier = "JX"`; `config` reports it with source `file` | file (verbatim): `version = 1` / `machine = "hs"` / `decide_tier = "JX" # set by the Agent Tiers dialog, 2026-11-08` / `context_distill_tier = "G1" # …` / `lead_tier = "S3" # …` / `manager_tier = "O3" # …`. (`worker_tier = "H3"` is not written: H3 is the default.) `config` printed `{"ok": true, "decide_tier": "JX", "source": "file"}` (filtered by jq-style extraction of the full output's `config.decide_tier` and `sources.decide_tier`) — exit 0 | pass |
| 6 | `--decide J4` refused, naming the efforts J offers | `timeout 60 target/debug/cpd config tiers set --decide J4 --auxiliar G1 --worker H3 --lead S3 --manager O3` | `tier_invalid` (exit 2), message names J's efforts | `{"ok":false,"error":{"code":"tier_invalid","message":"Tier J4 is not valid: openrouter/typesafe/jev-router supports efforts 0,1,2,3.","details":{"efforts":[0,1,2,3],"key":"decide_tier","model":"openrouter/typesafe/jev-router","tier":"J4"}}}` — exit 2 | pass |

## Verdict

Every check passes: the built cpd accepts Tier letter J (row `J`, model `openrouter/typesafe/jev-router`,
efforts 0, 1, 2, 3), the decide role at JX (accepted, written, read back with source `file`), and refuses
`J4` with the efforts J offers named in both the message and `details.efforts`. The scratch CPD_HOME was
removed after the run (`rm -rf $tmp`); the machine's own `$CPD_HOME` was not touched. No product code,
config or policy file changed.

One deviation from the Spec's step (4) wording, recorded here: the Spec wrote `--distill G1`, and the
command's own `--help` shows the flag is `--auxiliar` (`--decide --auxiliar --worker --lead --manager`,
all five required); `--auxiliar G1` was used. Also, because `JX` is the compiled default of
`decide_tier`, the spec's set alone does not write the key into `config.toml`; check 3 first set
`--decide J0` so check 4's rewrite of that line gives the literal read-back the acceptance asks for.
