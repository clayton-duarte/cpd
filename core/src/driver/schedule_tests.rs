//! The scheduler's contract (contract §3.7 E3 "the queue walk", pro-d106): the cases of the doc, and the proofs of the bound on
//! waiting, of no deadlock and of no idle slot over every small state. Pure: no store, no clock, no randomness.

use std::collections::VecDeque;

use super::schedule::{Pool, Ready, schedule};

fn pool(free: u32) -> Pool {
    Pool {
        free,
        urgent_free: 0,
    }
}

/// A Job of the queue: its `seq` is its id.
fn job(id: u32, needs: &[usize]) -> Ready {
    Ready {
        id,
        urgent: false,
        skipped: None,
        seq: id,
        tenant: false,
        needs: needs.to_vec(),
    }
}

fn urgent(id: u32, needs: &[usize]) -> Ready {
    Ready {
        urgent: true,
        ..job(id, needs)
    }
}

/// A Job E3 skipped before, with skip order `n`.
fn skipped(id: u32, n: u32, needs: &[usize]) -> Ready {
    Ready {
        skipped: Some(n),
        ..job(id, needs)
    }
}

/// A Job that is already a tenant: a retry.
fn tenant(id: u32, needs: &[usize]) -> Ready {
    Ready {
        tenant: true,
        ..job(id, needs)
    }
}

#[test]
fn schedule_orders_urgent_then_skipped_then_queue() {
    // The caller's order is nothing: the Jobs come in scrambled. Urgent 6 (seq 6), skipped 5 (n 1), queue 1, 2 and 3.
    let scrambled = [
        job(2, &[0]),
        skipped(5, 1, &[0]),
        job(1, &[0]),
        urgent(6, &[0]),
        job(3, &[0]),
    ];

    let plan = schedule(&[pool(3)], &scrambled);
    assert_eq!(
        plan.start,
        [6, 5, 1],
        "urgent, then skipped, then the queue by seq; three slots"
    );
    assert_eq!(
        plan.skipped,
        [2, 3],
        "the Jobs that found no slot, in the order of the walk"
    );

    // One slot: the urgent Job takes it; the skipped Job and the queue are skipped, in the order of the walk.
    let plan = schedule(&[pool(1)], &scrambled);
    assert_eq!((plan.start, plan.skipped), (vec![6], vec![5, 1, 2, 3]));

    // Two urgent Jobs go by seq among themselves, ahead of the skipped Job.
    let plan = schedule(
        &[pool(2)],
        &[skipped(1, 1, &[0]), urgent(8, &[0]), urgent(7, &[0])],
    );
    assert_eq!((plan.start, plan.skipped), (vec![7, 8], vec![1]));
}

#[test]
fn schedule_skipped_jobs_keep_first_skipped_first_served_order() {
    // Skip orders 1, 2, 3 on seqs 3, 1, 2: the skip order wins over the seq.
    let ready = [
        skipped(1, 2, &[0]),
        skipped(2, 3, &[0]),
        skipped(3, 1, &[0]),
        job(4, &[0]),
    ];

    let plan = schedule(&[pool(2)], &ready);
    assert_eq!(plan.start, [3, 1], "first skipped, first served");
    assert_eq!(plan.skipped, [2, 4]);

    // The skipped Job ahead that cannot start does not stop the one after it: it is skipped again, and keeps its place.
    let plan = schedule(
        &[pool(1), pool(0)],
        &[skipped(1, 1, &[0, 1]), skipped(2, 2, &[0]), job(3, &[0])],
    );
    assert_eq!((plan.start, plan.skipped), (vec![2], vec![1, 3]));
}

