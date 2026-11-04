// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../testing/render.tsx";
import { Badge } from "./components/badge.tsx";
import { Bubble } from "./components/bubble.tsx";
import { Button } from "./components/button.tsx";
import { Marker } from "./components/marker.tsx";

it("hand written shadcn parts render their documented hooks", () => {
  const m = mount(
    <div>
      <Bubble variant="secondary" align="end" id="b">hi</Bubble>
      <Marker variant="separator" id="m">sep</Marker>
      <Button variant="outline" size="sm" tone="landing" id="btn">go</Button>
      <Badge variant="secondary" id="bad">x</Badge>
      <Badge tone="focus" id="bad2">y</Badge>
    </div>,
  );
  const q = (id: string) => m.container.querySelector(`#${id}`) as HTMLElement;
  expect(q("b").dataset.slot).toBe("bubble");
  expect(q("b").dataset.variant).toBe("secondary");
  expect(q("b").dataset.align).toBe("end");
  expect(q("m").dataset.slot).toBe("marker");
  expect(q("m").dataset.variant).toBe("separator");
  expect(q("btn").dataset.slot).toBe("button");
  expect(q("btn").dataset.variant).toBe("outline");
  expect(q("btn").dataset.size).toBe("sm");
  expect(q("btn").className).toContain("text-status-landing");
  expect(q("btn").className).toContain("border-status-landing");
  expect(q("bad").dataset.slot).toBe("badge");
  expect(q("bad").dataset.variant).toBe("secondary");
  expect(q("bad2").dataset.variant).toBe("outline");
  expect(q("bad2").className).toContain("text-status-focus");
  m.unmount();
});
