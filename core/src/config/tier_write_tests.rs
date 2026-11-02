//! `set_tiers_in_text` (commands.md §14.2a "Effects"): the line of a changed key is replaced whole, an absent key is
//! inserted after the last top-level key, a key already at its value and every other byte stay.

use super::tier_write::set_tiers_in_text;

const DATE: &str = "2026-01-01";

#[test]
fn tiers_rewrite_replaces_only_changed_lines_whole_with_the_comment() {
    let before = "version = 1\nmachine = \"hs\"\nlead_tier = \"O2\"  # my own note\nworker_tier = \"H3\" # keep\n";
    let after = "version = 1\nmachine = \"hs\"\nlead_tier = \"S3\" # set by the Agent Tiers dialog, 2026-01-01\nworker_tier = \"H3\" # keep\n";
    assert_eq!(
        set_tiers_in_text(before, &[("lead_tier", "S3"), ("worker_tier", "H3")], DATE),
        after
    );
}

#[test]
fn tiers_rewrite_keeps_every_other_byte_and_inserts_an_absent_key_after_the_last_top_level_key() {
    let before =
        "# the machine\nversion = 1\nmachine = \"hs\"   \n\n# watch\n[watch]\ntick_seconds = 5\n";
    let after = "# the machine\nversion = 1\nmachine = \"hs\"   \ncontext_distill_tier = \"G1\" # set by the Agent Tiers dialog, 2026-01-01\nworker_tier = \"H2\" # set by the Agent Tiers dialog, 2026-01-01\n\n# watch\n[watch]\ntick_seconds = 5\n";
    assert_eq!(
        set_tiers_in_text(
            before,
            &[("context_distill_tier", "G1"), ("worker_tier", "H2")],
            DATE
        ),
        after
    );

    // With no table the lines go at the end.
    let before = "version = 1\nmachine = \"hs\"\n";
    let after = "version = 1\nmachine = \"hs\"\nmanager_tier = \"O3\" # set by the Agent Tiers dialog, 2026-01-01\n";
    assert_eq!(
        set_tiers_in_text(before, &[("manager_tier", "O3")], DATE),
        after
    );
}

#[test]
fn tiers_rewrite_of_equal_values_changes_nothing() {
    let before = "version = 1\nmachine = \"hs\"\nlead_tier = \"S3\" # set by hand, long ago\nmanager_tier = \"O3\"\n\n[watch]\ntick_seconds = 5\n";
    assert_eq!(
        set_tiers_in_text(before, &[("lead_tier", "S3"), ("manager_tier", "O3")], DATE),
        before
    );
    assert_eq!(set_tiers_in_text(before, &[], DATE), before);
}
