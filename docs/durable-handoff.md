# Same-session native transcript staging contract

## Status and boundary

This document specifies **staged conversion only**: validating a settled native coding-agent transcript and appending a durable representation while retaining the same CPD Session ID. It does not authorize or implement a live switch, production exposure, process control, database edits, archive/recreate, or ownership transfer. The native session remains authoritative and unmodified. A staged result is not evidence that durable may accept input.

The contract follows decisions pro-d622 and pro-d624. Durable has no documented coding-agent JSONL import API. This host-defined adapter uses only the public durable API and explicitly retains information that the durable conversation model cannot natively express. It makes no claim of runtime, UI, branch-operation, extension, or accounting equivalence.

## Public API and version boundary

Export `stageNativeConversation(input, sink): Promise<StageResult>` from `pi/durable-host/handoff.ts`; the durable host's existing `run.ts` staging entrypoint calls it. It must not be reachable as a public live-migration or ownership-switch command.

Compatible pins for this contract are `@earendil-works/pi-durable` **1.5.0**, its `pi-ai` **1.5.0**, and `@earendil-works/pi-coding-agent` **1.5.2** with its `pi-ai` **1.5.2**. Similar structural types do not establish compatibility across the two pi-ai versions. Before staging, the adapter must verify its actual package versions and validate every converted message against durable's installed public pi-ai 1.5.0 schema. It must use coding-agent's public `SessionManager`/`buildSessionProjection` and `convertToLlm` APIs for source parsing/projection/conversion; it must not import internals, cast across incompatible message types, or write either store directly. Any unsupported or version-incompatible shape fails closed.

The sink is an injected adapter to public durable APIs: one `Harness.root(context, { agent, init })` for a previously absent root and `Tx.appendEntry(conversationId, EntryDraft)` inside its `ConversationInit`. `EntryDraft` uses public `kind`, `model`, and JSON `data`; the API assigns durable IDs. Do not access SQLite, private storage, internal tables, or provider internals. Do not overwrite or initialize an existing conversation. Root initialization is one durable creation commit; use append return values and the in-memory mapping because reads during `init` are prohibited (`ReadAfterWrite`). The public adapter may report its actual installed versions and schema-validation outcome; it cannot claim a cross-version guarantee beyond validation of the concrete input.

## Exact typed boundary

The following TypeScript types define the serialized contract. `JsonValue` is recursive JSON, with no `undefined`, bigint, function, symbol, cycles, or non-finite numbers. The caller supplies an immutable snapshot; the staging implementation never edits it or the source file.

