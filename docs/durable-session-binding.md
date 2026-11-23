# Trusted Session transcript staging boundary

**Status:** design contract, not implemented. It specifies a proposed private integration. It does not authorize live cutover, native transcript mutation, deployment, or a public import API. Implementation requires approval of this ownership/fence design and its private transport. No existing staging helper satisfies this contract.

## Authority and goal

A durable transcript may be staged only from the exact native file recorded in the authoritative CPD `sessions.file` field for the requested existing Session. The caller's Session ID, a filename, a pi header ID, and a native `delivered_to` marker are not proof of ownership. Snapshot validation and source ownership are separate checks: valid JSONL and a matching digest do not establish which CPD Session owns the bytes.

The boundary is staged-only: it creates a durable root under the same CPD Session ID and records source provenance. It does not activate the root, deliver queued sends, move a runner, stop a native process, edit native JSONL, or rewrite/delete/update historical Attempts. Historical Attempt records remain facts about their original runs, including forks and failures.

## Callable integration and private transport

The core entry point is private to the session domain, not a CLI/RPC command:

```rust
pub(super) fn stage_native(args: StageNativeArgs) -> Result<StageResult, CpdError>;

pub(super) struct StageNativeArgs {
    pub session_id: SessionId,
}

// Private implementation result; no caller-supplied snapshot or path is accepted.
enum StageResult { Staged { conversation_id: String, source_sha256: String }, Refused { code: StageRefusal } }
```

Core loads the row itself by `session_id`; callers cannot pass a `Row`, `file`, fork claim, digest, or “verified” boolean. The typed private core value, constructible only after the acquisition checks below, is conceptually:

```rust
struct VerifiedNativeSnapshot {
    cpd_session_id: SessionId,
    recorded_file: PathBuf,
    source_bytes: Vec<u8>,
    source_sha256: [u8; 32],
    forked_from: Option<SessionId>,
}
```

This value is never deserialized from a caller request. Core owns it and its lock guards. It is transferred once over a **private one-shot child stdio channel** to the paused staging host: core creates/owns the child and its piped stdin/stdout, writes one length-delimited request, then reads exactly one length-delimited reply. No public socket, host RPC, general-purpose import command, caller-selected executable/path, inherited ambient stdin/stdout, or caller-provided snapshot is part of this API. The child is the already configured trusted staging host launched by the owning runtime; transport setup must fail closed if that owned child/channel cannot be established. The host gets no native-file path access requirement: the channel carries bytes, not an instruction to reopen a path.

Request schema (versioned, strict, single message):

```ts
type StageVerifiedNativeRequest = {
  protocol: "cpd.stage-verified-native/1";
  cpdSessionId: string;                 // canonical CPD Session ID from the loaded row
  sourceSha256: string;                 // lowercase SHA-256 of sourceBytes
  sourceBytesBase64: string;            // exact bytes read by core
  forkedFrom: string | null;            // row association, not inferred from transcript
};
```

The trusted adapter decodes the bytes, computes SHA-256 itself, checks the digest, strict UTF-8/JSONL completeness and schema, then derives the header, ordered entries, tree, leaf and projection from those bytes. The TS staging seam is private to the host module:

```ts
stageVerifiedNativeSnapshot(
  input: VerifiedNativeSnapshot,
  sink: DurableStageSink,
): Promise<StageResult>
```

`VerifiedNativeSnapshot` is a module-private, nominal capability (private symbol/constructor); only the one-shot channel decoder may create it. It is not the current public `NativeSnapshot` shape and is not constructible from a TypeScript caller's asserted IDs/path/header. `stageNativeConversation(input: NativeSnapshot, sink)` remains an untrusted conversion helper and must not be treated as the trusted acquisition API or wired as an ownership proof. The host validates association before `rootAbsent`, `createRoot`, or any `appendEntry`. It uses the received `cpdSessionId` for the root and provenance, preserving exact source bytes/digest and fork provenance; it stages no other session.

Reply is exactly one of `{ protocol: "cpd.stage-verified-native/1", status: "staged", cpdSessionId, sourceSha256, durableConversationId }` or `{ protocol: "cpd.stage-verified-native/1", status: "refused", cpdSessionId, sourceSha256, code }`. Validate protocol, status, canonical session ID and digest against the outstanding request. EOF, malformed/extra data, child exit, mismatched reply, or lost connection is an indeterminate outcome, not success. No second request is multiplexed over this channel.

## Acquisition, identity and fence lifetime

