/*
 * pi/compact-tools: the compact-tools extension of the cpd pi adapter. Comments only.
 *
 * What it is: a copy of pi-compact-tools (nedleeds, MIT; LICENSE and NOTICE beside this file name the
 * upstream and the base commit) that draws pi's tool rows, thinking block and grouping, with the cpd
 * fold language as its defaults: fold "toggle", the cockpit's expand and collapse glyphs, and the tools
 * read, write, edit, bash, grep, find and ls, style compact. A compact-tools.json still overrides them.
 *
 * Where it lives: pi/compact-tools/extensions/ (the sources, under upstream's file names) and
 * pi/compact-tools/schemas/ (the schema of compact-tools.json). pi loads it from the "pi.extensions"
 * of pi/package.json, entry ./compact-tools/extensions/compact-tools.ts, as an extension of its own.
 * Its tests are pi/test/compact-tools/, which the install leaves out with pi/test.
 *
 * Who reads its config besides it: pi/lib/render.ts, through loadConfig of compact-tools-config.ts.
 * A double load: it stands down, loudly once, when the settings still list the npm package
 * pi-compact-tools or a git source of that name (compact-tools-double-load.ts).
 *
 * The contract is docs/contract.md section 6.9. The copy, the list of changes made to it, the tests and
 * the procedure for taking upstream changes are in docs/implementing.md, "The compact-tools extension".
 */
