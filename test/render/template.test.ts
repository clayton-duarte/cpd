import { test } from "node:test";
import assert from "node:assert/strict";
import { renderTemplate, TemplateError } from "../../src/render/template.ts";

const fails = (code: string) => (e: unknown) => e instanceof TemplateError && e.code === code && e.exitCode === 1;

test("substitutes names; numbers and booleans are printed", () => {
  assert.equal(renderTemplate("a {{x}} b {{n}} {{ok}}", { x: "X", n: 2, ok: true }), "a X b 2 true");
});

test("each iterates strings via this and objects via their fields, with outer names visible", () => {
  const t = "{{#each items}}{{wf}}:{{this}};{{/each}}|{{#each rows}}{{wf}}/{{a}}/{{b}};{{/each}}";
  assert.equal(renderTemplate(t, { wf: "W", items: ["x", "y"], rows: [{ a: 1, b: "p" }, { a: 2, b: "q" }] }), "W:x;W:y;|W/1/p;W/2/q;");
});

test("if is true for non-empty strings and arrays, true and non-zero numbers", () => {
  const t = "{{#if v}}yes{{/if}}";
  for (const v of ["a", ["a"], true, 1]) assert.equal(renderTemplate(t, { v }), "yes", JSON.stringify(v));
  for (const v of ["", [], false, 0]) assert.equal(renderTemplate(t, { v }), "", JSON.stringify(v));
});

test("blocks nest", () => {
  const t = "{{#each rows}}[{{#if show}}{{name}}{{/if}}]{{/each}}";
  assert.equal(renderTemplate(t, { rows: [{ show: true, name: "a" }, { show: false, name: "b" }] }), "[a][]");
});

test("a block tag alone on its line takes the line with it", () => {
  const t = "head\n{{#each xs}}\n- {{this}}\n{{/each}}\n{{#if no}}\nhidden\n{{/if}}\ntail\n";
  assert.equal(renderTemplate(t, { xs: ["a", "b"], no: false }), "head\n- a\n- b\ntail\n");
});

test("substituted text is not scanned again for tags", () => {
  assert.equal(renderTemplate("{{x}}", { x: "{{y}}" }), "{{y}}");
});

test("a missing value is template_missing_value, naming it", () => {
  assert.throws(() => renderTemplate("{{nope}}", {}), (e: unknown) => fails("template_missing_value")(e) && (e as TemplateError).details.name === "nope");
  assert.throws(() => renderTemplate("{{x}}", { x: undefined }), fails("template_missing_value"));
  assert.throws(() => renderTemplate("{{x}}", { x: null }), fails("template_missing_value"));
  assert.throws(() => renderTemplate("{{#if nope}}a{{/if}}", {}), fails("template_missing_value"));
  assert.throws(() => renderTemplate("{{#each nope}}a{{/each}}", {}), fails("template_missing_value"));
});

test("an item field left empty does not fall back to an outer value of the same name", () => {
  assert.throws(() => renderTemplate("{{#each rows}}{{tier}}{{/each}}", { tier: "S3", rows: [{ tier: undefined }] }), fails("template_missing_value"));
});

test("an empty string is a value, not a missing one", () => {
  assert.equal(renderTemplate("[{{x}}]", { x: "" }), "[]");
});

test("a value of the wrong type is template_bad_value", () => {
  assert.throws(() => renderTemplate("{{x}}", { x: ["a"] }), fails("template_bad_value"));
  assert.throws(() => renderTemplate("{{x}}", { x: { a: 1 } }), fails("template_bad_value"));
  assert.throws(() => renderTemplate("{{#each x}}a{{/each}}", { x: "str" }), fails("template_bad_value"));
});

test("syntax errors are template_syntax: unknown constructs, unbalanced blocks", () => {
  for (const t of ["{{x.y}}", "{{ x y }}", "{{!comment}}", "{{#each}}a{{/each}}", "{{#with x}}a{{/with}}", "{{/each}}", "{{#each x}}a", "{{#each x}}a{{/if}}", "{{#if x}}{{#each y}}{{/if}}{{/each}}"]) {
    assert.throws(() => renderTemplate(t, { x: [], y: [] }), fails("template_syntax"), t);
  }
});
