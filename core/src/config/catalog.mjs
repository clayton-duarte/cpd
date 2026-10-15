// Reads the model catalog of an installed pi release (contract §9).
// Run by cpd with pi's Node: node --input-type=module -e <this file> -- \
//   <pi-coding-agent entry> <pi-ai entry> <letter>=<provider>/<model-id>…
// Prints one JSON object: letter -> the model's supported thinking levels
// (pi's own names, in pi's order), or null when pi has no such model.
import { pathToFileURL } from "node:url";

const [agentEntry, aiEntry, ...tiers] = process.argv.slice(1);
const agent = await import(pathToFileURL(agentEntry).href);
const ai = await import(pathToFileURL(aiEntry).href);
const runtime = await agent.ModelRuntime.create({ allowModelNetwork: false });
const out = {};
for (const tier of tiers) {
  const eq = tier.indexOf("=");
  const letter = tier.slice(0, eq);
  const ref = tier.slice(eq + 1);
  const slash = ref.indexOf("/");
  const model = runtime.getModel(ref.slice(0, slash), ref.slice(slash + 1));
  out[letter] = model ? ai.getSupportedThinkingLevels(model) : null;
}
process.stdout.write(JSON.stringify(out) + "\n");
