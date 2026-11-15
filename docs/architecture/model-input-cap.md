# Model input cap

The [core contract](../contract.md#623-the-model-input-cap-pro-d559) owns the
input-cap rule. It applies to **every CPD-owned model call**, including agent
sessions, curation, title writing and classification (User, 2026-11-14).
Implementation conformance is unverified.

## Measurement and dispatch

Measure the full effective input: system prompt, tool schemas, messages and
tool results, after any context reconstruction. The cap is an input bound;
output is outside its formula. The active model's context window must be a
finite positive integer. Missing or invalid metadata refuses dispatch without
a fallback.

The counter must establish that input does not exceed the cap. A counter,
pre-request hook or provider integration that cannot establish the bound does
not satisfy the requirement. The implementation may choose the counting and
hook mechanisms; it must not describe an unverified estimate as a hard bound.

For a Session, try one compaction when input exceeds the cap, then remeasure.
If it still exceeds the cap, refuse without sending a request. Preserve the
stored transcript. A one-shot auxiliary call with no compactable transcript
must fit before dispatch or be refused; it does not gain an exception.

Recalculate for the selected model at every dispatch, including after switches.
Existing harness compaction thresholds remain separate from the hard cap.

## Failure and evidence

`input_over_cap` reports the model, window, cap and measured token count.
`context_window_invalid` reports the model and invalid metadata. A command
returns its failure envelope; an engine or Session records an error and leaves
the request unsent. A failed dispatch must not look like a completed model call.

## Acceptance

- Input at the cap dispatches unchanged; input one token above it compacts once
  or refuses. Integer rounding follows the contract formula.
- Compaction that fits dispatches once. Compaction that does not fit sends no
  model request and preserves the stored transcript.
- Switching to a smaller-window model uses the new cap, not the old one.
- Invalid window metadata sends no request.
- Each auxiliary caller obeys the same bound and refusal behavior as a Session.

Tests use injected counters and model boundaries; no real model call is needed.
