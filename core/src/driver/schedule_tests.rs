//! The scheduler's contract (contract §3.7 E3 "Priority, acquisition and reservation"): the cases of the doc, and the proofs
//! of no starvation and no deadlock over every small state. Pure: no store, no clock, no randomness.

use std::collections::VecDeque;

use super::schedule::{Earmark, Pool, Ready, schedule};

fn pool(free: u32) -> Pool {
    Pool { free, urgent_free: 0 }
}

fn job(id: u32, needs: &[usize]) -> Ready {
    Ready { id, urgent: false, needs: needs.to_vec() }
}

fn urgent(id: u32, needs: &[usize]) -> Ready {
    Ready { id, urgent: true, needs: needs.to_vec() }
}

fn mark(job: u32, pool: usize) -> Earmark {
    Earmark { job, pool }
}

#[test]
fn schedule_starts_every_job_whose_resources_all_have_a_free_slot() {
    let pools = [pool(2), pool(1)];
    let ready = [job(1, &[0]), job(2, &[1]), job(3, &[0])];

    let plan = schedule(&pools, &ready);

    assert_eq!(plan.start, [1, 2, 3]);
    assert_eq!(plan.earmarks, []);
}

#[test]
fn schedule_takes_workers_and_the_spec_resources_all_or_none() {
    // Pool 0 is `workers`, pool 1 a resource the Job's Spec lists.
    let ready = [job(1, &[0, 1])];

    assert_eq!(schedule(&[pool(1), pool(1)], &ready).start, [1], "both free: it starts");
    assert_eq!(schedule(&[pool(1), pool(0)], &ready).start, Vec::<u32>::new(), "the resource is full: no Worker slot is taken either");
    assert_eq!(schedule(&[pool(0), pool(1)], &ready).start, Vec::<u32>::new(), "no Worker slot: the resource is not taken either");
    // The Worker slot the blocked Job would take is held back (earmarked) from the Job behind it.
    let behind = [job(1, &[0, 1]), job(2, &[0])];
    let plan = schedule(&[pool(1), pool(0)], &behind);
    assert_eq!((plan.start, plan.earmarks), (vec![], vec![mark(1, 0), mark(1, 1), mark(2, 0)]));
}

#[test]
fn schedule_never_starts_a_job_holding_part_of_its_resources() {
    // Three one-slot pools, three Jobs each needing two of them: only the first fits.
    let pools = [pool(1), pool(1), pool(1)];
    let ready = [job(1, &[0, 1]), job(2, &[1, 2]), job(3, &[0, 2])];
    assert_eq!(schedule(&pools, &ready).start, [1]);

    // Over every pair of Jobs on pools of 1 or 2 slots: what is started never uses more slots than a pool has free.
    for free in [[1, 1, 1], [1, 2, 1], [2, 1, 2]] {
        let pools: Vec<Pool> = free.iter().map(|&f| pool(f)).collect();
        for a in subsets(3) {
            for b in subsets(3) {
                for c in subsets(3) {
                    let ready = [job(1, &a), job(2, &b), job(3, &c)];
                    let plan = schedule(&pools, &ready);
                    let mut used = [0u32; 3];
                    for id in &plan.start {
                        for &p in &ready[*id as usize - 1].needs {
                            used[p] += 1;
                        }
                    }
                    for p in 0..3 {
                        assert!(used[p] <= free[p], "pool {p} of {free:?} over-used by {:?} for {ready:?}", plan.start);
                    }
                }
            }
        }
    }
}

#[test]
fn schedule_lower_job_backfills_slots_the_blocked_job_does_not_need() {
    // Job 1 needs pools 0 and 1; pool 1 is full. Pool 0 has a spare slot beyond Job 1's, and pool 2 is not Job 1's at all.
    let pools = [pool(2), pool(0), pool(1)];
    let plan = schedule(&pools, &[job(1, &[0, 1]), job(2, &[0]), job(3, &[2]), job(4, &[0])]);

    assert_eq!(plan.start, [2, 3], "the spare slot of pool 0 and the pool Job 1 does not need");
    assert_eq!(plan.earmarks, [mark(1, 0), mark(1, 1), mark(4, 0)]);
}