```ts
export type JsonPrimitive = null | boolean | number | string;
export type JsonValue = JsonPrimitive | readonly JsonValue[] | { readonly [key: string]: JsonValue };

export type NativeEntry = {
  readonly id: string;
  readonly parentId: string | null;
  readonly type: string;
  readonly timestamp: number;
  /** Exact parsed JSONL entry, with missing and explicit-null fields distinguished. */
  readonly raw: { readonly [key: string]: JsonValue };
};

export type NativeSnapshot = {
  readonly cpdSessionId: string;
  readonly sourcePath: string;
  /** Exact original bytes represented as base64, including line endings and final newline state. */
  readonly sourceBytesBase64: string;
  readonly sourceSha256: string;
  readonly header: { readonly [key: string]: JsonValue };
  readonly entries: readonly NativeEntry[]; // all non-header entries in source file order
  readonly leafId: string | null;
  readonly tree: readonly { readonly id: string; readonly parentId: string | null }[];
  readonly codingAgentVersion: "1.5.2";
  readonly codingAgentPiAiVersion: "1.5.2";
  readonly durableVersion: "1.5.0";
  readonly durablePiAiVersion: "1.5.0";
};

export type DurableModel = { readonly provider: string; readonly modelId: string };
export type StageRequest = {
  readonly input: NativeSnapshot;
  readonly sink: DurableStageSink;
};

export type StageResult =
  | { readonly status: "staged"; readonly cpdSessionId: string; readonly durableConversationId: string;
      readonly sourceSha256: string; readonly sourceToDurableEntryId: Readonly<Record<string, string>>;
      readonly activeProjectionSourceIds: readonly string[]; readonly leafSourceId: string | null;
      readonly importVersion: "cpd-native-transcript/1"; readonly stagedOnly: true }
  | { readonly status: "refused"; readonly cpdSessionId: string; readonly sourceSha256: string;
      readonly code: "invalid-snapshot" | "identity-mismatch" | "unsupported-entry" | "unsupported-extension" |
        "invalid-tree" | "invalid-projection" | "invalid-message" | "version-incompatible" |
        "sink-incompatible" | "root-exists" | "sink-failed";
      readonly message: string; readonly sourceEntryId?: string };

export type DurableStageSink = {
  readonly versions: {
    readonly durable: string;
    readonly durablePiAi: string;
    readonly codingAgent: string;
    readonly codingAgentPiAi: string;
  };
  /** True only for a new, absent root; an existing or ambiguous root must be refused. */
  rootAbsent(cpdSessionId: string): Promise<boolean>;
  /** Implemented with public Harness.root and its one-time ConversationInit. */
  createRoot(args: {
    readonly cpdSessionId: string;
    readonly agent: { readonly model: DurableModel; readonly thinkingLevel: string; readonly cwd: string };
    readonly init: (tx: PublicTx, conversationId: string) => Promise<void>;
  }): Promise<{ readonly conversationId: string }>;
};

/** Public durable Tx subset, not a private database interface. */
export type PublicTx = {
  appendEntry(conversationId: string, value: {
    readonly kind: string;
    readonly model?: readonly DurableMessage[];
    readonly data?: JsonValue;
  }): Promise<{ readonly id: string }>;
};
export type DurableMessage = object; // must be a value validated as pi-ai 1.5.0 Message at runtime; never cast to assert validity
```

`stageNativeConversation(input, sink)` takes the two `StageRequest` fields as positional arguments as shown by its signature: `stageNativeConversation(input: NativeSnapshot, sink: DurableStageSink): Promise<StageResult>`. The `sink` is part of the injectable public boundary for deterministic tests. Implementation catches and returns sink failures as `sink-failed`; it must not report `staged` until `createRoot` has committed successfully. A failed/ambiguous create is not retried against an existing root. The `sourceToDurableEntryId` result maps every source entry ID (including off-branch rows) to the durable ID of its provenance entry, never to an inferred model-message identity.

## Validation and complete provenance

Before any sink write, validate source byte digest, UTF-8/JSONL completeness, header and CPD identity association, exact supported versions, unique nonempty source IDs, parent/tree consistency, acyclicity, exact leaf, source order, timestamps and all relevant references. Require `tree` to agree with entries and parent links. Validate supported native entry schemas, projection, tool call/result linkage, converted durable messages and effective active model/thinking/cwd. Preserve JSON distinctions (including missing versus null). If byte-exact source bytes cannot be retained and verified, refuse.

Preserve every source entry in original file order, including off-branch and superseded history. Append one silent `cpd.native-entry` durable entry per source entry with `model: []` and `data` containing `importVersion`, `cpdSessionId`, source path, source SHA-256, original ordinal, source ID, source parent ID, timestamp, original type and the complete raw entry JSON. Append one silent `cpd.native-header` containing complete header, exact source bytes (base64), digest, leaf, tree and all four package versions. Append active model projection entries in coding-agent projection order as `cpd.native-projection`, with validated converted `model` messages and source-reference IDs in data. Finally append a silent `cpd.native-map` containing all source-to-provenance durable IDs, source parent mapping and leaf mapping. IDs returned by `appendEntry` are the only durable IDs in this mapping. All source-to-durable mapping and source facts are retained, not reconstructed from ordering later.

The root's initial agent settings are the validated *effective current* model, thinking level and cwd. Historic model/thinking change entries stay in provenance; do not replay them as durable changes. Do not substitute CPD Session ID into durable provider UUID identity: these identities have no documented equivalence. Retain both identities where public durable APIs expose them, and never claim that a seeded transcript preserves task state.