#[test]
fn schedule_skips_a_job_that_misses_a_resource_and_reserves_nothing() {
    // Pool 0 is `workers` (2 free), pool 1 is `okta-port` (none free). Job 1 misses the port: it is skipped, and the two
    // workers it would take are free for the Jobs behind it, which both start.
    let plan = schedule(
        &[pool(2), pool(0)],
        &[job(1, &[0, 1]), job(2, &[0]), job(3, &[0])],
    );
    assert_eq!(plan.start, [2, 3]);
    assert_eq!(plan.skipped, [1]);

    // Not blocked, not held back: even a Job of the same missing resource behind it is only skipped in its turn.
    let plan = schedule(&[pool(2), pool(0)], &[job(1, &[0, 1]), job(2, &[0, 1])]);
    assert_eq!((plan.start, plan.skipped), (vec![], vec![1, 2]));

    // A pool freed after the walk is nobody's: the next walk offers it to the Jobs in order, the skipped one first.
    let plan = schedule(&[pool(1), pool(1)], &[job(2, &[0]), skipped(1, 1, &[0, 1])]);
    assert_eq!((plan.start, plan.skipped), (vec![1], vec![2]));
}

#[test]
fn schedule_next_job_uses_the_slot_the_skipped_job_does_not_need() {
    // `workers` 1 free, `a` none, `b` 1 free. Job 1 (a) is skipped; Job 2 takes `workers` and `b`; Job 3 needs `b` and `workers`
    // and finds neither; Job 4 needs only `a` and is skipped too.
    let ready = [
        job(1, &[0, 1]),
        job(2, &[0, 2]),
        job(3, &[0, 2]),
        job(4, &[1]),
    ];
    let plan = schedule(&[pool(1), pool(0), pool(1)], &ready);
    assert_eq!(plan.start, [2]);
    assert_eq!(plan.skipped, [1, 3, 4]);

    // Two workers: the second takes the slot of `workers` the skipped Job does not need.
    let plan = schedule(&[pool(2), pool(0), pool(1)], &ready);
    assert_eq!(
        (plan.start, plan.skipped),
        (vec![2], vec![1, 3, 4]),
        "`b` has one slot: Job 3 lacks it"
    );
    let plan = schedule(&[pool(2), pool(0), pool(2)], &ready);
    assert_eq!((plan.start, plan.skipped), (vec![2, 3], vec![1, 4]));
}

#[test]
fn schedule_takes_workers_and_the_spec_resources_all_or_none() {
    // Pool 0 is `workers`, pool 1 a resource the Job's Spec lists.
    let ready = [job(1, &[0, 1])];

    assert_eq!(
        schedule(&[pool(1), pool(1)], &ready).start,
        [1],
        "both free: it starts"
    );
    let plan = schedule(&[pool(1), pool(0)], &ready);
    assert_eq!(
        (plan.start, plan.skipped),
        (vec![], vec![1]),
        "the resource is full: no Worker slot is taken either"
    );
    let plan = schedule(&[pool(0), pool(1)], &ready);
    assert_eq!(
        (plan.start, plan.skipped),
        (vec![], vec![1]),
        "no Worker slot: the resource is not taken either"
    );
    // What the skipped Job would have taken stays free for the Job behind it.
    let plan = schedule(&[pool(1), pool(0)], &[job(1, &[0, 1]), job(2, &[0])]);
    assert_eq!((plan.start, plan.skipped), (vec![2], vec![1]));

    // Three one-slot pools, three Jobs each needing two of them: only the first fits, and no pool is over-used.
    let plan = schedule(
        &[pool(1), pool(1), pool(1)],
        &[job(1, &[0, 1]), job(2, &[1, 2]), job(3, &[0, 2])],
    );
    assert_eq!(plan.start, [1]);
    assert_eq!(plan.skipped, [2, 3]);
}

#[test]
fn schedule_a_tenant_job_retrying_starts_without_competing() {
    // Every pool is full, and the retry needs none: it starts and takes nothing.
    let plan = schedule(&[pool(0), pool(0)], &[tenant(1, &[0, 1])]);
    assert_eq!((plan.start, plan.skipped), (vec![1], vec![]));

    // It takes no slot: the Job behind it still finds the one free slot.
    let plan = schedule(&[pool(1), pool(1)], &[tenant(1, &[0, 1]), job(2, &[0, 1])]);
    assert_eq!((plan.start, plan.skipped), (vec![1, 2], vec![]));

    // A tenant is never skipped, whatever its place: behind a skipped Job and an urgent one.
    let plan = schedule(
        &[pool(0)],
        &[job(1, &[0]), tenant(2, &[0]), urgent(3, &[0])],
    );
    assert_eq!((plan.start, plan.skipped), (vec![2], vec![3, 1]));
}

