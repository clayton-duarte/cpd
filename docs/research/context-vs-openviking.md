# Research Spike: cpd Context vs OpenViking (Executive Reference)

**Primary Document**: For the complete, unabridged comparative analysis across all 8 spike areas, see:
[`docs/research/context-memory-providers.md`](context-memory-providers.md)

---

## At a Glance

This spike investigates whether cpd should replace its in-tree Context subsystem (`core/src/context/`) with **OpenViking** (`github.com/volcengine/OpenViking`) or external agent memory engines (Mem0, Honcho, Hindsight, ByteRover, Basic Memory, Holographic).

### Key Takeaways

1. **Recommendation**: **Option C with Option B refinements** (Keep cpd Context, reject external engine adoption, selectively borrow L0/L1/L2 progressive summarization).
2. **Operational Verdict**: OpenViking cannot run embedded in a single binary. It requires a long-running Python FastAPI daemon (`openviking-server` on port 1933), an asynchronous extraction worker, local or S3 filesystem storage (`ragfs`), and external LLM/embedding endpoints. This directly breaches architectural decisions `pro-d567` and `pro-d569` (one cpd process per host, machine-local, zero containers).
3. **Licensing**: OpenViking server is AGPL-3.0. Linking or wrapping it exposes internal orchestration to network-copyleft conditions.
4. **Data Model**: cpd Context records decisions and notes deterministically with monotonic `seq`, immutable tables, and explicit supersession DAGs. OpenViking models memory as a hierarchical virtual filesystem (`viking://`) with probabilistic background LLM extraction.
5. **Code Deletion Analysis**: Attempting to delete `core/src/context/` (17,432 LOC) does not net code savings; it trades reliable in-process SQLite FTS5 queries for thousands of lines of distributed RPC glue, background process supervision, and failure mitigation.

Refer to [`docs/research/context-memory-providers.md`](context-memory-providers.md) for the full breakdown.
