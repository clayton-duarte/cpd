import { describe, expect, it } from "vitest";
import { routeRequest } from "./sw-routes.ts";

const origin = "http://localhost:3333";
const req = (o: Partial<Parameters<typeof routeRequest>[0]>) =>
  routeRequest({ method: "GET", url: `${origin}/`, mode: "no-cors", origin, ...o });

describe("routeRequest", () => {
  it("navigations fall back to the cached index.html", () => {
    expect(req({ mode: "navigate" })).toBe("navigation-fallback");
    expect(req({ mode: "navigate", url: `${origin}/some/route` })).toBe("navigation-fallback");
  });
  it("built assets are precached", () => {
    expect(req({ url: `${origin}/assets/index-abc123.js` })).toBe("precache");
    expect(req({ url: `${origin}/assets/index-abc123.css` })).toBe("precache");
  });
  it("the websocket endpoint and non-GET requests go to the network", () => {
    expect(req({ url: `${origin}/cpd-ws` })).toBe("network");
    expect(req({ url: `${origin}/api/x` })).toBe("network");
    expect(req({ method: "POST", url: `${origin}/assets/a.js` })).toBe("network");
    expect(req({ url: "https://other.example/assets/a.js" })).toBe("network");
  });
});
