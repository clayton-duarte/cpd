//! `plan cancel` (commands.md §4.12, E1): the command tree in this process (`Fx`) against a temporary `$CPD_HOME`.
//! The 11 tests that cancel a Plan with a Lead pane, a fake `gh` and the engine wait for the Ports seams
//! (docs/research/tests-mock-mig-flow-b.md, section 4, `mrfb-4`) and are not here.

use crate::common;

use common::fx::Fx;
use common::refused;

// ---------- the command ----------

// ---------- a `tdd` PR's last test and impl Jobs do not stop a plan cancel (hs-d573, rule B2) ----------

// ---------- refusals ----------

#[test]
fn an_unknown_plan_is_plan_not_found() {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n");
    refused(&fx.cpd(&["plan", "cancel", "--plan", "nope"]), "plan_not_found", 4);
}

