# Why now: the history of the test setup, GitHub Actions and the leak check (Q1, rs-hist)

Attempt rs-hist-a3, 2026-10-26. Machine `pro` (macOS, 14 cores), cargo-nextest 0.9.148. Labels: `verified` (command or file:line), `inference`, `unverified`.

## Verdict

1. **The LEAK is new on this setup; it is not an old weakness made visible by `result = "fail"`.** Nextest ran on GitHub Actions for 1 day only (2026-10-18 17:25 to 2026-10-19 00:36 EDT, 61 nextest logs, 59 finished, 92,238 test executions) and printed `LEAK` 0 times, a passing leaky test included (default `result = "pass"` still prints `LEAK`). Verified: `gh api .../jobs/<id>/logs` over every run since the nextest commit (Evidence 2).
2. **D, confirmed (the strongest cause): a different machine and a different load.** CI = one run alone per fresh VM, `ubuntu-24.04` (verified), 1,410-1,727 tests, 282-656 s. The Gate on `pro` = macOS, 14 threads, 3,237 to 5,169 tests, 4-5 Gates at once at load 55-100. The Gate also ran on hs (Linux, 16 cores, 1,307 tests, 0 LEAK in 3 runs, commit ba01e4fc) before the Mac.
3. **B, confirmed (contributing): the suite grew 1,307 (10-18) to 5,137 (10-25), 3.9 times, in 7 days.** More processes per run = more chances for the 100 ms timer to fire late.
4. **C/F, confirmed (contributing, a new finding): test slots capped full runs at 2 from 2026-10-20 (fa7fb29b) to 2026-10-25 18:14 EDT (84c274bf, #425).** Gates now run at the merge and the slots are gone. Gate runs overlapping on pro: max 2 on 10-22..10-24, max 4 on 10-25. Of 8 LEAK runs 6 fell on 10-25; but 4 of the 8 overlapped no other nextest run: other load (builds, scanners) also counts (inference).
5. **A, killed as the cause, confirmed as a fact:** `result = "fail"` was added 2026-10-20 (739ec1cc) for a real leak (`ms-leak`, a fake engine's `sleep`). It made leaks fail runs; it did not create them. Logs before it do not exist on this machine (earliest 2026-10-20 14:40), so the pass-LEAK count there is 0 of 0 logs (unverified for macOS before 10-22).
6. **E, killed:** nextest 0.9.148 is pinned from the first nextest commit (CI log `cargo-nextest 0.9.148`, same on `pro`); no upgrade happened. rustc: not pinned, CI used `stable` (unverified which).
7. For the User: the leak check is stricter than any CI ever was, the machine that runs it is new, and the guard that held Gates down (slots) was removed 10-25. Nothing says GitHub-hosted runners would leak: they never saw one in 92k executions at 2 threads (2 vCPU: unverified, GitHub's documented size for private repos).
