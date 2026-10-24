//! A Lead's and a Manager's pi get the configured plannotator port range as `PLANNOTATOR_PORT` (contract §6.5, §8.2).

use std::path::Path;

use cpd_core::config::Config;
use cpd_core::home::CpdHome;
use cpd_core::host::pi::{lead, LeadLaunch, LeadStart, Launch, Plannotator};
use cpd_core::ids::{PlanId, SessionId};

const ID: &str = "596d007f-1111-4222-8333-444455556666";

fn launch(range: &str, plan: Option<&PlanId>) -> Launch {
    let text = format!("version = 1\nmachine = \"m1\"\nplannotator_ports = \"{range}\"\n");
    let config = Config::parse(&text, Path::new("config.toml")).unwrap().0;
    let plannotator = Plannotator::from_config(&config, "192.168.3.3".into());
    let home = CpdHome::resolve(Some("/cpd-home".into()), None).unwrap();
    let session = SessionId::parse(ID).unwrap();
    lead(&LeadLaunch {
        home: &home,
        session: &session,
        plan,
        extensions: &[],
        plannotator: &plannotator,
        start: LeadStart::Resume { model: "p/m:high".into(), file: "/s/x.jsonl".into() },
    })
}

#[test]
fn a_leads_pi_gets_the_configured_port_range_as_plannotator_port() {
    let plan = PlanId::parse("cpd2").unwrap();
    let launch = launch("19432-19456", Some(&plan));
    assert_eq!(launch.env.get("PLANNOTATOR_PORT").map(String::as_str), Some("19432-19456"));
    assert_eq!(launch.env.get("PLANNOTATOR_REMOTE").map(String::as_str), Some("1"));
    assert_eq!(launch.env.get("PLANNOTATOR_URL_HOST").map(String::as_str), Some("192.168.3.3"));
}

#[test]
fn a_managers_pi_gets_the_configured_port_range_as_plannotator_port() {
    let launch = launch("20000-20003", None);
    assert_eq!(launch.env.get("PLANNOTATOR_PORT").map(String::as_str), Some("20000-20003"));
    assert_eq!(launch.env.get("PLANNOTATOR_REMOTE").map(String::as_str), Some("1"));
    assert_eq!(launch.env.get("PLANNOTATOR_URL_HOST").map(String::as_str), Some("192.168.3.3"));
}