#[test]
fn schedule_blocked_job_earmarks_the_next_freed_slot_of_each_resource_it_lacks() {
    // Job 1 lacks pools 0 and 1: it earmarks the next slot freed of each. It does not lack pool 2: it holds one free slot of it
    // back. Every needed pool is earmarked once, in `needs` order (the Job starves otherwise: User, pro-d92).
    let pools = [pool(0), pool(0), pool(1), pool(1)];
    let plan = schedule(&pools, &[job(1, &[0, 1, 2]), job(2, &[2]), job(3, &[3])]);
    assert_eq!(plan.earmarks, [mark(1, 0), mark(1, 1), mark(1, 2), mark(2, 2)]);
    assert_eq!(plan.start, [3], "pool 3 is not Job 1's; pool 2's one free slot is held back for it");

    // A second Job that lacks the same pool earmarks the next freed slot after the first's: a count per resource.
    let plan = schedule(&[pool(0)], &[job(1, &[0]), job(2, &[0])]);
    assert_eq!(plan.earmarks, [mark(1, 0), mark(2, 0)]);
    assert_eq!(plan.start, Vec::<u32>::new());

    // A pool with more free slots than earmarks still serves a later Job.
    let plan = schedule(&[pool(2), pool(0)], &[job(1, &[0, 1]), job(2, &[0]), job(3, &[0])]);
    assert_eq!(plan.start, [2]);
    assert_eq!(plan.earmarks, [mark(1, 0), mark(1, 1), mark(3, 0)]);
}

#[test]
fn schedule_earmarks_follow_the_urgent_then_seq_order() {
    // The caller's order is the priority (urgent first, then seq): the first of two Jobs for the one free slot gets it,
    // the other earmarks the next.
    let pools = [pool(1)];
    let urgent_first = schedule(&pools, &[urgent(10, &[0]), job(1, &[0])]);
    assert_eq!((urgent_first.start, urgent_first.earmarks), (vec![10], vec![mark(1, 0)]));

    let seq_order = schedule(&pools, &[job(1, &[0]), job(2, &[0]), job(3, &[0])]);
    assert_eq!((seq_order.start, seq_order.earmarks), (vec![1], vec![mark(2, 0), mark(3, 0)]));

    // Earmarks come out in priority order: a blocked urgent Job's before a blocked lower Job's.
    let blocked = schedule(&[pool(0), pool(1)], &[urgent(9, &[0, 1]), job(2, &[0]), job(1, &[1, 0])]);
    assert_eq!(blocked.earmarks, [mark(9, 0), mark(9, 1), mark(2, 0), mark(1, 1), mark(1, 0)]);
}

#[test]
fn schedule_urgent_job_may_take_the_one_slot_beyond_concurrency_and_no_other_job_may() {
    // `workers` with every slot taken and the one beyond free.
    let full = [Pool { free: 0, urgent_free: 1 }];
    let plan = schedule(&full, &[urgent(1, &[0]), job(2, &[0])]);
    assert_eq!(plan.start, [1]);
    assert_eq!(plan.earmarks, [mark(2, 0)]);

    let plan = schedule(&full, &[job(1, &[0])]);
    assert_eq!((plan.start, plan.earmarks), (vec![], vec![mark(1, 0)]), "a non-urgent Job never takes it");

    // Only one: a second urgent Job waits.
    let plan = schedule(&full, &[urgent(1, &[0]), urgent(2, &[0])]);
    assert_eq!((plan.start, plan.earmarks), (vec![1], vec![mark(2, 0)]));

    // A free slot below `concurrency` goes first, then the one beyond.
    let one_free = [Pool { free: 1, urgent_free: 1 }];
    let plan = schedule(&one_free, &[urgent(1, &[0]), urgent(2, &[0]), job(3, &[0])]);
    assert_eq!((plan.start, plan.earmarks), (vec![1, 2], vec![mark(3, 0)]));

    // The slot beyond is taken with the Job's other resources, all or none.
    let two = [Pool { free: 0, urgent_free: 1 }, pool(0)];
    assert_eq!(schedule(&two, &[urgent(1, &[0, 1])]).start, Vec::<u32>::new());
}

