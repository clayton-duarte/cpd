# cpd

## Use cpd

Prerequisites (from the dotfiles): this repo at `~/projects/cpd`, `cpd` on PATH, and `~/.cpd/config.json`.

One-time global install: add these entries to `packages` in `~/.pi/agent/settings.json`:

```json
"packages": [
  "../../projects/cpd/pi",
  "npm:@plannotator/pi-extension@0.28.17",
  "npm:@juicesharp/rpiv-ask-user-question@2.13.8"
]
```

The first is relative to the settings file, so it works for any home directory.

Start any pi session in a herdr pane; it is a Lead. `CPD_HOME` defaults to `~/.cpd`.
Workers and Reviewers that cpd starts get `--no-skills` and `--exclude-tools` for every Lead-only tool, so they are not Leads.
