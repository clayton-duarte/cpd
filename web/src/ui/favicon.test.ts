// @vitest-environment node
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url)); // web/src/ui
const webRoot = join(here, "../.."); // web
const indexPath = join(webRoot, "index.html");

describe("favicon", () => {
  it("index.html links SVG and ICO favicons", () => {
    const html = readFileSync(indexPath, "utf8");

    // Must link both SVG and ICO in head
    expect(html).toMatch(/<link\s+rel="icon"\s+type="image\/svg\+xml"\s+href="\/favicon\.svg"\s*\/?>/);
    expect(html).toMatch(/<link\s+rel="alternate icon"\s+type="image\/x-icon"\s+href="\/favicon\.ico"\s*\/?>/);

    // After title
    const titleIndex = html.indexOf("<title>CPD</title>");
    const svgIndex = html.indexOf('href="/favicon.svg"');
    const icoIndex = html.indexOf('href="/favicon.ico"');

    expect(titleIndex).toBeGreaterThan(-1);
    expect(svgIndex).toBeGreaterThan(titleIndex);
    expect(icoIndex).toBeGreaterThan(svgIndex);
  });

  it("static favicon assets exist and are served with 200", async () => {
    // CONTRACT 26.2 and 26.4:
    // cpdFavicon is configured in web/vite.config.ts and serves /favicon.svg and /favicon.ico via dev middleware,
    // and emits them in generateBundle for build.
    // Verify that vite.config.ts registers cpdFavicon, or test the dev middleware against a fake server.
    const viteConfigContent = readFileSync(join(webRoot, "vite.config.ts"), "utf8");
    expect(viteConfigContent).toMatch(/\bcpdFavicon\b/);

    // Also verify Vite config or plugin export exists and serves both endpoints
    // Import vite config or plugin
    const configModule: any = await import("../../vite.config.ts");
    const config = configModule.default;
    const resolvedConfig = typeof config === "function" ? await config({ command: "serve", mode: "development" }) : await config;
    const plugins = resolvedConfig?.plugins;
    const faviconPlugin = (plugins ?? []).flat().find((p: any) => p && p.name === "cpd-favicon");

    expect(faviconPlugin, "cpdFavicon plugin must be registered in vite.config.ts").toBeDefined();

    // Fake ViteDevServer test for configureServer
    const middlewares: Array<{ route?: string; handle: (req: any, res: any, next: () => void) => void }> = [];
    const fakeServer = {
      middlewares: {
        use(handle: any) {
          middlewares.push({ handle });
        },
      },
    };

    if (faviconPlugin && typeof faviconPlugin.configureServer === "function") {
      faviconPlugin.configureServer(fakeServer as any);
    }

    const testRoute = async (url: string, expectedType: string) => {
      let statusCode = 404;
      let contentType = "";
      let body: any = null;
      const res = {
        setHeader(name: string, value: string) {
          if (name.toLowerCase() === "content-type") contentType = value;
        },
        end(data: any) {
          statusCode = 200;
          body = data;
        },
      };

      for (const m of middlewares) {
        let calledNext = false;
        m.handle({ url }, res, () => { calledNext = true; });
        if (!calledNext) break;
      }

      expect(statusCode, `route ${url} should respond with 200`).toBe(200);
      expect(contentType, `route ${url} content-type`).toBe(expectedType);
      expect(body, `route ${url} body`).toBeTruthy();
    };

    await testRoute("/favicon.svg", "image/svg+xml");
    await testRoute("/favicon.ico", "image/x-icon");
  });
});