1. Core resolves the requested existing Session row from the Store. It requires a non-null `row.file`; it does not use `pi::session_file`, derive a filename from an ID, search nearby files, or silently substitute another path. Missing row, null file, stale/nonexistent path, unreadable file, invalid/non-regular source, or inability to acquire required locks refuses before starting the host or touching the durable sink.
2. Under the Session fence described below, core obtains the native run lock for that same Session and requires it to be immediately free. It does not wait behind a live native run: busy means refuse. The Session fence prevents a new run/fork/send or `sessions.file` reassignment while acquisition and staging are in progress. Core re-reads the row under the fence and verifies its ID, exact `file`, and `forked_from` still equal the initially resolved association before reading. It reads the exact recorded file once into immutable bytes and hashes those bytes; the host never reopens or rereads the native source.
3. The Session fence and native run lock remain owned by core across snapshot validation, private request/reply, host conversion, durable root creation, all sink appends/commit, and receipt validation. They are released only after a determinate staged/refused reply, or after core has classified a failure as indeterminate and prevented automatic retry. RAII/drop releases locks on ordinary error or process death; ambiguous sink completion still requires reconciliation, not a blind retry. Root-exists or any sink failure is refusal; after an uncertain commit, do not retry until the durable root is inspected through an authorized recovery path.
4. Hash verification binds the exact bytes sent and staged. It does **not** replace the row/file ownership proof. A matching digest over caller-supplied bytes, matching filename, root-absent result, or valid native header cannot acquire the capability.

The existing `pi-<session>.lock` is a file-description-scoped exclusive `flock` used by `session run` and held while native pi executes. Reuse that same lock file/lock primitive to exclude an already running native process during staging; do not create an unrelated lock filename and assume it coordinates. Staging uses nonblocking acquisition and refuses if busy. The existing lock alone is insufficient: it does not fence accepted sends or all row/file/fork changes.

A new per-Session fence is therefore a prerequisite, not an optional optimization. All relevant writers must participate in a documented consistent order: acquire the Session fence before touching native run lock or changing the session association. `session run`/fork acquisition and `sessions.file` assignment, accepted `session send` commit/wakeup, and fork creation must hold the fence over their association-changing/acceptance transaction (and the run path must coordinate with the native lock). Staging takes the fence first, then the existing pi lock. Writers use the same order; no participant may wait while holding the pi lock for a fence staging is waiting on. The fence serializes staging against accepted sends/fork/reassignment; the pi lock excludes a live native writer. If this complete fence cannot be added atomically to every relevant path, staging refuses/not implemented—reading an “idle” row or holding only the pi lock is not a substitute. Lock acquisition errors, inconsistent order, Store failures, or fence contention all fail closed with no sink call.

## Matching, forks and refusal rules

The matching claim is established by core lookup of the requested CPD Session row plus exact use of its non-null recorded `row.file` while the association and writers are fenced. The native header UUID is parsed and retained as transcript data, but is not required to equal or treated as a CPD Session ID: these identity spaces are not interchangeable. Header identity alone neither grants nor negates ownership of a file already authoritatively recorded for the row. The host accepts identity only from the private core-owned channel, never from a public snapshot argument.

For a nonfork Session (`row.forked_from == NULL`), the exact recorded file and row association prove which CPD Session is being staged; source bytes, digest, and native header are validated as transcript content. A missing/null file is refused, not guessed from the Session ID.

For a fork Session, core requires the child's exact recorded non-null `row.file`, the child's `forked_from` association, and the referenced ancestor Session row to exist and remain stable under the fence (lock the relevant Session associations in stable Session-ID order when more than one fence is needed). The transport binds both child `cpdSessionId` and `forkedFrom`. The child-owned file may contain ancestor entries because native forks preserve transcript history; those entries do not make the file owned by the ancestor. Stage the child under the child's CPD ID and preserve the fork association/provenance; never infer the child from ancestor content, marker recipient, header UUID, path basename, or a caller assertion. Missing ancestor, null child file, inconsistent/changing association, an unavailable fence for either relevant row, or inability to establish the recorded child-file association is unverifiable and refused before sink access. Do not stage the ancestor's file as the child file.

Every mismatch, missing/unreadable/stale association, invalid bytes/digest, unexpected fork relation, capability/transport/reply failure, or unsupported snapshot is fail-closed: no durable sink calls on any preflight refusal. Sink invocation occurs only after core has supplied the verified channel request and the host has validated it. If the sink may have committed despite a lost reply, report indeterminate/refused to the caller and require reconciliation; never assert success or repeat automatically.

## Explicit non-goals and current boundary

This is a proposed concrete contract for a future approved implementation, not a claim that the repository currently has the Session fence, the private child channel, or trusted staging integration. Current `stageNativeConversation` validates caller-supplied bytes and converts them; `stageHostTranscript` adapts a caller-supplied snapshot and Harness. Neither reads `sessions.file`, proves ownership, nor supplies the lock lifetime above. Do not fix that gap with a marker check, header-ID comparison, filename check, public RPC/CLI, unconditional refusal advertised as matching support, or native source mutation. No live cutover is specified here.
