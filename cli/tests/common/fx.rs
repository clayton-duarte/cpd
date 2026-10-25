//! `Fx`: a test env whose `cpd` commands run in the test process (skeleton: every method is a stub).

use std::path::Path;

use super::{Run, TestEnv};

pub struct Fx {
    pub env: TestEnv,
}

impl Fx {
    pub fn new() -> Fx {
        todo!("Fx::new")
    }

    pub fn with(_env: TestEnv) -> Fx {
        todo!("Fx::with")
    }

    pub fn try_new() -> Option<Fx> {
        todo!("Fx::try_new")
    }

    pub fn cpd(&self, _args: &[&str]) -> Run {
        todo!("Fx::cpd")
    }

    pub fn cpd_in(&self, _cwd: &Path, _args: &[&str]) -> Run {
        todo!("Fx::cpd_in")
    }

    pub fn cpd_with(&self, _cwd: &Path, _args: &[&str], _vars: &[(&str, &str)]) -> Run {
        todo!("Fx::cpd_with")
    }
}
