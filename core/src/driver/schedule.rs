//! E3's queue walk (contract §3.7 E3 "the queue walk", Plan `resource-slots`, pro-d106) as a PURE function: no store, no
//! clock, no files. E3 reads the Ledger and the lock files into [`Pool`]s and [`Ready`] Jobs, calls [`schedule`], and starts
//! the Jobs it names, in order, each taking all its resources or none. Nothing is reserved: a Job that misses a pool is
//! SKIPPED, the walk goes on, and the skipped Jobs come back before the queue.

/// One pool of slots as E3 sees it at this tick, `workers` included.
///
/// `workers`: `free` = `concurrency` minus the tenant Jobs (never below 0; a `failed` Job and a retried `queued` Job are
/// tenants), and `urgent_free` = 1 while those tenants are at most `concurrency` (the one slot beyond, which only an urgent
/// Job may take), else 0. Every other pool: `free` = `slots` minus the Job tenants minus the held command slots (contract §1.4),
/// `urgent_free` = 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    /// Slots any Job may take.
    pub free: u32,
    /// Slots only an urgent Job may take, after the `free` ones.
    pub urgent_free: u32,
}

/// A Ready Job, as E3 lists it. The caller passes the Jobs in ANY order: [`schedule`] walks them urgent first (by `seq`), then
/// the skipped ones (by `skipped`, ascending), then the rest (by `seq`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub id: u32,
    /// `jobs.urgent`.
    pub urgent: bool,
    /// `jobs.skipped_n`: the Job's skip order, `Some` once E3 has skipped it, `None` before.
    pub skipped: Option<u32>,
    /// `jobs.seq`: the queue order, unique.
    pub seq: u32,
    /// Whether the Job is already a tenant (a retry): it competes for no pool and starts whenever it is Ready.
    pub tenant: bool,
    /// Indices into the pools slice: the resources the Job needs, `workers` included. Never empty, no repeats.
    pub needs: Vec<usize>,
}

/// What E3 does this tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schedule {
    /// The ids of the Jobs to start, in the order of the walk.
    pub start: Vec<u32>,
    /// The ids of the Jobs skipped this tick, in the order of the walk: a Job that is no tenant and that missed a pool.
    /// Nothing is reserved for them.
    pub skipped: Vec<u32>,
}

/// Walks `ready` in the order urgent > skipped > queue. A Job that is a tenant starts. Any other Job starts when every pool it
/// needs has a slot it may take (a non-urgent Job takes `free`, an urgent one `free` then `urgent_free`), taking one slot of
/// each, all or none; else it is skipped, takes nothing and reserves nothing, and the walk goes on to the next Job.
pub fn schedule(pools: &[Pool], ready: &[Ready]) -> Schedule {
    let mut order: Vec<&Ready> = ready.iter().collect();
    // Urgent first (by `seq`), then the skipped ones (by skip order), then the rest (by `seq`).
    order.sort_by_key(|j| match (j.urgent, j.skipped) {
        (true, _) => (0, 0, j.seq),
        (false, Some(n)) => (1, n, 0),
        (false, None) => (2, 0, j.seq),
    });
    let mut left: Vec<Pool> = pools.to_vec();
    let mut out = Schedule::default();
    for job in order {
        if job.tenant {
            out.start.push(job.id);
            continue;
        }
        let may_take = |p: usize, left: &[Pool]| left[p].free + if job.urgent { left[p].urgent_free } else { 0 };
        if job.needs.iter().all(|&p| may_take(p, &left) > 0) {
            for &p in &job.needs {
                if left[p].free > 0 {
                    left[p].free -= 1;
                } else {
                    left[p].urgent_free -= 1;
                }
            }
            out.start.push(job.id);
        } else {
            out.skipped.push(job.id);
        }
    }
    out
}
