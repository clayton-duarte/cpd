// Typed pi tools connecting to core socket over run/core.sock using NDJSON framing (contract §5.9).

import net from "node:net";
import readline from "node:readline";
import { TOOL_DEFINITIONS, type ToolNamespace } from "./generated/tools.ts";

export interface CoreSocketClientOptions {
  socketPath: string;
  session: string;
}

export interface CoreRpcRequest {
  id: number;
  method: string;
  params: Record<string, unknown>;
}

export interface CoreRpcReply {
  id: number;
  result: Record<string, unknown> | null;
  error: { code: string; message: string; details?: Record<string, unknown> } | null;
}

export class CoreSocketClient {
  private socketPath: string;
  private session: string;
  private client: net.Socket | null = null;
  private nextId = 1;
  private pending = new Map<number, { resolve: (val: any) => void; reject: (err: any) => void }>();
  private authenticated = false;

  constructor(options: CoreSocketClientOptions) {
    this.socketPath = options.socketPath;
    this.session = options.session;
  }

  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection({ path: this.socketPath });
      this.client = socket;

      const rl = readline.createInterface({ input: socket });
      rl.on("line", (line) => {
        if (!line.trim()) return;
        try {
          const reply: CoreRpcReply = JSON.parse(line);
          const p = this.pending.get(reply.id);
          if (p) {
            this.pending.delete(reply.id);
            if (reply.error) {
              p.reject(reply.error);
            } else {
              p.resolve(reply.result);
            }
          }
        } catch (e) {
          // ignore invalid json line
        }
      });

      socket.on("error", (err) => {
        reject(err);
      });

      socket.on("connect", async () => {
        try {
          // hello handshake
          const helloResult = await this.sendDirect(0, "hello", {
            session: this.session,
            protocol: 1,
          });
          this.authenticated = true;
          resolve();
        } catch (e) {
          reject(e);
        }
      });
    });
  }

  private sendDirect(id: number, method: string, params: Record<string, unknown>): Promise<any> {
    return new Promise((resolve, reject) => {
      if (!this.client || this.client.destroyed) {
        return reject({ code: "core_unreachable", details: { sent: false } });
      }
      this.pending.set(id, { resolve, reject });
      const line = JSON.stringify({ id, method, params }) + "\n";
      this.client.write(line, (err) => {
        if (err) {
          this.pending.delete(id);
          reject({ code: "core_unreachable", details: { sent: false } });
        }
      });
    });
  }

  async call(domain: string, verb: string, params: Record<string, unknown> = {}): Promise<any> {
    if (!this.authenticated) {
      throw { code: "bad_request", message: "Request before hello" };
    }
    const id = this.nextId++;
    const method = `${domain}.${verb}`;
    return this.sendDirect(id, method, params);
  }

  close(): void {
    if (this.client) {
      this.client.destroy();
      this.client = null;
    }
  }
}

export function getToolDefinitions() {
  return TOOL_DEFINITIONS;
}
