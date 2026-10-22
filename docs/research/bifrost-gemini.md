# bifrost-gemini: gemini-3.8-flash on Bifrost (spike2-gemini-a1)

Checkpoint (verified, mid-run). Findings so far: the 22 s to 84 s reply is intermittent (`fluke`-like tail: 5 of about 60 curl runs took 6 s to 141 s, on several Gemini models), and the gateway injects about 1500 tokens of its own MCP tools (`listToolFiles`, `readToolFile`, `getTool`) into every request ("hi" = 1508 prompt tokens; with header `x-bf-mcp-include-clients: none-such` it is 1). Sections G1 to G6 follow; each ends with a `verdict:` line.

## G1. Latency
not done: in progress

## G2. Routes and thinking
not done: in progress

## G3. Cache
not done: in progress

## G4. Auxiliaries
not done: in progress

## G5. The block
not done: in progress

## G6. Risks and unknowns
not done: in progress
