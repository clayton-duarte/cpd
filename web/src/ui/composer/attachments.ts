import type { Attachment, Capabilities, WireImage } from "../../protocol/domain.ts";

export interface FileLike { readonly name: string; readonly type: string; readonly size: number }
export interface FileReaderBoundary { readBase64(file: FileLike): Promise<string>; readText(file: FileLike): Promise<string> }
export type RejectionReason = "too_many_images" | "images_too_large" | "unsupported_image_type" | "images_unsupported" | "binary_not_supported" | "text_too_large";
export interface AttachmentRejection { readonly name: string; readonly reason: RejectionReason; readonly message: string }
export interface AddResult { readonly accepted: Attachment[]; readonly rejected: AttachmentRejection[] }

export function addFiles(_files: readonly FileLike[], _existing: readonly Attachment[], _capabilities: Capabilities | null, _reader: FileReaderBoundary): Promise<AddResult> {
  throw new Error("not implemented");
}
export function filesFromClipboard(_data: { files: ArrayLike<FileLike> } | null): FileLike[] {
  throw new Error("not implemented");
}
export function filesFromDrop(_data: { files: ArrayLike<FileLike> } | null): FileLike[] {
  throw new Error("not implemented");
}
export function buildPrompt(_text: string, _attachments: readonly Attachment[]): { text: string; images: WireImage[] } {
  throw new Error("not implemented");
}
