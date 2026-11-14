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
  engineDownSince?: string | null;
  engineRestartReason?: string | null;
  lastSeenAt: string | null;
}

/** One banner, two inputs: the socket (the client's own last-seen time) and, while it is open, the engine. */
export function engineBanner(input: BannerInput): Banner | null {
  switch (input.connection) {
    case "connecting":
      return null;
    case "reconnecting":
      return { kind: "reconnecting", since: input.lastSeenAt };
    case "closed":
      return { kind: "server-unreachable", since: input.lastSeenAt };
    case "open":
      if (input.engineRunning) return null;
      return {
        kind: input.engineRestartReason === "install" ? "engine-restarting" : "engine-down",
        since: input.engineDownSince ?? null,
      };
  }
}
