import { readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import type { Credential, CredentialInfo, CredentialStore } from "@earendil-works/pi-ai";

/** Default location of pi's auth file; overridable via CPD_PI_AUTH. */
export const AUTH_PATH = process.env.CPD_PI_AUTH ?? `${homedir()}/.pi/agent/auth.json`;

type AuthFile = Record<string, Credential>;

async function readAuthFile(path: string): Promise<AuthFile> {
  try {
    const raw = await readFile(path, "utf8");
    return JSON.parse(raw) as AuthFile;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      throw new Error(`No GitHub Copilot credentials found at ${path}. Run \`pi\` and log in first.`);
    }
    throw error;
  }
}

/**
 * `CredentialStore` over pi's existing `auth.json`. This is app-owned (pi-ai does not export a
 * usable implementation); entries already match pi-ai's `Credential` shape and pass straight
 * through. `modify` is the only write path, matching the interface contract.
 */
export class FileCredentialStore implements CredentialStore {
  private readonly path: string;

  constructor(path: string = AUTH_PATH) {
    this.path = path;
  }

  async read(providerId: string): Promise<Credential | undefined> {
    const data = await readAuthFile(this.path);
    return data[providerId];
  }

  async list(): Promise<readonly CredentialInfo[]> {
    const data = await readAuthFile(this.path);
    return Object.entries(data).map(([providerId, credential]) => ({
      providerId,
      type: credential.type,
    }));
  }

  async modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
  ): Promise<Credential | undefined> {
    const data = await readAuthFile(this.path);
    const next = await fn(data[providerId]);
    if (next === undefined) return data[providerId];
    data[providerId] = next;
    await writeFile(this.path, JSON.stringify(data, null, 2), "utf8");
    return next;
  }

  async delete(providerId: string): Promise<void> {
    const data = await readAuthFile(this.path);
    delete data[providerId];
    await writeFile(this.path, JSON.stringify(data, null, 2), "utf8");
  }
}
