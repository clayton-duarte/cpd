export interface LayoutNode {
  id: string;
  width: number;
  height: number;
  parentId?: string;
  /** Optional per-node ELK layout option overrides (e.g. extra padding to
   *  reserve room for a header/label rendered inside a compound node). */
  layoutOptions?: Record<string, string>;
}
export interface LayoutEdge { id: string; source: string; target: string }
export interface LayoutGraph { nodes: LayoutNode[]; edges: LayoutEdge[] }
export interface Positioned { id: string; x: number; y: number; width: number; height: number }
export type LayoutResult = Record<string, Positioned>;
export type LayoutFn = (graph: LayoutGraph) => Promise<LayoutResult>;
