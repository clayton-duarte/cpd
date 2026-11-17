# Durable `cpd-skills` entry rendering

This contract applies to durable protocol 2 entries in a `view`, `ops` result, or history page. `cpd-skills` has two distinct forms: a silent skills baseline is metadata, while a displayed skills-change notice carries a user message. Do not decode the baseline as a model message.

## Silent baseline metadata

The durable worker writes the initial baseline intentionally as an entry of this shape:

```json
{
  "kind": "cpd-skills",
  "model": [],
  "data": {
    "skill": "cpd-lead",
    "files": { "SKILL.md": "<64 lowercase hexadecimal SHA-256 characters>" }
  }
}
```

`model: []` is valid for this `cpd-skills` metadata form only; it is not a missing or malformed chat message. The durable transcript renderer must recognize this form before reading `model[0]`. It contributes no transcript item and no visible message, and it must not create a synthetic user or assistant message. The `data` payload remains the entry's skill name and file digest map; it is bookkeeping consumed by the durable worker as the baseline for later change checks, not message content or fold details.

The entry still needs its ordinary entry identity (`id`) and kind. The metadata payload has the `skill` and `files` keys: `skill` is a string and `files` is the digest map (relative file paths to SHA-256 strings). An empty model does not make unrelated entry kinds valid and must not be treated as a general permission to skip malformed entries.

## Displayed change notice

When a skill changes, the durable worker appends a `cpd-skills` entry with a non-empty model containing the notice message and the same metadata payload (`data.skill` and the updated `data.files`). This is a message, not a baseline. Preserve and render its message content as the `cpd-skills` fold body; derive the fold title from the first line of that body, and retain the existing closed-by-default fold behavior. Keep the `data` payload intact and distinct from the message body: it records the skill and the new baseline, rather than replacing or rewriting the displayed notice.

## Malformed entries and adjacent messages

The empty-model exception is narrowly limited to the valid `cpd-skills` baseline form above. A `cpd-skills` entry that purports to carry a message must contain a well-formed message in `model[0]` with the expected user role and valid content; a missing, empty, non-object, or wrong-role message in a message-bearing entry is a protocol error, not a baseline to silently drop. Likewise, a `pi.user`, `pi.assistant`, `pi.tool-result`, or other message-bearing entry with an empty or malformed model remains a protocol error under that kind's normal message contract.

Skip only a valid silent baseline as transcript output. Continue parsing adjacent valid chat entries in entry order, and continue to reject malformed actual messages; do not make the transcript more lenient by skipping errors or changing transcript history.

## Source and intent

`pi/durable-host/skills.ts` writes the initial entry with `model: []` and `{skill, files}` in `data`; on change it writes a notice message plus the updated metadata. This distinction preserves both the worker's baseline semantics and the user's visible notice without interpreting metadata as model-authored conversation.
