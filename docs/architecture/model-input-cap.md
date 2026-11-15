# The model input cap

Decision **pro-d559** (a Context decision of the User) sets a hard ceiling on the input every model request carries. This note is the detail behind the contract: the formula, the dispatch guard, the metadata rule, the integration points and the tests the implementation must have. The normative rule is [contract §6.23](../contract.md); where the two disagree, the contract wins and this note is fixed.

Status: **decided, not implemented.** No code enforces the cap yet. Every claim about pi's or pi-durable's internals is **unverified** until the implementation Job reads and runs it; the open points are in §9.

## 1. The cap

```
cap = min(200000, floor(0.8 × W))        W = the active model's context window, in tokens
```

`floor(0.8 × W)` is computed in integers as `floor(W × 4 / 5)`, so no float rounding can move the boundary.

| W (tokens) | 0.8 × W | cap |
|---|---|---|
| 8 192 | 6 553.6 | 6 553 |
| 128 000 | 102 400 | 102 400 |
| 200 000 | 160 000 | 160 000 |
| 250 000 | 200 000 | 200 000 |
| 250 001 | 200 000.8 | 200 000 |
| 1 000 000 | 800 000 | 200 000 |

The 200 000 ceiling binds for every window above 250 000. The cap is a **ceiling, not a target**: a request under the cap is sent unchanged.

Both constants (`200000`, `0.8`) are fixed in code. Neither is a config key: pro-d559 names no key, and a config key would be an option nobody asked for (AGENTS: configure only real deviations). Changing them is a decision, not a setting.

## 2. What is measured

The cap bounds the **full effective model input** of one request:

- the system prompt, including every cpd section (`cpd-context-files`, `cpd-policy`, `cpd-time-zone`);
- the tool schemas of every tool the request offers;
- the messages, as they would be sent (the rebuilt context, after any compaction);
- the tool results inside those messages.

The cap has **no output term**. I infer from the formula that the response budget is not reserved by this rule; §9 asks the User to confirm.

The token count comes from a counter the implementation Job chooses (§9). This note does not choose it.

## 3. The dispatch guard

The guard runs before every model request of a Session (Lead, Manager, Worker). In order:

1. Resolve the active model (the policy's `[model]` entry for the Session's current Tier, §8.3, §9) and its window `W` (§5). An invalid `W` stops here, with no count and no request.
2. Measure the input (§2). If `tokens ≤ cap`, dispatch.
3. Otherwise **compact** (§6.3): the explicit compaction the guard asks for, not left to pi's threshold trigger.
4. Re-measure the rebuilt input. If `tokens ≤ cap`, dispatch.
5. Otherwise **refuse**: fail with `input_over_cap` (contract §1.2, exit 3). **No model request is sent.**

One compaction per request. A second pass is a loop, and the guard does not loop.

## 4. Model switches

The cap is a function of the model **at dispatch**. It is never cached across a switch.

- Every switch recalculates it, **downswitches included**. `session retier` (contract §9, `session-retiered`) and `cpd preset apply` (`session-model-changed`, §5.2) both change the model of a live Session.
- A downswitch can leave a transcript that fitted the old cap over the new one. The next dispatch then compacts or refuses. Example: a Session at 300 000 input tokens on a 1 000 000-token model (cap 200 000) switches to a 128 000-token model (cap 102 400). Its next request compacts; if the compacted input is still over 102 400, it is refused.

## 5. Context window metadata

`W` is the model's `contextWindow` in pi's model catalog, read for the model in hand. The rule:

- `W` must be a finite positive integer.
- A missing, zero, negative, non-integer, `NaN` or infinite `W` is `context_window_invalid` (contract §1.2, exit 1), with `details` `{model, value}`. The guard sends no request.
- **No fallback.** Neither a default window nor pi's own value replaces a bad `W`. This is stricter than §6.18's compaction ratio, which keeps pi's value when `contextWindow` is absent (commands §15.19). That fallback still governs pi's compaction trigger. It does not reach the cap (§6.3).

Static models need their own `contextWindow` in pi's `models.json`: a static entry replaces the package's entry of the same id and takes its defaults, which include a 128K context (research note `docs/research/bifrost-cpd.md`, recorded there as verified earlier and not re-checked here). A static entry without a real window would fail loud under this rule, as it should.

## 6. Integration points

### 6.1 The durable host (contract §6.13)

`pi/durable-host/` is a headless host over pi-durable. The guard sits in front of its submit path, the one that starts a model request. The host already passes the live compaction settings to pi-durable (contract §6.18, `compactionPolicy(source)`), so compaction decisions read the model in hand.

