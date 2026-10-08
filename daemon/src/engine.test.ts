import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, afterEach } from "vitest";
import { FileCredentialStore } from "./credentials.ts";
import { flattenMessages, sseFrame } from "./engine.ts";

describe("FileCredentialStore", () => {
  let dir: string;

  afterEach(() => {
    if (dir) rmSync(dir, { recursive: true, force: true });
  });

  it("read() returns the stored entry for a provider", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({
        "github-copilot": { type: "oauth", access: "tok", refresh: "ref", expires: 0 },
      }),
    );
    const store = new FileCredentialStore(authPath);

    const cred = await store.read("github-copilot");

    expect(cred).toEqual({ type: "oauth", access: "tok", refresh: "ref", expires: 0 });
  });

  it("read() returns undefined for a missing provider key", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(authPath, JSON.stringify({}));
    const store = new FileCredentialStore(authPath);

    const cred = await store.read("nonexistent");

    expect(cred).toBeUndefined();
  });

  it("modify() round-trips a write", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({ "github-copilot": { type: "oauth", access: "old", refresh: "r", expires: 0 } }),
    );
    const store = new FileCredentialStore(authPath);

    const updated = await store.modify("github-copilot", async (current) => ({
      ...(current as { type: "oauth"; access: string; refresh: string; expires: number }),
      access: "new",
    }));

    expect(updated).toEqual({ type: "oauth", access: "new", refresh: "r", expires: 0 });
    const reread = await store.read("github-copilot");
    expect(reread).toEqual({ type: "oauth", access: "new", refresh: "r", expires: 0 });
  });

  it("list() reports providerId and type without secrets", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({
        "github-copilot": { type: "oauth", access: "tok", refresh: "ref", expires: 0 },
        "other": { type: "api_key", key: "shh" },
      }),
    );
    const store = new FileCredentialStore(authPath);

    const list = await store.list();

    expect(list).toEqual(
      expect.arrayContaining([
        { providerId: "github-copilot", type: "oauth" },
        { providerId: "other", type: "api_key" },
      ]),
    );
  });

  it("delete() removes the key", async () => {
    dir = mkdtempSync(join(tmpdir(), "cpd-auth-"));
    const authPath = join(dir, "auth.json");
    writeFileSync(
      authPath,
      JSON.stringify({ "github-copilot": { type: "oauth", access: "tok", refresh: "ref", expires: 0 } }),
    );
    const store = new FileCredentialStore(authPath);

    await store.delete("github-copilot");

    const reread = await store.read("github-copilot");
    expect(reread).toBeUndefined();
  });

  it("produces an actionable error when the auth file is missing", async () => {
    const missingPath = join(tmpdir(), "cpd-auth-does-not-exist", "auth.json");
    const store = new FileCredentialStore(missingPath);

    await expect(store.read("github-copilot")).rejects.toThrow(
      /No GitHub Copilot credentials found at .*\. Run `pi` and log in first\./,
    );
  });
});

describe("flattenMessages", () => {
  it("joins text parts of an assistant/user message into a plain string", () => {
    expect(
      flattenMessages([
        { role: "user", content: [{ type: "text", text: "a" }, { type: "text", text: "b" }] } as never,
      ]),
    ).toEqual([{ role: "user", content: "ab" }]);
  });

  it("ignores non-text parts", () => {
    expect(
      flattenMessages([
        {
          role: "assistant",
          content: [
            { type: "text", text: "hi" },
            { type: "toolCall", id: "1", name: "x", arguments: {} },
          ],
        } as never,
      ]),
    ).toEqual([{ role: "assistant", content: "hi" }]);
  });

  it("drops empty system messages", () => {
    expect(
      flattenMessages([
        { role: "system", content: "" } as never,
        { role: "system", content: [] } as never,
        { role: "user", content: "hello" } as never,
      ]),
    ).toEqual([{ role: "user", content: "hello" }]);
  });

  it("passes through a plain string system message with content", () => {
    expect(flattenMessages([{ role: "system", content: "be nice" } as never])).toEqual([
      { role: "system", content: "be nice" },
    ]);
  });
});

describe("sseFrame", () => {
  it("serializes a payload to exactly data: {...}\\n\\n", () => {
    const frame = sseFrame({ type: "messages", messages: [] });
    expect(frame).toBe(`data: ${JSON.stringify({ type: "messages", messages: [] })}\n\n`);
  });
});
