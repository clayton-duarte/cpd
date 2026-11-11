/**
 * Client for direct core WebSocket connection `/cpd-ws`.
 * Contract: web/CONTRACT.md §26.1 and docs/contract.md §5.8
 */

export interface CoreSnapshot {
  contexts: Array<{ id: string; title: string }>;
  plans: Array<{ id: string; title: string; phase: string }>;
  jobs: Array<{ plan: string; key: string; status: string }>;
  cursor: number;
}

export interface FakeCoreWsSocket {
  sent: string[];
  send(data: string): void;
  close(): void;
  on(event: "message", cb: (data: string) => void): void;
  on(event: "open", cb: () => void): void;
  on(event: "close", cb: () => void): void;
}

export interface CoreWsClientOptions {
  url: string;
  connectWs: (url: string) => FakeCoreWsSocket;
  onSnapshot?: (snapshot: CoreSnapshot) => void;
  onEvent?: (event: unknown) => void;
}

export class CoreWsClient {
  private readonly options: CoreWsClientOptions;
  private socket: FakeCoreWsSocket | null = null;
  private currentSnapshot: CoreSnapshot | null = null;
  private currentCursor: number = -1;
  private nextId: number = 1;
  private pendingRequests: Map<number, string> = new Map();

  constructor(options: CoreWsClientOptions) {
    this.options = options;
  }

  connect(): void {
    const socket = this.options.connectWs(this.options.url);
    this.socket = socket;

    socket.on("open", () => {
      this.sendRequest("snapshot", {});
    });

    socket.on("message", (data: string) => {
      let parsed: any;
      try {
        parsed = JSON.parse(data);
      } catch {
        return;
      }

      if (parsed && typeof parsed.id === "number") {
        const method = this.pendingRequests.get(parsed.id);
        this.pendingRequests.delete(parsed.id);

        if (method === "snapshot" && parsed.result) {
          this.currentSnapshot = parsed.result;
          if (typeof parsed.result.cursor === "number") {
            this.currentCursor = parsed.result.cursor;
          }
          this.options.onSnapshot?.(parsed.result);

          // Subscribe to live events with cursor
          this.sendRequest("events.subscribe", { cursor: this.currentCursor });
        }
      } else if (parsed && parsed.method === "event" && parsed.params?.event) {
        const ev = parsed.params.event;
        if (typeof ev.id === "number") {
          this.currentCursor = ev.id;
        }
        this.options.onEvent?.(ev);
      }
    });
  }

  private sendRequest(method: string, params: Record<string, unknown>): number {
    const id = this.nextId++;
    this.pendingRequests.set(id, method);
    this.socket?.send(JSON.stringify({ id, method, params }));
    return id;
  }

  get snapshot(): CoreSnapshot | null {
    return this.currentSnapshot;
  }

  get cursor(): number {
    return this.currentCursor;
  }

  close(): void {
    this.socket?.close();
  }
}
