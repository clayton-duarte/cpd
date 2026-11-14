import { describe, expect, it } from "vitest";
import { staticHeaders } from "./static-headers.ts";

describe("staticHeaders", () => {
  it("sw.js and index.html are no-cache", () => {
    expect(staticHeaders("/sw.js")["Cache-Control"]).toBe("no-cache");
    expect(staticHeaders("/index.html")["Cache-Control"]).toBe("no-cache");
  });
  it("hashed assets are immutable", () => {
    expect(staticHeaders("/assets/index-abc123.js")["Cache-Control"]).toBe(
      "public, max-age=31536000, immutable",
    );
  });
  it("the manifest has the manifest content type", () => {
    const h = staticHeaders("/manifest.webmanifest");
    expect(h["Content-Type"]).toBe("application/manifest+json");
    expect(h["Cache-Control"]).toBe("no-cache");
  });
});
