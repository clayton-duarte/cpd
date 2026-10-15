// Scratch extension for the RPC trace: exercises the extension UI sub-protocol without a model call.
export default function (pi: any) {
  pi.registerCommand("demo", {
    description: "RPC extension UI demo",
    handler: async (_args: string, ctx: any) => {
      ctx.ui.setStatus("demo", "demo running");
      const ok = await ctx.ui.confirm("Proceed?", "Demo confirm");
      const pick = await ctx.ui.select("Pick one", ["approve", "reject"]);
      ctx.ui.notify(`mode=${ctx.mode} hasUI=${ctx.hasUI} confirmed=${ok} picked=${pick}`, "info");
      ctx.ui.setStatus("demo", undefined);
    },
  });
}
