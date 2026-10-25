//! `Ports`: the three seams a test fakes (docs/implementing.md, "Ports, Spawn and run_with").
//!
//! STUBS of PR ports2's red Job: every body is `todo!()`; Job ports-2/3 implements them.

use std::sync::Arc;

use crate::clock::Clock;
#[cfg(feature = "fakes")]
use crate::clock::ManualClock;
#[cfg(feature = "fakes")]
use crate::host::runner::FakeRunner;
use crate::host::runner::Runner;
#[cfg(feature = "fakes")]
use crate::host::spawn::FakeSpawn;
use crate::host::spawn::Spawn;

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
        todo!("Ports::real (Job ports-3)")
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
        todo!("Fakes::new (Job ports-3)")
    }
}
