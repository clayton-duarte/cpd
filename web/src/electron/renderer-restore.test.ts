import { expect, it } from "vitest";
import { PRODUCTION_RENDERER, restoreRenderer } from "./renderer-restore.ts";

it("restoreRenderer loads the production renderer after leaving preview", async () => {
  const loaded: string[] = [];
  await restoreRenderer({
    loadFile: async (file) => {
      loaded.push(file);
    },
  });
  expect(loaded).toEqual([PRODUCTION_RENDERER]);
});
