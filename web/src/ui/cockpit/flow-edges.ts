export interface FlowBox { readonly id: string; readonly top: number; readonly height: number }
export interface FlowEdge { readonly id: string; readonly d: string; readonly kind: "forward" | "loop" }
export function flowEdges(
  _boxes: readonly FlowBox[],
  _geometry: { readonly left: number; readonly width: number },
  _loops: readonly { readonly from: string; readonly to: string }[],
): FlowEdge[] {
  return [];
}
