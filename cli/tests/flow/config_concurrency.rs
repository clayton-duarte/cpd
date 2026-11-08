//! `cpd config concurrency set <n>` (commands.md §14.2c; contract §8.1, §8.2): the CLI end to end, in this process.

use std::fs;

use crate::common;
use common::fx::Fx;

const MINIMAL: &str = "version = 1\nmachine = \"hs\"\n";

fn text(fx: &Fx) -> String {
    fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap()
}

#[test]
fn set_writes_the_value_and_refuses_out_of_range() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);

    let run = fx.cpd(&["config", "concurrency", "set", "7"]);
    assert_eq!(run.exit, 0, "{run:?}");
    assert!(text(&fx).contains("concurrency = 7"), "{}", text(&fx));

    let out = fx.cpd(&["config"]).ok();
    assert_eq!(out["config"]["concurrency"], 7, "{out}");
    assert_eq!(out["sources"]["concurrency"], "file", "{out}");

    for n in ["0", "17"] {
        let before = text(&fx);
        let run = fx.cpd(&["config", "concurrency", "set", n]);
        common::refused(&run, "config_invalid", 1);
        assert_eq!(text(&fx), before, "set {n} changes nothing");
    }
}

#[test]
fn a_hand_edited_config_above_16_fails_to_load() {
    let fx = Fx::new();
    let hand_edited = format!("{MINIMAL}concurrency = 17\n");
    fx.env.write_config(&hand_edited);

    let run = fx.cpd(&["config"]);
    common::refused(&run, "config_invalid", 1);

    assert_eq!(text(&fx), hand_edited, "a refused load writes nothing");
}