/// A Job that needs pools A and B, with a stream of Jobs each needing A alone: while B is held, A is free and each stream Job
/// takes it. The Job starts once B is freed, whatever the stream does.
#[test]
fn schedule_multi_resource_job_starts_despite_an_endless_stream_of_single_resource_jobs() {
    const A: usize = 0;
    const B: usize = 1;
    const MULTI: u32 = 1;
    // Tenants end one per step, oldest first. B's holder is the first tenant, A's slot is free at the start.
    let mut tenants: VecDeque<(u32, Vec<usize>)> = VecDeque::from([(0, vec![B])]);
    let mut next_stream = 100;
    let mut started_at = None;
    for step in 1..=20 {
        // A tenant ends: B is held, then freed at step 3 (the holder is the oldest tenant while stream Jobs queue behind).
        if step >= 3 || tenants.len() > 1 {
            tenants.pop_front();
        }
        let used = |p: usize| u32::try_from(tenants.iter().filter(|(_, held)| held.contains(&p)).count()).unwrap();
        let pools = [pool(1 - used(A)), pool(1 - used(B))];
        // The multi-resource Job is the oldest (seq order); a new stream Job arrives at every step.
        let mut ready = vec![job(MULTI, &[A, B])];
        for _ in 0..3 {
            ready.push(job(next_stream, &[A]));
            next_stream += 1;
        }
        let plan = schedule(&pools, &ready);
        for id in &plan.start {
            let needs = ready.iter().find(|j| j.id == *id).unwrap().needs.clone();
            tenants.push_back((*id, needs));
        }
        if plan.start.contains(&MULTI) {
            started_at = Some(step);
            break;
        }
    }
    let step = started_at.expect("the Job needing A and B starts: the stream of Jobs needing A alone does not starve it");
    assert!(step <= 5, "it starts within the steps B is held plus the stream Job holding A: started at step {step}");
}

/// A state of the proofs: pools of 1..=2 slots with some slots taken by tenants that are not Jobs (they end one per step), and
/// Jobs each needing a non-empty subset of the pools.
struct State {
    caps: Vec<u32>,
    taken: Vec<u32>,
    jobs: Vec<Vec<usize>>,
}

/// Every non-empty subset of `0..pools`, as sorted lists.
fn subsets(pools: usize) -> Vec<Vec<usize>> {
    (1..(1u32 << pools)).map(|m| (0..pools).filter(|p| m >> p & 1 == 1).collect()).collect()
}

/// Every state of up to 3 pools with 1..=2 slots, any number of them taken, and 1..=4 Jobs. No randomness: a fixed enumeration.
fn states() -> Vec<State> {
    let mut all = Vec::new();
    for pools in 1..=3usize {
        let mut caps_all = vec![vec![]];
        for _ in 0..pools {
            caps_all = caps_all.into_iter().flat_map(|c| (1..=2).map(move |n| [c.clone(), vec![n]].concat())).collect();
        }
        for caps in caps_all {
            let mut taken_all = vec![vec![]];
            for cap in &caps {
                taken_all = taken_all.into_iter().flat_map(|t| (0..=*cap).map(move |n| [t.clone(), vec![n]].concat())).collect();
            }
            let subs = subsets(pools);
            let mut jobs_all: Vec<Vec<Vec<usize>>> = vec![vec![]];
            for count in 1..=4 {
                jobs_all = jobs_all.into_iter().flat_map(|j| subs.iter().map(move |s| [j.clone(), vec![s.clone()]].concat())).collect();
                for jobs in jobs_all.iter().filter(|j| j.len() == count) {
                    for taken in &taken_all {
                        all.push(State { caps: caps.clone(), taken: taken.clone(), jobs: jobs.clone() });
                    }
                }
            }
        }
    }
    all
}

