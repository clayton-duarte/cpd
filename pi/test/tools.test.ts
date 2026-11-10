import assert from "node:assert/strict";
import { test } from "node:test";

// docs/contract.md §5.9 (Typed pi tools, context curation and the message bus).
// Pins:
// - every namespace of the table is one tool, and each op sends the method <domain>.<verb>
//   with the parameters the generated schema allows over run/core.sock;
// - hello binds the connection to the Session's role, and a request before it is bad_request;
// - a Worker's decide is not_permitted.

test("typed tools call core socket", async () => {
  // When implemented, the extension registers the 12 domain tools (cpd_plan, cpd_job, cpd_context, cpd_message, etc.)
  // and tool invocations connect to run/core.sock using NDJSON framing.
  // Before implementation, registration and socket dispatch fail.
  assert.fail("typed tools over core socket not yet implemented");
});

