# B91 skills metadata backport

## Scope and baseline

This is the narrow skills-metadata contract for the deployed WebUI serving baseline. The serving roots' `web/src/server/durable/frames.ts` files are identical (SHA-256 `a7ff2926c9080f1e319e6ebda46eaf2e24dc77e0c092657561ea6cf33112a62c`). The intended change is limited to recognizing and validating the `cpd-skills` metadata baseline; it is not a replacement of the serving file with a newer full-file version. The newer main fix `20935ea42b3a8f6de62661dbf4e8a3de6b987f25` is not compatible as a full-file replacement because it also changes compaction and dialog behavior.

No activation, runtime, or transcript changes are part of this backport. Existing compaction and dialog behavior remains unchanged.

## Silent skills baseline

The durable producer writes the initial skills baseline as a `cpd-skills` entry with its ordinary identity and kind, an empty `model` array, and metadata in `data`:

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

Only this `cpd-skills` form is permitted to have no model message. The baseline is valid only when `data` is an object, `data.skill` is a string, `data.files` is an object, and every file digest is a 64-character lowercase hexadecimal string. An empty files object is valid under this validation. Malformed or missing metadata, or an invalid digest, is a protocol error. `model: []` is not a general allowance for empty models or malformed entries of other kinds.

A valid baseline is bookkeeping, not a chat message: omit it from transcript output, without synthesizing a user or assistant item. Continue to parse later entries in order.

## Skills-change notice

A skills-change `cpd-skills` entry has a message in `model[0]`; it is not a silent baseline. The message must be an object with role `user`. Its `content` must either be a string or an array in which every element is an object of type `text` with a string `text` value. Missing or empty `model[0]`, a non-object message or content block, a wrong role, or malformed content is a protocol error, never a reason to silently skip the entry. An empty content array is well-formed by this structural validation.

Render a valid notice as the existing `cpd-skills` fold: preserve its message text as the body, use the first line as the title (subject to the existing title-length limit), and keep the existing closed-by-default behavior. The producer's `{skill, files}` metadata remains entry data and is distinct from the visible message body. Do not reinterpret it as message content.

Malformed ordinary message-bearing entries (including `pi.user`, `pi.assistant`, and `pi.tool-result`) retain their normal strict message validation; this exception does not make them skippable. Adjacent valid chat entries remain in order, and malformed actual messages continue to fail with a protocol error.
