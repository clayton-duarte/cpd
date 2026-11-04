import { expect, it } from "vitest";
import type { Attachment, Capabilities } from "../../protocol/domain.ts";
import { addFiles, buildPrompt, filesFromClipboard, filesFromDrop } from "./attachments.ts";
import type { FileLike, FileReaderBoundary } from "./attachments.ts";

const caps: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: { supported: true }, followUp: { supported: true }, abort: { supported: true },
  abortSubmission: { supported: true }, compact: { supported: true },
  configure: { model: { supported: true }, thinking: { supported: true }, tier: { supported: true, options: ["S1"] } },
  dialogs: { mode: "interactive" },
};

const file = (name: string, type: string, size = 10): FileLike => ({ name, type, size });

function reader(base64: Record<string, string> = {}, text: Record<string, string> = {}): FileReaderBoundary {
  return {
    readBase64: async (f) => base64[f.name] ?? "QUJD",
    readText: async (f) => text[f.name] ?? "content",
  };
}

it("image files become base64 attachments with mime type", async () => {
    const r = await addFiles([file("a.png", "image/png")], [], caps, reader({ "a.png": "iVBORw0=" }));
    expect(r.rejected).toEqual([]);
    expect(r.accepted).toEqual([{ kind: "image", name: "a.png", mimeType: "image/png", dataBase64: "iVBORw0=" }]);
  });

it("more than four images are rejected", async () => {
    const files = ["1", "2", "3", "4", "5"].map((n) => file(`${n}.png`, "image/png"));
    const r = await addFiles(files, [], caps, reader());
    expect(r.accepted.map((a) => a.name)).toEqual(["1.png", "2.png", "3.png", "4.png"]);
    expect(r.rejected.map((x) => [x.name, x.reason])).toEqual([["5.png", "too_many_images"]]);
  });

it("total base64 beyond 786432 characters is rejected", async () => {
    const big = "A".repeat(500_000);
    const r = await addFiles([file("a.png", "image/png"), file("b.png", "image/png")], [], caps, reader({ "a.png": big, "b.png": big }));
    expect(r.accepted.map((a) => a.name)).toEqual(["a.png"]);
    expect(r.rejected.map((x) => [x.name, x.reason])).toEqual([["b.png", "images_too_large"]]);
  });

it("text files are inlined into the prompt", async () => {
    const r = await addFiles([file("notes.md", "text/markdown")], [], caps, reader({}, { "notes.md": "hello" }));
    expect(r.accepted).toEqual([{ kind: "text", name: "notes.md", text: "hello" }]);
    const built = buildPrompt("see", r.accepted as Attachment[]);
    expect(built.text).toBe('see\n\n<file name="notes.md">\nhello\n</file>');
    expect(built.images).toEqual([]);
  });

it("binary non-image files are rejected with a reason", async () => {
    const r = await addFiles([file("a.zip", "application/zip")], [], caps, reader());
    expect(r.accepted).toEqual([]);
    expect(r.rejected).toHaveLength(1);
    expect(r.rejected[0]?.reason).toBe("binary_not_supported");
    expect(r.rejected[0]?.message).not.toBe("");
  });

it("paste events yield attachments from clipboard files", async () => {
    const f = file("p.png", "image/png");
    const files = filesFromClipboard({ files: [f] });
    expect(files).toEqual([f]);
    expect(filesFromClipboard(null)).toEqual([]);
    const r = await addFiles(files, [], caps, reader());
    expect(r.accepted.map((a) => a.name)).toEqual(["p.png"]);
  });

it("drop events yield attachments from dataTransfer files", async () => {
    const f = file("d.txt", "text/plain");
    const files = filesFromDrop({ files: [f] });
    expect(files).toEqual([f]);
    expect(filesFromDrop(null)).toEqual([]);
    const r = await addFiles(files, [], caps, reader());
    expect(r.accepted.map((a) => a.name)).toEqual(["d.txt"]);
  });
