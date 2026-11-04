import { expect, it } from "vitest";
import { cn } from "../lib/utils.ts";

it("cn merges conflicting tailwind classes and drops falsy values", () => {
  expect(cn("p-2", "p-4", false)).toBe("p-4");
  expect(cn("a", false, undefined, ["b"])).toBe("a b");
});
