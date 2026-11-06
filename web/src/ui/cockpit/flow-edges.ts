export interface FlowBox { readonly id: string; readonly top: number; readonly height: number }
export interface FlowEdge { readonly id: string; readonly d: string; readonly kind: "forward" | "loop" }

const LANE_FIRST = 14;
const LANE_STEP = 10;
const SELF_HALF = 6;

export function flowEdges(
  boxes: readonly FlowBox[],
  geometry: { readonly left: number; readonly width: number },
  loops: readonly { readonly from: string; readonly to: string }[],
): FlowEdge[] {
  const byId = new Map(boxes.map((b) => [b.id, b]));
  const box = (id: string): FlowBox => {
    const b = byId.get(id);
    if (b === undefined) throw new Error(`unknown flow box id: ${id}`);
    return b;
  };
  const cx = geometry.left + geometry.width / 2;
  const right = geometry.left + geometry.width;
  const edges: FlowEdge[] = [];
  for (let i = 0; i + 1 < boxes.length; i++) {
    const a = boxes[i]!;
    const b = boxes[i + 1]!;
    edges.push({ id: `${a.id}>${b.id}`, d: `M ${cx} ${a.top + a.height} L ${cx} ${b.top}`, kind: "forward" });
  }
  loops.forEach((l, i) => {
    const from = box(l.from);
    const to = box(l.to);
    const x = right + LANE_FIRST + LANE_STEP * i;
    const mid = (b: FlowBox) => b.top + b.height / 2;
    const y1 = from === to ? mid(from) - SELF_HALF : mid(from);
    const y2 = from === to ? mid(to) + SELF_HALF : mid(to);
    edges.push({
      id: `loop:${l.from}>${l.to}`,
      d: `M ${right} ${y1} L ${x} ${y1} L ${x} ${y2} L ${right} ${y2}`,
      kind: "loop",
    });
  });
  return edges;
}
