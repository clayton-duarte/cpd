import { IMAGE_MAX_COUNT, IMAGE_MAX_TOTAL_BASE64, IMAGE_MIME_TYPES, TEXT_INLINE_MAX_BYTES } from "../../protocol/domain.ts";
import type { Attachment, Capabilities, ImageMime, WireImage } from "../../protocol/domain.ts";

export interface FileLike { readonly name: string; readonly type: string; readonly size: number }
export interface FileReaderBoundary { readBase64(file: FileLike): Promise<string>; readText(file: FileLike): Promise<string> }
export type RejectionReason = "too_many_images" | "images_too_large" | "unsupported_image_type" | "images_unsupported" | "binary_not_supported" | "text_too_large";
export interface AttachmentRejection { readonly name: string; readonly reason: RejectionReason; readonly message: string }
export interface AddResult { readonly accepted: Attachment[]; readonly rejected: AttachmentRejection[] }

const TEXT_TYPES = new Set(["application/json", "application/xml", "application/x-yaml", "application/toml", "application/javascript", "application/typescript"]);
const TEXT_EXTENSIONS = new Set("md txt json ts tsx js jsx mjs cjs rs toml yaml yml csv log sh css html xml py go java c h cpp sql".split(" "));

function isImageMime(type: string): type is ImageMime {
  return (IMAGE_MIME_TYPES as readonly string[]).includes(type);
}

function isTextLike(file: FileLike): boolean {
  if (file.type.startsWith("text/") || TEXT_TYPES.has(file.type)) return true;
  if (file.type !== "" && file.type !== "application/octet-stream") return false;
  const dot = file.name.lastIndexOf(".");
  return dot >= 0 && TEXT_EXTENSIONS.has(file.name.slice(dot + 1).toLowerCase());
}

export async function addFiles(files: readonly FileLike[], existing: readonly Attachment[], capabilities: Capabilities | null, reader: FileReaderBoundary): Promise<AddResult> {
  const accepted: Attachment[] = [];
  const rejected: AttachmentRejection[] = [];
  const reject = (name: string, reason: RejectionReason, message: string) => rejected.push({ name, reason, message });
  let images = existing.filter((a) => a.kind === "image").length;
  let total = existing.reduce((n, a) => n + (a.kind === "image" ? a.dataBase64.length : 0), 0);
  for (const file of files) {
      if (file.type.startsWith("image/")) {
        if (!isImageMime(file.type)) { reject(file.name, "unsupported_image_type", `Unsupported image type ${file.type}`); continue; }
        if (capabilities !== null && !capabilities.images.supported) { reject(file.name, "images_unsupported", capabilities.images.reason); continue; }
        if (images >= IMAGE_MAX_COUNT) { reject(file.name, "too_many_images", `At most ${IMAGE_MAX_COUNT} images`); continue; }
        const dataBase64 = await reader.readBase64(file);
        if (total + dataBase64.length > IMAGE_MAX_TOTAL_BASE64) { reject(file.name, "images_too_large", `Images exceed ${IMAGE_MAX_TOTAL_BASE64} base64 characters in total`); continue; }
        images += 1;
        total += dataBase64.length;
        accepted.push({ kind: "image", name: file.name, mimeType: file.type, dataBase64 });
      } else if (isTextLike(file)) {
        if (file.size > TEXT_INLINE_MAX_BYTES) { reject(file.name, "text_too_large", `Text files are limited to ${TEXT_INLINE_MAX_BYTES} bytes`); continue; }
        accepted.push({ kind: "text", name: file.name, text: await reader.readText(file) });
      } else {
        reject(file.name, "binary_not_supported", "Binary files other than png, jpeg, gif and webp images cannot be attached");
      }
  }
  return { accepted, rejected };
}

export function filesFromClipboard(data: { files: ArrayLike<FileLike> } | null): FileLike[] {
  return data === null ? [] : Array.from(data.files);
}
export function filesFromDrop(data: { files: ArrayLike<FileLike> } | null): FileLike[] {
  return data === null ? [] : Array.from(data.files);
}

export function buildPrompt(text: string, attachments: readonly Attachment[]): { text: string; images: WireImage[] } {
  let out = text;
  const images: WireImage[] = [];
  for (const a of attachments) {
    if (a.kind === "text") out += `\n\n<file name="${a.name.replaceAll('"', "&quot;")}">\n${a.text}\n</file>`;
    else images.push({ mimeType: a.mimeType, dataBase64: a.dataBase64 });
  }
  return { text: out, images };
}
