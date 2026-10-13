import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { CpdError, failure } from "../lib/errors.ts";

// Minimal template engine (contract §16): `{{name}}`, `{{#each list}}…{{/each}}`, `{{#if x}}…{{/if}}`. Nothing more.
//
// - A value is looked up in the innermost scope that defines the name, then outwards. Inside `each`, the item itself is `{{this}}`
//   and, when it is an object, its fields are names.
// - A name that no scope defines (or whose value is `undefined`/`null`) is `template_missing_value`.
// - `{{name}}` takes a string, number or boolean; anything else is `template_bad_value`.
// - `if` is true for a non-empty string, a non-empty array, `true` or a non-zero number; the name must still exist.
// - A block tag alone on its line takes the whole line with it, so templates stay readable.
// - Output is a single pass: substituted text is never re-scanned for tags.

export type TemplateErrorCode = "template_syntax" | "template_missing_value" | "template_bad_value";

/** Failure of a template or its data: a bug in the caller or the template, never in the User's input (exit 1). */
export class TemplateError extends CpdError {
  constructor(code: TemplateErrorCode, message: string, details: Record<string, unknown> = {}) {
    super(code, message, 1, details);
    this.name = "TemplateError";
  }
}

export type TemplateData = { readonly [key: string]: unknown };

type Node =
  | { t: "text"; text: string }
  | { t: "var"; name: string }
  | { t: "each"; name: string; body: Node[] }
  | { t: "if"; name: string; body: Node[] };

const NAME = /^(?:this|[A-Za-z_][A-Za-z0-9_]*)$/;
const TAG = /\{\{([^{}]*)\}\}/g;
/** A block tag (open or close) with only blanks around it on its line. */
const STANDALONE = /^[ \t]*(\{\{[#/][^{}]*\}\})[ \t]*(?:\n|$)/gm;

function parse(source: string): Node[] {
  const text = source.replace(STANDALONE, "$1");
  const root: Node[] = [];
  const stack: { node: Extract<Node, { body: Node[] }>; opener: string }[] = [];
  const current = () => stack.at(-1)?.node.body ?? root;
  let last = 0;
  for (const m of text.matchAll(TAG)) {
    if (m.index > last) current().push({ t: "text", text: text.slice(last, m.index) });
    last = m.index + m[0].length;
    const inner = m[1]!.trim();
    const block = /^#(each|if)\s+(\S+)$/.exec(inner);
    if (block) {
      const name = block[2]!;
      if (!NAME.test(name)) throw new TemplateError("template_syntax", `invalid name in ${m[0]}`, { tag: m[0] });
      const node = { t: block[1] as "each" | "if", name, body: [] as Node[] };
      current().push(node);
      stack.push({ node, opener: m[0] });
    } else if (inner === "/each" || inner === "/if") {
      const open = stack.pop();
      if (!open || `/${open.node.t}` !== inner) {
        throw new TemplateError("template_syntax", `unexpected ${m[0]}`, { tag: m[0], open: open?.opener ?? null });
      }
    } else if (NAME.test(inner)) {
      current().push({ t: "var", name: inner });
    } else {
      throw new TemplateError("template_syntax", `invalid placeholder ${m[0]}`, { tag: m[0] });
    }
  }
  if (last < text.length) current().push({ t: "text", text: text.slice(last) });
  const unclosed = stack.at(-1);
  if (unclosed) throw new TemplateError("template_syntax", `${unclosed.opener} is never closed`, { tag: unclosed.opener });
  return root;
}

type Scope = { readonly item: unknown; readonly isThis: boolean };

function lookup(scopes: Scope[], name: string): unknown {
  for (let i = scopes.length - 1; i >= 0; i--) {
    const s = scopes[i]!;
    if (name === "this") {
      if (s.isThis) return s.item;
      continue;
    }
    if (s.item !== null && typeof s.item === "object" && !Array.isArray(s.item) && Object.hasOwn(s.item, name)) {
      // A name the scope defines but leaves empty is missing; it never falls through to an outer scope.
      const v = (s.item as Record<string, unknown>)[name];
      if (v === undefined || v === null) break;
      return v;
    }
  }
  throw new TemplateError("template_missing_value", `template value is missing: ${name}`, { name });
}

const truthy = (v: unknown): boolean => (Array.isArray(v) ? v.length > 0 : typeof v === "string" ? v !== "" : Boolean(v));

function renderNodes(nodes: Node[], scopes: Scope[]): string {
  let out = "";
  for (const n of nodes) {
    if (n.t === "text") {
      out += n.text;
    } else if (n.t === "var") {
      const v = lookup(scopes, n.name);
      if (typeof v !== "string" && typeof v !== "number" && typeof v !== "boolean") {
        throw new TemplateError("template_bad_value", `template value ${n.name} must be a string, number or boolean`, { name: n.name, type: Array.isArray(v) ? "array" : typeof v });
      }
      out += String(v);
    } else if (n.t === "if") {
      if (truthy(lookup(scopes, n.name))) out += renderNodes(n.body, scopes);
    } else {
      const list = lookup(scopes, n.name);
      if (!Array.isArray(list)) throw new TemplateError("template_bad_value", `template value ${n.name} must be a list for #each`, { name: n.name });
      for (const item of list) out += renderNodes(n.body, [...scopes, { item, isThis: true }]);
    }
  }
  return out;
}

/** Render `source` with `data`. Throws `TemplateError` on a syntax problem, a missing value or a value of the wrong type. */
export function renderTemplate(source: string, data: TemplateData): string {
  return renderNodes(parse(source), [{ item: data, isThis: false }]);
}

// --- files ---

/** Contents of `templates/<name>` in the cpd repository. */
export function loadTemplate(name: "worker.md" | "reviewer.md"): string {
  const url = new URL(`../../templates/${name}`, import.meta.url);
  try {
    return readFileSync(url, "utf8");
  } catch (e) {
    throw failure("template_unreadable", `cannot read template ${name}: ${(e as Error).message}`, { name });
  }
}

/** Write `text` to `path` through a temp file and a rename, creating parent directories. Rewriting is idempotent. */
export function writeTextAtomic(path: string, text: string): void {
  mkdirSync(dirname(path), { recursive: true });
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, text);
  renameSync(tmp, path);
}