#[test]
fn schedule_urgent_job_may_take_the_one_worker_beyond_concurrency_and_no_other_job_may() {
    // `workers` with every slot taken and the one beyond free.
    let full = [Pool {
        free: 0,
        urgent_free: 1,
    }];
    let plan = schedule(&full, &[urgent(1, &[0]), job(2, &[0])]);
    assert_eq!((plan.start, plan.skipped), (vec![1], vec![2]));

    let plan = schedule(&full, &[job(1, &[0])]);
    assert_eq!(
        (plan.start, plan.skipped),
        (vec![], vec![1]),
        "a non-urgent Job never takes it"
    );

    // Only one: a second urgent Job waits.
    let plan = schedule(&full, &[urgent(1, &[0]), urgent(2, &[0])]);
    assert_eq!((plan.start, plan.skipped), (vec![1], vec![2]));

    // A free slot below `concurrency` goes first, then the one beyond; a Job of the queue never gets the one beyond.
    let one_free = [Pool {
        free: 1,
        urgent_free: 1,
    }];
    let plan = schedule(&one_free, &[urgent(1, &[0]), urgent(2, &[0]), job(3, &[0])]);
    assert_eq!((plan.start, plan.skipped), (vec![1, 2], vec![3]));

    // The slot beyond is taken with the Job's other resources, all or none.
    let two = [
        Pool {
            free: 0,
            urgent_free: 1,
        },
        pool(0),
    ];
    let plan = schedule(&two, &[urgent(1, &[0, 1])]);
    assert_eq!((plan.start, plan.skipped), (vec![], vec![1]));
}

/// A state of the proofs. Pool 0 is `workers`; pools `1..` are resources. Every pool has 1..=2 slots. The tenants at the start
/// are Jobs (each takes one `workers` slot and the resources it lists) and they end one per step. The Jobs each need `workers` and a
/// non-empty subset of the resources.
struct State {
    caps: Vec<u32>,
    /// The tenants at the start: the resources each holds (`workers` is implied).
    tenants: Vec<Vec<usize>>,
    jobs: Vec<Vec<usize>>,
}

/// Every non-empty subset of the resources `1..=resources`, as sorted lists.
fn subsets(resources: usize) -> Vec<Vec<usize>> {
    (1..(1u32 << resources))
        .map(|m| (1..=resources).filter(|p| m >> (p - 1) & 1 == 1).collect())
        .collect()
}

/// Every state of up to 3 pools (`workers` and 1 or 2 resources) with 1..=2 slots, up to 3 tenants that fit, and 1..=4 Jobs. No
/// randomness: a fixed enumeration.
fn states() -> Vec<State> {
    let mut all = Vec::new();
    for resources in 1..=2usize {
        let subs = subsets(resources);
        let mut holds = vec![vec![]];
        holds.extend(subs.iter().cloned());
        let mut caps_all = vec![vec![]];
        for _ in 0..=resources {
            caps_all = caps_all
                .into_iter()
                .flat_map(|c| (1..=2).map(move |n| [c.clone(), vec![n]].concat()))
                .collect();
        }
        let mut tenants_all: Vec<Vec<Vec<usize>>> = vec![vec![]];
        let mut by_count: Vec<Vec<Vec<usize>>> = vec![vec![]];
        for _ in 0..3 {
            tenants_all = tenants_all
                .into_iter()
                .flat_map(|t| {
                    holds
                        .iter()
                        .map(move |h| [t.clone(), vec![h.clone()]].concat())
                })
                .collect();
            by_count.extend(tenants_all.iter().cloned());
        }
        let mut jobs_all: Vec<Vec<Vec<usize>>> = vec![vec![]];
        let mut job_sets = Vec::new();
        for _ in 1..=4 {
            jobs_all = jobs_all
                .into_iter()
                .flat_map(|j| {
                    subs.iter()
                        .map(move |s| [j.clone(), vec![s.clone()]].concat())
                })
                .collect();
            job_sets.extend(jobs_all.iter().cloned());
        }
        for caps in &caps_all {
            for tenants in by_count.iter().filter(|t| fits(caps, t)) {
                for jobs in &job_sets {
                    all.push(State {
                        caps: caps.clone(),
                        tenants: tenants.clone(),
                        jobs: jobs.clone(),
                    });
                }
            }
        }
    }
    all
}

