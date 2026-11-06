import { expect, it } from "vitest";
import { flowEdges } from "./flow-edges.ts";

const boxes = [
  { id: "a", top: 0, height: 40 },
  { id: "b", top: 60, height: 40 },
  { id: "c", top: 120, height: 20 },
];
const geo = { left: 10, width: 100 };

it("forward edges run from bottom centre to the next top centre", () => {
  const edges = flowEdges(boxes, geo, []);
  expect(edges).toEqual([
    { id: "a>b", d: "M 60 40 L 60 60", kind: "forward" },
    { id: "b>c", d: "M 60 100 L 60 120", kind: "forward" },
  ]);
});

it("loop edges run in right-hand lanes one per loop", () => {
  const edges = flowEdges(boxes, geo, [{ from: "a", to: "c" }, { from: "c", to: "b" }]);
  expect(edges.slice(2)).toEqual([
    { id: "loop:a>c", d: "M 110 20 L 124 20 L 124 130 L 110 130", kind: "loop" },
    { id: "loop:c>b", d: "M 110 130 L 134 130 L 134 80 L 110 80", kind: "loop" },
  ]);
});

it("a self loop leaves and re-enters the same card's right edge", () => {
  const edges = flowEdges(boxes, geo, [{ from: "b", to: "b" }]);
  expect(edges[2]).toEqual({ id: "loop:b>b", d: "M 110 74 L 124 74 L 124 86 L 110 86", kind: "loop" });
});

it("an unknown box id throws", () => {
  expect(() => flowEdges(boxes, geo, [{ from: "zz", to: "a" }])).toThrow(/zz/);
});
