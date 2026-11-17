import type { Tx } from "@earendil-works/pi-durable";

export type JsonValue = null | boolean | number | string | readonly JsonValue[] | { readonly [key: string]: JsonValue };
export type NativeEntry = { readonly id: string; readonly parentId: string | null; readonly type: string; readonly timestamp: number; readonly raw: { readonly [key: string]: JsonValue } };
export type DurableModel = { readonly provider: string; readonly modelId: string };
export type NativeSnapshot = {
  readonly cpdSessionId: string; readonly sourcePath: string; readonly sourceBytesBase64: string; readonly sourceSha256: string;
  readonly header: { readonly [key: string]: JsonValue }; readonly entries: readonly NativeEntry[]; readonly leafId: string | null;
  readonly tree: readonly { readonly id: string; readonly parentId: string | null }[];
  readonly activeAgent: { readonly model: DurableModel; readonly thinkingLevel: string; readonly cwd: string };
  readonly codingAgentVersion: "1.5.2"; readonly codingAgentPiAiVersion: "1.5.2"; readonly durableVersion: "1.5.0"; readonly durablePiAiVersion: "1.5.0";
};
export type PublicTx = Pick<Tx, "appendEntry">;
export type DurableStageSink = {
  readonly versions: { readonly durable: string; readonly durablePiAi: string; readonly codingAgent: string; readonly codingAgentPiAi: string };
  rootAbsent(cpdSessionId: string): Promise<boolean>;
  createRoot(args: { readonly cpdSessionId: string; readonly agent: NativeSnapshot["activeAgent"]; readonly init: (tx: PublicTx, conversationId: string) => Promise<void> }): Promise<{ readonly conversationId: string }>;
};
export type StageResult =
  | { readonly status: "staged"; readonly cpdSessionId: string; readonly durableConversationId: string; readonly sourceSha256: string; readonly sourceToDurableEntryId: Readonly<Record<string, string>>; readonly activeProjectionSourceIds: readonly string[]; readonly leafSourceId: string | null; readonly importVersion: "cpd-native-transcript/1"; readonly stagedOnly: true }
  | { readonly status: "refused"; readonly cpdSessionId: string; readonly sourceSha256: string; readonly code: "invalid-snapshot" | "identity-mismatch" | "unsupported-entry" | "unsupported-extension" | "invalid-tree" | "invalid-projection" | "invalid-message" | "version-incompatible" | "sink-incompatible" | "root-exists" | "sink-failed"; readonly message: string; readonly sourceEntryId?: string };
export async function stageNativeConversation(_input: NativeSnapshot, _sink: DurableStageSink): Promise<StageResult> {
  throw new Error("staged conversion not implemented");
}
