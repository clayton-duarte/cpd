import type { Scheduler } from "../protocol/runtime.ts";
import type { FileReaderBoundary } from "./composer/attachments.ts";

export const browserScheduler: Scheduler = {
  now: () => Date.now(),
  after(ms, run) {
    const id = setTimeout(run, ms);
    return { dispose: () => clearTimeout(id) };
  },
};

const toBase64 = (buffer: ArrayBuffer): string => {
  const bytes = new Uint8Array(buffer);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
};

export const browserFileReader: FileReaderBoundary = {
  async readBase64(file) { return toBase64(await (file as File).arrayBuffer()); },
  readText(file) { return (file as File).text(); },
};
