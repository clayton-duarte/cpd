// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ExternalLink } from "./ExternalLink.tsx";

let mounted: { container: HTMLElement; unmount(): void } | null = null;

afterEach(() => {
  mounted?.unmount();
  mounted = null;
});

function renderLink(props: React.ComponentProps<typeof ExternalLink>): HTMLAnchorElement {
  mounted = mount(<ExternalLink {...props} />);
  const a = mounted.container.querySelector("a");
  if (!a) throw new Error("Expected anchor element to be rendered");
  return a;
}

describe("ExternalLink", () => {
  it("opens external links in a new tab with noopener noreferrer", () => {
    const link = renderLink({ href: "https://example.com/docs", children: "Docs" });
    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  });

  it("leaves in-app AgentKey routes in same tab", () => {
    const link = renderLink({ href: "#/a/test-agent", children: "Agent" });
    expect(link.getAttribute("target")).toBeNull();
    expect(link.getAttribute("rel")).toBeNull();
  });

  it("leaves in-app hash anchors in same tab", () => {
    const link = renderLink({ href: "#section-1", children: "Section" });
    expect(link.getAttribute("target")).toBeNull();
    expect(link.getAttribute("rel")).toBeNull();
  });
});
