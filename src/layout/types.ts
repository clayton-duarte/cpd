export interface LayoutNode { id: string; width: number; height: number; parentId?: string }
export interface LayoutEdge { id: string; source: string; target: string }
export interface LayoutGraph { nodes: LayoutNode[]; edges: LayoutEdge[] }
export interface Positioned { id: string; x: number; y: number; width: number; height: number }
export type LayoutResult = Record<string, Positioned>;
export type LayoutFn = (graph: LayoutGraph) => Promise<LayoutResult>;
