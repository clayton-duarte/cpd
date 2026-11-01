# The two clocks

cpd runs two clocks, not one (hs-d802, `cpd context show --record hs-d802`, item 5): the **engine loop clock** (`watch.tick_seconds`) and the **GitHub poll clock** (`watch.pr_poll_seconds`, engine step E5). They are decoupled on purpose: the engine sleeps until the earlier of its next tick and E5's due time, so the poll is never held to a tick boundary ([contract §3.7](../contract.md)).

Skeleton; content follows.