/// Whether `tenants` fit in `caps` (each takes a `workers` slot and its resources).
fn fits(caps: &[u32], tenants: &[Vec<usize>]) -> bool {
    let mut used = vec![0u32; caps.len()];
    for held in tenants {
        used[0] += 1;
        for &p in held {
            used[p] += 1;
        }
    }
    used.iter().zip(caps).all(|(u, c)| u <= c)
}

/// What a step of [`run`] shows `on_step`.
struct Step<'a> {
    number: usize,
    /// The free slots of each pool when the walk began.
    free: &'a [u32],
    /// The tenants after the walk: the Job index (None: one of the start) and the pools it holds, `workers` included.
    tenants: &'a [(Option<usize>, Vec<usize>)],
    /// The Jobs still waiting after the walk.
    waiting: &'a [usize],
    /// The Jobs the walk started.
    begun: &'a [usize],
    /// The Jobs the walk reported skipped.
    reported: &'a [u32],
}

/// One run of a state: a tenant ends per step (the oldest, or the newest when `newest_first`), no Job arrives, and each step the
/// freed slots are offered to the walk, whose skip order the run keeps as E3 does (a number above every other, once, cleared at
/// the start). Returns the step each Job started at (None: never), within a number of steps that is generous.
fn run(state: &State, newest_first: bool, mut on_step: impl FnMut(&Step)) -> Vec<Option<usize>> {
    let mut tenants: VecDeque<(Option<usize>, Vec<usize>)> = state
        .tenants
        .iter()
        .map(|held| (None, [vec![0], held.clone()].concat()))
        .collect();
    let mut waiting: Vec<usize> = (0..state.jobs.len()).collect();
    let mut order: Vec<Option<u32>> = vec![None; state.jobs.len()];
    let mut next_order = 1;
    let mut started: Vec<Option<usize>> = vec![None; state.jobs.len()];
    for step in 0..=(tenants.len() + state.jobs.len()) * 2 + 4 {
        if step > 0 {
            if newest_first {
                tenants.pop_back()
            } else {
                tenants.pop_front()
            };
        }
        let free: Vec<u32> = state
            .caps
            .iter()
            .enumerate()
            .map(|(p, cap)| {
                cap - u32::try_from(tenants.iter().filter(|(_, held)| held.contains(&p)).count())
                    .unwrap()
            })
            .collect();
        let pools: Vec<Pool> = free.iter().map(|&f| pool(f)).collect();
        let ready: Vec<Ready> = waiting
            .iter()
            .map(|&i| Ready {
                skipped: order[i],
                ..job(
                    u32::try_from(i).unwrap(),
                    &[vec![0], state.jobs[i].clone()].concat(),
                )
            })
            .collect();
        let plan = schedule(&pools, &ready);
        let begun: Vec<usize> = plan.start.iter().map(|&id| id as usize).collect();
        for &id in &plan.skipped {
            let i = id as usize;
            if order[i].is_none() {
                order[i] = Some(next_order);
                next_order += 1;
            }
        }
        for &i in &begun {
            tenants.push_back((Some(i), [vec![0], state.jobs[i].clone()].concat()));
            started[i] = Some(step);
            order[i] = None;
        }
        waiting.retain(|i| !begun.contains(i));
        let snapshot: Vec<_> = tenants.iter().cloned().collect();
        on_step(&Step {
            number: step,
            free: &free,
            tenants: &snapshot,
            waiting: &waiting,
            begun: &begun,
            reported: &plan.skipped,
        });
        if waiting.is_empty() {
            break;
        }
    }
    started
}

fn describe(state: &State, newest_first: bool) -> String {
    format!(
        "jobs {:?} caps {:?} tenants {:?} (newest_first {newest_first})",
        state.jobs, state.caps, state.tenants
    )
}