What the host does not yet expose is the exact input pi-durable will send. The measurement in §2 needs it. **Unverified:** whether pi-durable 1.5.0 offers that input before the request, or only after. §9.

### 6.2 The interactive harness (pi, the adapter)

The adapter runs in pi's process (contract §6.6). The guard must run before pi's provider request. **Unverified:** the name and timing of pi 1.3's pre-request hook. §9.

Pi's own threshold compaction is **idle-only** (commands §15.19, pro-d353): it is deferred while a turn runs or messages are queued. So the guard cannot wait for the threshold to fire. It compacts explicitly, as step 3 of §3 says. The guard's compaction and pi's threshold compaction are two triggers of one pi compaction path; the implementation Job makes sure they do not both run for one request.

### 6.3 Production Manager compaction threshold (contract §6.18, commands §15.19)

The decided production Manager contract, including for a durable Manager host, is `threshold = min(200000, floor(0.8 * W))` and `reserve = W - threshold`, where `W` is the active model's available context window in tokens. Compaction is triggered when used context exceeds the threshold. `W` must be present and a finite positive integer; missing or invalid metadata is invalid input and must be rejected at the boundary. No threshold or reserve is derived from invalid metadata, and no fallback window is substituted.

This threshold contract is distinct from the model-input cap described in this note. It does not assert incident attribution or runtime deployment, and this note makes no claim about the production trigger's implementation or which path runs first.

### 6.4 Auxiliary runs

The distiller (contract §4.6) and the title writer (contract §3.7 `session-titles`) run `pi --print` with no adapter. Whether they are model dispatches under this rule is **not decided** (§9).

## 7. Errors and their channel

| Code | Exit | When | `details` |
|---|---|---|---|
| `input_over_cap` | 3 | the input is over the cap after compaction; no request sent | `{model, window, cap, tokens}` |
| `context_window_invalid` | 1 | `W` is missing or not a finite positive integer; no request sent | `{model, value}` |

The code is the same everywhere. The channel depends on the caller: a command returns it as its failure (contract §1.2); an engine or Session caller records it as an `error` Event (contract §5.2) carrying the same code, and the turn does not start. The exact channel for each caller is an open point (§9).

## 8. Test requirements

Tests cover only the code this rule adds. The model, the token counter and the catalog are mocks. No test calls a real model, the network, the real clock or the system zone, and no test sleeps.

Each test below must fail first, for the right reason, before the code it pins is written.

1. **Formula.** The table in §1, including the boundaries `W = 250 000` (cap 200 000), `W = 250 001` (cap 200 000), `W = 1` and `W = 8 192`. Integer rounding is pinned by `W = 8 192` (6 553, not 6 554).
2. **Invalid window.** Each of: missing, `0`, negative, `1.5`, `NaN`, `Infinity`, a string. Each yields `context_window_invalid`, and the model mock is called **zero** times.
3. **Under the cap.** Input at or below the cap: one dispatch, zero compactions.
4. **Compact then fit.** Input over the cap, compaction brings it under: one compaction, then one dispatch with the compacted input.
5. **Compact and still over.** Compaction does not bring it under: `input_over_cap`, zero model calls, exactly one compaction attempt.
6. **Downswitch.** Dispatch at `W = 1 000 000` with 300 000 tokens, then switch to `W = 128 000`: the next dispatch uses cap 102 400, not 200 000.
7. **Stored transcript.** After a compaction, the stored transcript is unchanged, entry for entry. Compaction changes only the request built from it.
8. **One source.** The host and the harness call the same guard function. A test feeds each the same inputs and gets the same verdict.
9. **Ratio independence.** `reserveTokensOf` returns the same value with and without the guard.

Each test pins the contract (the verdict and the refusal), not the mechanism, so a refactor of the guard breaks no test.

## 9. Open points

These are not decided by pro-d559. Each needs an answer before the implementation Job starts, or the Job stops and asks the Lead:

1. **Token counter.** Which counter counts the input: the provider's count, a local tokenizer, or an estimate? A local estimate can be under the real count, and then the cap is not a cap.
2. **Hook points.** The pre-request hook of pi 1.3 and the input pi-durable 1.5.0 exposes before a request. Both unverified (§6.1, §6.2).
3. **Auxiliary runs.** Are the distiller and the title writer model dispatches under this rule? They run with no adapter (§6.4).
4. **Output budget.** Does the cap reserve room for the response? The formula says no (§2).
5. **Error channel per caller.** Which of a command's failure, an `error` Event, or both each caller uses (§7).
6. **Ratio fallback.** §5 says the cap fails loud on an absent window while §6.18 keeps pi's value for the compaction trigger. Confirm that both stand.