/// One run of a state, a tenant ending per step (the oldest, or the newest when `newest_first`). `on_step` sees the step
/// (from 1), the tenants after the walk, the waiting Jobs' indices and what the walk started.
fn run(state: &State, newest_first: bool, mut on_step: impl FnMut(usize, &[(Option<usize>, Vec<usize>)], &[usize], &[usize])) -> Vec<Option<usize>> {
    // A tenant: the Job it is (None: an outside holder) and the pools it holds.
    let mut tenants: VecDeque<(Option<usize>, Vec<usize>)> = VecDeque::new();
    for (p, n) in state.taken.iter().enumerate() {
        for _ in 0..*n {
            tenants.push_back((None, vec![p]));
        }
    }
    let mut waiting: Vec<usize> = (0..state.jobs.len()).collect();
    let mut started: Vec<Option<usize>> = vec![None; state.jobs.len()];
    let bound = tenants.len() + state.jobs.len() + 1;
    for step in 0..=bound * 2 + 4 {
        if step > 0 {
            if newest_first { tenants.pop_back() } else { tenants.pop_front() };
        }
        let pools: Vec<Pool> = state
            .caps
            .iter()
            .enumerate()
            .map(|(p, cap)| pool(cap - u32::try_from(tenants.iter().filter(|(_, held)| held.contains(&p)).count()).unwrap()))
            .collect();
        // No Job arrives: the Ready Jobs are the waiting ones, in their order.
        let ready: Vec<Ready> = waiting.iter().map(|&i| job(u32::try_from(i).unwrap(), &state.jobs[i])).collect();
        let plan = schedule(&pools, &ready);
        let begun: Vec<usize> = plan.start.iter().map(|&id| id as usize).collect();
        if let Some(&head) = waiting.iter().find(|i| !begun.contains(i)) {
            for &p in &state.jobs[head] {
                let taken_by_lower = begun.iter().filter(|&&i| i > head && state.jobs[i].contains(&p)).count();
                assert!(
                    taken_by_lower == 0 || pools[p].free as usize > taken_by_lower,
                    "Job {head}, blocked, lost the free slot of pool {p} to a lower Job: jobs {:?} caps {:?} taken {:?}",
                    state.jobs, state.caps, state.taken
                );
            }
        }
        for &i in &begun {
            tenants.push_back((Some(i), state.jobs[i].clone()));
            started[i] = Some(step);
        }
        waiting.retain(|i| !begun.contains(i));
        let snapshot: Vec<_> = tenants.iter().cloned().collect();
        on_step(step, &snapshot, &waiting, &begun);
        if waiting.is_empty() {
            break;
        }
    }
    started
}

/// **No starvation.** No Job arrives, a tenant ends per step (the oldest in one run, the newest in another) and each step the freed slot
/// is offered to the walk. Every Job starts within `tenants + jobs + resources it needs` steps: the tenants that exist at the start
/// (each ends within one step of its turn), every Job of the state (each is a tenant at most once; the ones ahead of it must start
/// first, the ones behind it may backfill and so queue up to end before it, which the order of ending makes count), and one freeing
/// for each of its own resources. Each walk also keeps the head of the line: the highest Job still waiting never loses a free slot
/// of a pool it needs to a lower Job (it is what stops two staggered streams from starving a Job that needs both pools).
#[test]
fn schedule_no_starvation_in_every_small_state() {
    let mut checked = 0;
    for state in states() {
        let initial: usize = state.taken.iter().map(|&n| n as usize).sum();
        for newest_first in [false, true] {
            let started = run(&state, newest_first, |_, _, _, _| {});
            for (i, needs) in state.jobs.iter().enumerate() {
                let bound = initial + state.jobs.len() + needs.len();
                match started[i] {
                    Some(step) => assert!(step <= bound, "Job {i} started at step {step}, bound {bound}: {:?} caps {:?} taken {:?}", state.jobs, state.caps, state.taken),
                    None => panic!("Job {i} never started: jobs {:?} caps {:?} taken {:?} (newest_first {newest_first})", state.jobs, state.caps, state.taken),
                }
            }
            checked += 1;
        }
    }
    assert!(checked > 100_000, "the enumeration covers every small state: {checked}");
}

/// **No deadlock.** After every walk: when no Job could start and some Job waits, a tenant exists (so a slot will be freed:
/// progress is possible); and no Job that waits holds a slot (a Job holds all its resources or none), so Jobs never wait on each other.
#[test]
fn schedule_no_deadlock_in_every_small_state() {
    for state in states() {
        for newest_first in [false, true] {
            run(&state, newest_first, |step, tenants, waiting, begun| {
                let ctx = format!("step {step}: jobs {:?} caps {:?} taken {:?}", state.jobs, state.caps, state.taken);
                if begun.is_empty() && !waiting.is_empty() {
                    assert!(!tenants.is_empty(), "no Job can start, some wait, and no tenant exists: {ctx}");
                }
                for &w in waiting {
                    assert!(tenants.iter().all(|(who, _)| *who != Some(w)), "waiting Job {w} holds a slot: {ctx}");
                }
                // The tenants are only outside holders and Jobs that started: none waits on a Job that waits.
                let held: usize = tenants.iter().map(|(_, pools)| pools.len()).sum();
                assert!(held <= state.caps.iter().map(|&c| c as usize).sum::<usize>(), "a pool holds more than its slots: {ctx}");
            });
        }
    }
}
