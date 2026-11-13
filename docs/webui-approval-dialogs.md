# WebUI Approval Dialogs

## Intent

The WebUI backend receives approval dialogs from the durable host via `ui_request` frames with purposes `secret_approve`, `host_approve`, and `external_approve`. These are fail-closed approval dialogs that the User must answer before proceeding.

## Behavior

### Mapping to interactive dialogs

The backend's `dialogFromRequest` function maps these approval requests to interactive `ask_user_question` dialogs:

- `ui_request` with `method: "select"`, `purpose: "secret_approve"`, `"host_approve"`, or `"external_approve"`, and `options: ["Allow once", "Deny"]`
- Maps to: `Dialog` with `method: "ask_user_question"`, `interactive: true`, one question with `header: "Approval"`, `question` = the payload `title`, options `"Allow once"` and `"Deny"`

### Responding to approval dialogs

When the User answers an approval dialog, the backend's `respond` handler must:

1. Identify that the dialog is an approval based on its `ui_request.purpose` (not from the title or question text)
2. Send the response to the host as `ui_response {id, value: "Allow once"|"Deny"}` (not `{answer: ...}`)
3. Map the User's choice to the approval value:
   - User selects "Allow once" (and it is an option with `kind: "option"`) → send `{value: "Allow once"}`
   - All other outcomes (Deny, custom answer, unanswered, cancelled) → send `{value: "Deny"}` (fail closed)

## Purposes

### secret_approve

User approves a secret access action. Defined in [docs/contract.md §6.15](contract.md).

### host_approve

User approves a cpd checkout repair. Defined in [docs/contract.md §6.15](contract.md).

### external_approve

User approves an external write (PR reply, PR post, Slack send). Defined in [docs/contract.md §6.15](contract.md) and [docs/commands.md §15.27](commands.md).

## Implementation

The WebUI backend's durable dialect (web/src/server/durable/backend.ts and frames.ts) must:

1. Keep an `approvals` Set to track which dialog ids are approval dialogs (by `ui_request.purpose`)
2. Add all approval purposes (`secret_approve`, `host_approve`, `external_approve`) to this set when creating dialogs
3. Use this set in the `respond` handler to choose the response format

The `respond` handler checks: if the dialog is in the `approvals` set, send `{value: ...}`; otherwise send `{answer: ...}`.

See web/CONTRACT.md section 3.8 for the precise contract.
