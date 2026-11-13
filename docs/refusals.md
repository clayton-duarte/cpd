# cpd 2: refusal codes

A refusal is the one JSON error a command prints when it declines to act. Its envelope and exit status are set by the command's `error_codes!` entry (`core/src/error.rs`) and printed by `core/src/output.rs` (`emit`):

```json
{"ok": false, "error": {"code": "<code>", "message": "<text>", "details": {…}}}
```

The exit status of a refusal is 3. The per-command precondition tables are in [commands.md](commands.md); this page documents the codes that more than one command returns.

## `prereq_unmet`

| Field | Value |
|---|---|
| `code` | `prereq_unmet` |
| Status (exit) | refused, 3 |
| Returned by | `pr merge-green`, `pr merge`, `pr post`, `inbox approve` (the deploy approval; **open**: a deploy item links a run, not a Workflow, so which Workflow's prerequisites it checks is not yet decided) |
| Written | nothing: the refusal is returned before the command's first write, `gh` call or Slack call |

**Meaning.** The Workflow has one or more external prerequisites that are not met (declared with `cpd workflow prereq add`, [commands.md §9.5b](commands.md#95b-workflow-prereq)). The command names every unmet prerequisite, so the Lead or the User sees the full list in one refusal. It stays refused until each one is met: an `ld-flag` prerequisite is met when its LaunchDarkly flag exists, a `manual` prerequisite once the User runs `cpd workflow prereq confirm`.

**`details`:**

| Field | Type | Meaning |
|---|---|---|
| `plan` | string | the Plan id of the Workflow |
| `workflow` | string | the Workflow key the prerequisites belong to |
| `command` | string | the command refused: `pr merge-green`, `pr merge`, `pr post` or `inbox approve` |
| `unmet` | array | one object per unmet prerequisite, in key order |

Each `unmet` entry:

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | the prerequisite's id, as `cpd workflow prereq add` printed it |
| `kind` | string | `ld-flag` or `manual` |
| `project`, `key`, `env` | string or null | `ld-flag` only: the LaunchDarkly project, flag key and environment (`env` null when not given) |
| `text` | string | `manual` only: the task |
| `cause` | string or absent | present when an `ld-flag` could not be checked. The values are **not yet decided** (proposed: `unreachable`, `no_token`). The prerequisite is still unmet: a failed check never lets a change through |

**Example** (the `message` wording and the field values are **illustrative**; the field names and shape are the contract):

```json
{
  "ok": false,
  "error": {
    "code": "prereq_unmet",
    "message": "api: 2 external prerequisites unmet, so cpd pr merge cannot run: ld-flag missionlane/opportunity-tiles-takeover; manual: Create the flag in staging",
    "details": {
      "plan": "pay-4521",
      "workflow": "api",
      "command": "pr merge",
      "unmet": [
        {"id": 1, "kind": "ld-flag", "project": "missionlane", "key": "opportunity-tiles-takeover", "env": null},
        {"id": 2, "kind": "manual", "text": "Create the flag in staging"}
      ]
    }
  }
}
```

**Idempotent.** Yes. A refused command writes nothing, so running it again while the prerequisites stay unmet gives the same refusal. Once all are met, the same command proceeds as it does without prerequisites.

**Not `prereq_unmet`.** An `ld-flag` check that cannot run is still `prereq_unmet` with a `cause`, not a separate code, so no caller has to tell "failed to check" from "unmet" to stay closed.

## Also in this page

Refusal codes are added here when a second command returns them. `prereq_unmet` is the first.
