//! A `Clock` whose naps run steps of the test at virtual times: the way a test releases what a wait of the store
//! waits for (a setup lock held by the test, a connection that finishes) at a time of the wait itself, in one thread,
//! with no sleep and no polling.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use cpd_core::clock::{Clock, ManualClock};
use time::OffsetDateTime;

type Step<'a> = (Duration, Box<dyn FnOnce() + Send + 'a>);

/// A [`ManualClock`] that runs each step once, in the first `sleep` that ends at or after the step's time.
pub struct ScriptedClock<'a> {
    clock: ManualClock,
    steps: Mutex<Vec<Step<'a>>>,
}

impl<'a> ScriptedClock<'a> {
    pub fn new() -> Self {
        ScriptedClock {
            clock: ManualClock::new(),
            steps: Mutex::new(Vec::new()),
        }
    }

    /// Runs `step` when virtual time has reached `at`, in the nap that reaches it (steps of one nap run in the order of
    /// their times).
    pub fn at(self, at: Duration, step: impl FnOnce() + Send + 'a) -> Self {
        let mut steps = self.steps.lock().unwrap();
        steps.push((at, Box::new(step)));
        steps.sort_by_key(|(at, _)| *at);
        drop(steps);
        self
    }

    /// The virtual time passed.
    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }
}

impl Clock for ScriptedClock<'_> {
    fn now(&self) -> Instant {
        self.clock.now()
    }

    fn sleep(&self, d: Duration) {
        self.clock.sleep(d);
        let now = self.clock.elapsed();
        let due = {
            let mut steps = self.steps.lock().unwrap();
            let split = steps.partition_point(|(at, _)| *at <= now);
            let later = steps.split_off(split);
            std::mem::replace(&mut *steps, later)
        };
        // Outside the lock: a step may read the clock.
        for (_, step) in due {
            step();
        }
    }

    fn utc(&self) -> OffsetDateTime {
        self.clock.utc()
    }
}

#[test]
fn a_scripted_clock_runs_each_step_once_in_the_first_nap_that_reaches_its_time() {
    let ran = Mutex::new(Vec::new());
    let clock = ScriptedClock::new()
        .at(Duration::from_millis(50), || {
            ran.lock().unwrap().push("late")
        })
        .at(Duration::from_millis(20), || {
            ran.lock().unwrap().push("early")
        });
    clock.sleep(Duration::from_millis(10));
    assert!(ran.lock().unwrap().is_empty(), "nothing is due at 10 ms");
    clock.sleep(Duration::from_millis(20));
    assert_eq!(
        *ran.lock().unwrap(),
        ["early"],
        "the step of 20 ms runs in the nap that ends at 30 ms"
    );
    clock.sleep(Duration::from_millis(30));
    clock.sleep(Duration::from_millis(30));
    assert_eq!(
        *ran.lock().unwrap(),
        ["early", "late"],
        "each step runs once"
    );
    assert_eq!(clock.elapsed(), Duration::from_millis(90));
}