## Mapping and refusal policy

| Source entry | Staged representation / rules |
| --- | --- |
| `session` header | `cpd.native-header`; retain complete header, native ID, cwd, parent session, leaf/tree/version. Header is not a durable entry identity. |
| `message` user/assistant/toolResult | Active projection via public compaction/edit-aware projection and `convertToLlm`, preserving structured content, tool-call IDs, names, error flags, usage, stop reason and timestamps when accepted by pinned pi-ai. Raw row always preserved. Verify every tool call/result relationship; never rerun tools or synthesize diagnostics. |
| `message` bashExecution/custom or other custom AgentMessage | Accept only when public converter and the pinned durable message schema preserve its semantics and all referenced artifacts are portable/retained. Unknown role, transformer, filesystem-backed output, non-JSON state or unverifiable semantics: refuse. |
| `thinking_level_change`, `model_change` | Silent raw provenance. Only final validated effective settings configure the staged root. |
| `usage` | Silent raw provenance; does not seed or claim durable accounting. |
| `compaction` | Preserve complete raw summary, kept-entry reference, details, usage, hook/system data. Use coding-agent's verified projection for the active model context. Refuse if active system/tool state cannot be represented. Do not fabricate a durable native compaction event. |
| `branch_summary` | Preserve complete raw record and use public coding-agent conversion if present in active projection. Keep native parent/tree mapping separately. |
| `context_edit` | Preserve target and complete replacement/omission raw data; projection supplies active context. Do not claim native arbitrary tree behavior is recreated. |
| `custom_message` | Preserve complete raw custom type/display/details. Include in model projection only when the public converter and durable schema validate its model semantics. |
| `custom` | Preserve JSON raw state only. Refuse if a required extension relies on it and no explicitly compatible restoration adapter exists. |
| `label`, `session_info` | Preserve target, name and label in raw provenance. Do not claim durable UI automatically resolves native labels/names. |
| `cpd`, first-message, context custom markers | Preserve every exact payload. Mapping to durable `cpd-event`, `cpd-first-message`, or `cpd-context` requires the recognized native payload schema and exact delivery/event IDs. Do not infer cursor high-watermarks across missing event IDs. Staging alone does not mark delivery or advance a follower. |
| Any unknown, ambiguous, malformed, extension/system/tree-dependent or unsupported version entry | Refuse before writes; never silently omit or approximate. |

There is no native durable representation of arbitrary parent pointers, original entry IDs, branch navigation, arbitrary extension runtime state, or prior usage accounting. Provenance allows audit/recovery, not automatic rehydration. Full history retention does not establish equivalent future model requests or behavior.

## Staging is not cutover

This API does not fence sends, establish cooperative quiescence, stop or control a process, prove all accepted messages were delivered, transfer exclusive ownership, change the runner, or admit durable input. It must not mutate native JSONL, CPD Session ID, session ownership, native cursor, accepted-send ledger, or live durable conversation. No production caller may expose the staged root as active.

Before any later exposure, a separately specified and implemented protocol must establish cooperative quiescence at a settled idle point; gate all send paths before acknowledgement; account for each accepted send and in-flight delivery exactly once; persist a session-scoped ownership fence/epoch; verify source and staged projection/manifest; and transfer exclusive ownership with restart-safe recovery. Direct input, extensions, tools, retries, follower and native launcher paths must participate. An uncertain state must fail closed. This contract intentionally does not specify that protocol's implementation or authorize a switch.

## Test obligations for this contract

Tests inject a fake public sink/Tx with deterministic returned IDs and assert exact append order, same CPD ID, all source IDs mapped, complete raw provenance, active projection references, no source mutation, and no call when validation refuses. Cover a linear user→assistant→toolResult→assistant history, a recognized CPD marker, compaction/context-edit projection, off-branch provenance, malformed and cyclic trees, unknown/custom extension refusal, broken tool linkage, invalid JSON/nonportable message, wrong package/schema versions, existing root, and sink failure. Assert no internal database API is used and staged result never implies ownership transfer. Mock the public adapter and all external systems; do not test live migration or process control.
