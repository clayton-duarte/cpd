import { WebError } from "../../protocol/domain.ts";
import type { HostCursor } from "./client.ts";

export type Op = readonly unknown[];
export interface Replica {
  view(frame: { epoch: string; seq: number; view: unknown }): void;
  ops(frame: { epoch: string; seq: number; ops: readonly unknown[] }): void;
  readonly value: unknown | null;
  readonly cursor: HostCursor | null;
}

const FORBIDDEN = new Set(["__proto__", "constructor", "prototype"]);

const fail = (message: string): never => {
  throw new WebError("protocol_error", message);
};

type Seg = string | number;

function asPath(v: unknown, allowEmpty: boolean): readonly Seg[] {
  if (!Array.isArray(v)) return fail("op path is not an array");
  if (!allowEmpty && v.length === 0) return fail("op path is empty");
  for (const s of v) {
    if (typeof s === "string") {
      if (FORBIDDEN.has(s)) return fail(`forbidden path segment ${s}`);
    } else if (!Number.isInteger(s) || (s as number) < 0) return fail("path segment is not a string or index");
  }
  return v as readonly Seg[];
}

const isObject = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);

const inRange = (arr: readonly unknown[], i: unknown, size = arr.length): i is number =>
  typeof i === "number" && Number.isInteger(i) && i >= 0 && i < size;

/** Copy only the path to `path`; `fn` receives the node at the path and returns its replacement. */
function update(node: unknown, path: readonly Seg[], depth: number, fn: (target: unknown) => unknown): unknown {
  if (depth === path.length) return fn(node);
  const seg = path[depth] as Seg;
  if (Array.isArray(node)) {
    if (!inRange(node, seg)) return fail(`index ${String(seg)} out of range`);
    const next = update(node[seg], path, depth + 1, fn);
    if (next === node[seg]) return node;
    const copy = node.slice();
    copy[seg] = next;
    return copy;
  }
  if (isObject(node)) {
    if (typeof seg !== "string" || !Object.hasOwn(node, seg)) return fail(`missing key ${String(seg)}`);
    const next = update(node[seg], path, depth + 1, fn);
    if (next === node[seg]) return node;
    return { ...node, [seg]: next };
  }
  return fail("path walks through a non-container");
}

/** Update the parent of the last segment of a non-empty path. */
function updateChild(root: unknown, path: readonly Seg[], fn: (parent: unknown, key: Seg) => unknown): unknown {
  const key = path[path.length - 1] as Seg;
  return update(root, path.slice(0, -1), 0, (parent) => fn(parent, key));
}

function applyOne(root: unknown, op: unknown): unknown {
  if (!Array.isArray(op) || typeof op[0] !== "string") return fail("op is not a tuple");
  const [kind] = op as [string, ...unknown[]];
  const arity = (n: number) => {
    if (op.length !== n) fail(`op ${kind} needs ${n - 1} arguments`);
  };
  switch (kind) {
    case "r":
      arity(2);
      return op[1];
    case "s": {
      arity(3);
      const path = asPath(op[1], false);
      return updateChild(root, path, (parent, key) => {
        if (Array.isArray(parent)) {
          if (!inRange(parent, key)) return fail("set index out of range");
          const copy = parent.slice();
          copy[key] = op[2];
          return copy;
        }
        if (isObject(parent) && typeof key === "string") return { ...parent, [key]: op[2] };
        return fail("set target is not a container");
      });
    }
    case "d": {
      arity(2);
      const path = asPath(op[1], false);
      return updateChild(root, path, (parent, key) => {
        if (Array.isArray(parent)) {
          if (!inRange(parent, key)) return fail("delete index out of range");
          return parent.filter((_, i) => i !== key);
        }
        if (isObject(parent) && typeof key === "string") {
          if (!Object.hasOwn(parent, key)) return fail(`missing key ${key}`);
          const { [key]: _gone, ...rest } = parent;
          return rest;
        }
        return fail("delete target is not a container");
      });
    }
    case "a":
    case "t": {
      arity(3);
      const path = asPath(op[1], false);
      const arg = op[2];
      if (kind === "a" ? typeof arg !== "string" : typeof arg !== "number" || !Number.isInteger(arg) || arg < 0) {
        return fail(`op ${kind} argument is invalid`);
      }
      return updateChild(root, path, (parent, key) => {
        let cur: unknown;
        if (Array.isArray(parent)) cur = inRange(parent, key) ? parent[key] : fail("index out of range");
        else if (isObject(parent) && typeof key === "string" && Object.hasOwn(parent, key)) cur = parent[key];
        else return fail("target is missing");
        if (typeof cur !== "string") return fail("target is not a string");
        const next = kind === "a" ? cur + (arg as string) : cur.slice(arg as number);
        if (Array.isArray(parent)) {
          const copy = parent.slice();
          copy[key as number] = next;
          return copy;
        }
        return { ...(parent as Record<string, unknown>), [key]: next };
      });
    }
    case "p": {
      arity(5);
      const path = asPath(op[1], true);
      const [, , start, del, items] = op as unknown[];
      if (!Array.isArray(items)) return fail("p items is not an array");
      return update(root, path, 0, (arr) => {
        if (!Array.isArray(arr)) return fail("p target is not an array");
        if (typeof start !== "number" || !Number.isInteger(start) || start < 0 || start > arr.length) return fail("p start out of range");
        if (typeof del !== "number" || !Number.isInteger(del) || del < 0 || start + del > arr.length) return fail("p deleteCount out of range");
        const copy = arr.slice();
        copy.splice(start, del, ...items);
        return copy;
      });
    }
    case "m": {
      arity(3);
      const path = asPath(op[1], true);
      const perm = op[2];
      return update(root, path, 0, (arr) => {
        if (!Array.isArray(arr)) return fail("m target is not an array");
        if (!Array.isArray(perm) || perm.length !== arr.length) return fail("m permutation length mismatch");
        const seen = new Set<number>();
        for (const p of perm) {
          if (!inRange(arr, p) || seen.has(p)) return fail("m permutation is invalid");
          seen.add(p);
        }
        return perm.map((p: number) => arr[p]);
      });
    }
    default:
      return fail(`unknown op ${kind}`);
  }
}

/** Apply Chord ops immutably (structural sharing). All ops apply or none: any refusal throws protocol_error. */
export function applyOps(root: unknown, ops: readonly unknown[]): unknown {
  if (!Array.isArray(ops)) return fail("ops is not an array");
  let value = root;
  for (const op of ops) value = applyOne(value, op);
  return value;
}

export function createReplica(): Replica {
  let value: unknown | null = null;
  let cursor: HostCursor | null = null;
  return {
    view(frame) {
      value = frame.view;
      cursor = { epoch: frame.epoch, seq: frame.seq };
    },
    ops(frame) {
      if (cursor === null || value === null) return fail("ops frame before any view");
      if (frame.epoch !== cursor.epoch) return fail("ops frame epoch changed");
      if (frame.seq !== cursor.seq + 1) return fail(`ops frame seq gap: expected ${cursor.seq + 1}, got ${frame.seq}`);
      value = applyOps(value, frame.ops);
      cursor = { epoch: frame.epoch, seq: frame.seq };
    },
    get value() {
      return value;
    },
    get cursor() {
      return cursor;
    },
  };
}
