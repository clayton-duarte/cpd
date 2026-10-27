//! 'Ready to merge' in the Deploy column (pro-d118; commands.md §13.2): the merge icon and the menu's Merge entry exist only
//! for a PR whose base is the repo's default branch. Rows come from `pull_requests` through the real read model
//! (`read::open_prs`) and are drawn by [`column::draw`].

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use time::OffsetDateTime;

use super::column::{self, Drawn, Machine};
use super::host::Usage;
use crate::menu::Entry;
use crate::read::{self, stacked_ready_tests::store_with};

const MERGE: &str = "\u{F062D}";

/// The column drawn from the stored rows, and whether any cell of the PR rows shows the merge icon.
fn column_of(base_ref: &str, default_branch: &str) -> (Drawn, bool) {
    let (_dir, mut store) = store_with(base_ref, default_branch);
    let (open, _) = read::open_prs(&mut store).unwrap();
    assert_eq!(open.len(), 1);
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
    let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
    let mut drawn = None;
    terminal
        .draw(|f| drawn = Some(column::draw(f, f.area(), &open, &machine, 0, 0, &[], (false, 0), OffsetDateTime::UNIX_EPOCH, &Default::default())))
        .unwrap();
    let buf = terminal.backend().buffer();
    let icon = buf.content().iter().any(|c| c.symbol() == MERGE);
    (drawn.unwrap(), icon)
}

fn menu(drawn: &Drawn) -> Vec<&'static str> {
    let [row] = &drawn.rows[..] else { panic!("{:?}", drawn.rows) };
    row.items().into_iter().map(|i| i.label()).collect()
}

#[test]
fn stacked_ready_deploy_column_shows_no_merge_icon_for_a_stacked_pr() {
    let (drawn, icon) = column_of("'cpd/other'", "'main'");
    assert!(!icon, "no merge icon in the cells");
    assert!(drawn.icons.is_empty(), "no merge icon to click");
}

#[test]
fn stacked_ready_deploy_menu_has_no_merge_entry_for_a_stacked_pr() {
    let (drawn, _) = column_of("'cpd/other'", "'main'");
    assert_eq!(menu(&drawn), ["Copy Link", "Go to Lead"]);
}

/// The positive control: base = default branch shows the icon, and the menu has Merge too.
#[test]
fn stacked_ready_deploy_column_shows_the_merge_icon_for_a_clean_pr_on_the_default_branch() {
    let (drawn, icon) = column_of("'main'", "'main'");
    assert!(icon, "the merge icon is in the cells");
    assert_eq!(drawn.icons.len(), 1);
    assert_eq!(menu(&drawn), ["Merge", "Copy Link", "Go to Lead"]);
}