/// **The bound on waiting** (contract §3.7 E3). A Job that needs `workers` and ONE other pool starts within
/// `tenants at the start + Jobs of the state + 1` steps while no new urgent Job arrives: a step ends one tenant, every tenant is
/// one of the start or a Job that started (once), the skipped Jobs ahead of it are walked first, and the freed slot of its pool
/// and a worker come together. The bound is stated for a Job of ONE extra resource only: the contract promises no more.
#[test]
fn schedule_job_needing_one_extra_resource_starts_when_its_tenant_ends_in_every_small_state() {
    let mut checked = 0;
    for state in states() {
        for newest_first in [false, true] {
            let started = run(&state, newest_first, |_| {});
            for (i, needs) in state.jobs.iter().enumerate().filter(|(_, n)| n.len() == 1) {
                let bound = state.tenants.len() + state.jobs.len() + 1;
                match started[i] {
                    Some(step) => assert!(
                        step <= bound,
                        "Job {i} ({needs:?}) started at step {step}, bound {bound}: {}",
                        describe(&state, newest_first)
                    ),
                    None => panic!(
                        "Job {i} ({needs:?}) never started: {}",
                        describe(&state, newest_first)
                    ),
                }
                checked += 1;
            }
        }
    }
    assert!(
        checked > 50_000,
        "the enumeration covers every small state: {checked}"
    );
}

/// **No deadlock.** After every walk: when a Job waits and no Job could start, a tenant exists whose end is the way on (a slot
/// will be freed); no Job that waits is a tenant (it holds nothing, so Jobs never wait on each other); no pool holds more than its
/// slots; and every Job starts in the end.
#[test]
fn schedule_no_deadlock_in_every_small_state() {
    for state in states() {
        for newest_first in [false, true] {
            let started = run(&state, newest_first, |step| {
                let ctx = format!("step {}: {}", step.number, describe(&state, newest_first));
                if step.begun.is_empty() && !step.waiting.is_empty() {
                    assert!(
                        !step.tenants.is_empty(),
                        "no Job can start, some wait, and no tenant exists: {ctx}"
                    );
                }
                for &w in step.waiting {
                    assert!(
                        step.tenants.iter().all(|(who, _)| *who != Some(w)),
                        "waiting Job {w} holds a slot: {ctx}"
                    );
                }
                let mut used = vec![0usize; state.caps.len()];
                for (_, held) in step.tenants {
                    for &p in held {
                        used[p] += 1;
                    }
                }
                for (p, cap) in state.caps.iter().enumerate() {
                    assert!(
                        used[p] <= *cap as usize,
                        "pool {p} holds more than its {cap} slots: {ctx}"
                    );
                }
            });
            for (i, at) in started.iter().enumerate() {
                assert!(
                    at.is_some(),
                    "Job {i} never started: {}",
                    describe(&state, newest_first)
                );
            }
        }
    }
}

/// **No idle slot.** After every walk no Job is left skipped while every pool it needs has a free slot it may take: for each Job
/// still waiting, some pool it needs has no slot left after what the walk started. The Jobs the walk reports skipped are exactly
/// the Jobs that wait.
#[test]
fn schedule_leaves_no_free_slot_idle_that_a_walked_job_could_use() {
    for state in states() {
        for newest_first in [false, true] {
            run(&state, newest_first, |step| {
                let ctx = format!("step {}: {}", step.number, describe(&state, newest_first));
                let mut left = step.free.to_vec();
                for &i in step.begun {
                    for &p in &[vec![0], state.jobs[i].clone()].concat() {
                        left[p] -= 1;
                    }
                }
                for &w in step.waiting {
                    let needs = [vec![0], state.jobs[w].clone()].concat();
                    assert!(
                        needs.iter().any(|&p| left[p] == 0),
                        "Job {w} is left skipped with a free slot in every pool it needs ({left:?}): {ctx}"
                    );
                }
                let mut reported: Vec<usize> = step.reported.iter().map(|&i| i as usize).collect();
                reported.sort_unstable();
                assert_eq!(
                    reported, step.waiting,
                    "the skipped Jobs are the waiting ones: {ctx}"
                );
            });
        }
    }
}
