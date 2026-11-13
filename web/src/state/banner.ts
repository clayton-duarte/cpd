export type ConnectionState = "connecting" | "open" | "reconnecting" | "closed";
export type BannerKind =
  "reconnecting" | "server-unreachable" | "engine-down" | "engine-restarting";
export interface Banner {
  kind: BannerKind;
  since: string | null;
}
export interface BannerInput {
  connection: ConnectionState;
  engineRunning: boolean;
  engineDownSince?: string;
  engineRestartReason?: "install";
  lastSeenAt: string | null;
}

export function engineBanner(_input: BannerInput): Banner | null {
  throw new Error("not implemented");
}
