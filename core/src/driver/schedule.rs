//! E3's reservation walk (contract §3.7 E3 "Priority, acquisition and reservation", Plan `resource-slots` A2–A4) as a
//! PURE function: no store, no clock, no files. E3 reads the Ledger and the lock files into [`Pool`]s and [`Ready`]
//! Jobs, calls [`schedule`], and starts the Jobs it names, in order, each taking all its resources or none.

/// One pool of slots as E3 sees it at this tick, `workers` included.
///
/// `workers`: `free` = `concurrency` minus the Attempts not `ended` (never below 0), and `urgent_free` = 1 while those
/// Attempts are at most `concurrency` (the one slot beyond, which only an urgent Job may take), else 0. Every other
/// pool: `free` = its slots no holder has, `urgent_free` = 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    /// Slots any Job may take.
    pub free: u32,
    /// Slots only an urgent Job may take, after the `free` ones.
    pub urgent_free: u32,
}

/// A Ready Job, as E3 lists it: urgent Jobs first, then by `jobs.seq` (the caller's order is the priority).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub id: u32,
    pub urgent: bool,
    /// Indices into the pools slice: the resources the Job needs, `workers` included. Never empty, no repeats.
    pub needs: Vec<usize>,
}

/// One slot of `pool` that `job` earmarks: the next one freed of that resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Earmark {
    pub job: u32,
    pub pool: usize,
}

/// What E3 does this tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schedule {
    /// The ids of the Jobs to start, in `ready`'s order.
    pub start: Vec<u32>,
    /// The earmarks of the blocked Jobs, in `ready`'s order, then in the order of each Job's `needs`.
    pub earmarks: Vec<Earmark>,
}

/// Walks `ready` in order. A Job starts when every pool it needs has more slots it may take than earmarks of the
/// blocked Jobs before it; else it is blocked and earmarks one slot of EVERY pool it needs (a pool it lacks: the next
/// slot freed; another: one free slot, held back), and a later Job uses a pool only on slots beyond the earmarks
/// (backfill: pools the blocked Job does not need, and spare slots). All or none: a started Job takes one slot of
/// each of its pools.
pub fn schedule(pools: &[Pool], ready: &[Ready]) -> Schedule {
    let mut left: Vec<Pool> = pools.to_vec();
    let mut earmarked = vec![0u32; pools.len()];
    let mut plan = Schedule::default();
    for job in ready {
        let may_take = |p: usize, left: &[Pool]| left[p].free + if job.urgent { left[p].urgent_free } else { 0 };
        if job.needs.iter().all(|&p| may_take(p, &left) > earmarked[p]) {
            for &p in &job.needs {
                if left[p].free > 0 {
                    left[p].free -= 1;
                } else {
                    left[p].urgent_free -= 1;
                }
            }
            plan.start.push(job.id);
        } else {
            for &p in &job.needs {
                earmarked[p] += 1;
                plan.earmarks.push(Earmark { job: job.id, pool: p });
            }
        }
    }
    plan
}
