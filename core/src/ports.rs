//! `Ports`: the three seams a test fakes (docs/implementing.md, "Ports, Spawn and run_with").

use std::sync::Arc;

use crate::clock::{Clock, SystemClock};
#[cfg(feature = "fakes")]
use crate::clock::ManualClock;
#[cfg(feature = "fakes")]
use crate::host::runner::FakeRunner;
use crate::host::runner::{Runner, SystemRunner};
#[cfg(feature = "fakes")]
use crate::host::spawn::FakeSpawn;
use crate::host::spawn::{ProcessSpawn, Spawn};

/// Time, short processes and long-lived or replacing processes.
#[derive(Clone)]
pub struct Ports {
    pub clock: Arc<dyn Clock>,
    pub runner: Arc<dyn Runner>,
    pub spawn: Arc<dyn Spawn>,
}

impl Ports {
    /// `SystemClock`, `SystemRunner` and `ProcessSpawn`.
    pub fn real() -> Ports {
        Ports { clock: Arc::new(SystemClock), runner: Arc::new(SystemRunner), spawn: Arc::new(ProcessSpawn) }
    }
}

/// The fakes over one `Ports`, typed so a test scripts them and reads their record.
#[cfg(feature = "fakes")]
pub struct Fakes {
    pub ports: Ports,
    pub clock: Arc<ManualClock>,
    pub runner: Arc<FakeRunner>,
    pub spawn: Arc<FakeSpawn>,
}

#[cfg(feature = "fakes")]
impl Fakes {
    /// A `ManualClock`, a `FakeRunner` on that clock and a `FakeSpawn`, and the `Ports` over them.
    pub fn new() -> Fakes {
        let clock = Arc::new(ManualClock::new());
        let runner = Arc::new(FakeRunner::new((*clock).clone()));
        let spawn = Arc::new(FakeSpawn::new());
        let ports = Ports { clock: clock.clone(), runner: runner.clone(), spawn: spawn.clone() };
        Fakes { ports, clock, runner, spawn }
    }
}
