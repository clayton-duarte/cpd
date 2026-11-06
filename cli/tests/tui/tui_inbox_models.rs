//! The `models-changed` Inbox item in the cockpit (commands.md §13.2 "Clicks on an Inbox item", §17 legend; contract §2.2
//! "The models-changed rule"; pro-d13, pro-d16): its row is cyan, a click on it opens the announced ids in the dialog kit's
//! read-only `text` view and runs no command, and the legend has a line for it. The view runs in this process on a script
//! (`common/inbox.rs`), with the fake tmux server and the fake `cpd` of the `FakeRunner`.

use crate::common;

use common::fx::Fx;
use common::inbox::{
    World, click, condition, frame_with, gone, open, rows, seed, show, show_with, view_fx,
};
use common::tree::{engine, stamp};
use cpd_core::host::runner::Reply;
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_tui::pump::Signal;
use cpd_tui::widgets::legend;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;

const CYAN: &str = "\u{F06F1}";
const WHITE: &str = "\u{F0366}";
const CLIENT: &str = "/dev/pts/9";
const HEIGHT: u16 = 30;
const AT: &str = "2026-10-25T14:00:00Z";
const SUBJECT: &str = "models:bifrost/2026-10-25T14:00:00Z";
const LABEL: &str = "bifrost: 2 new, 1 gone";

/// The notice: about the machine, so it has no Context, Plan, Job, Session or record.
fn notice() -> Condition {
    Condition {
        context: None,
        ..condition(Kind::ModelsChanged, Urgency::Cyan, SUBJECT, LABEL)
    }
}

/// The `swr_models` row of the provider, as E15 wrote it: `changes` at `at`.
fn cache(fx: &Fx, at: &str, appeared: &[&str], went_away: &[&str]) {
    let catalog = json!({
        "pi_version": "1.3.0",
        "source": {"kind": "openai-models", "auth": "custom", "base": null},
        "ids": ["anthropic/claude-sonnet-5"]
    });
    let changes = json!({"at": at, "appeared": appeared, "went_away": went_away});
    fx.env.sql(&format!(
        "INSERT OR REPLACE INTO swr_models (provider, catalog_json, changes_json, polled_at) VALUES ('bifrost', '{catalog}', '{changes}', '{at}');"
    ));
}

/// The store with the notice open (opened 330 s ago), the engine's lock held, a cockpit with a client, and the fake `cpd`.
fn rig() -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    reconcile(&mut open(&fx.env), &[notice()], &stamp(&fx, -330)).unwrap();
    fx
}

fn cockpit(fx: &Fx) {
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let view = ok(&[
        "new-session",
        "-d",
        "-s",
        "cpd",
        "-n",
        "main",
        "-x",
        "120",
        "-y",
        "40",
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view widgets",
    ]);
    ok(&["set", "-p", "-t", &view, "@cpd_view", "widgets"]);
    let chat = ok(&[
        "split-window",
        "-h",
        "-d",
        "-t",
        &view,
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view chat",
    ]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    fx.tmux_route.fake().add_client(CLIENT, 200, 60);
}

fn first_rows(fx: &Fx) -> Vec<String> {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the notice's row", |r| {
        r.iter().any(|row| row.contains(LABEL))
    });
    rows(&frames, at)
}

/// Clicks the notice's row and returns the popups' commands.
fn click_row(fx: &Fx, screen: &[String]) -> Vec<String> {
    let row = screen
        .iter()
        .position(|r| r.contains(LABEL))
        .expect("the row");
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let signals = vec![
        Signal::Commit,
        click(10, u16::try_from(row).unwrap()),
        gone(),
    ];
    show_with(fx, HEIGHT, signals, &World::new(), |argv| {
        panic!("a click on a notice runs no command of the tree: {argv:?}")
    });
    fx.tmux_probe()
        .state()
        .popups()
        .iter()
        .map(|p| p.last().expect("a popup has a command").clone())
        .collect()
}

#[test]
fn the_row_draws_the_cyan_glyph_and_its_label_and_a_click_opens_the_announced_ids_in_a_text_dialog_that_runs_no_command()
 {
    let fx = rig();
    let _engine = engine(&fx);
    cockpit(&fx);
    cache(
        &fx,
        AT,
        &["openai/gpt-5.6", "anthropic/claude-sonnet-5"],
        &["anthropic/claude-sonnet-4"],
    );
    let screen = first_rows(&fx);
    let row = screen.iter().find(|r| r.contains(LABEL)).unwrap();
    assert!(
        row.contains(CYAN),
        "the cyan tier glyph U+F06F1 on the row: {row:?}"
    );

    let commands = click_row(&fx, &screen);
    let [dialog] = &commands[..] else {
        panic!("one popup: {commands:?}")
    };
    assert!(
        dialog.starts_with("cpd tui view dialog text --title="),
        "a read-only `text` dialog: {dialog}"
    );
    assert!(
        dialog.contains("Models: bifrost"),
        "titled `Models: <provider>`: {dialog}"
    );
    let at = |text: &str| {
        dialog
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} in {dialog}"))
    };
    let (new, gone) = (at("New (2)"), at("Gone (1)"));
    assert!(
        new < at("openai/gpt-5.6") && new < at("anthropic/claude-sonnet-5"),
        "the new ids under `New (2)`"
    );
    assert!(
        at("anthropic/claude-sonnet-5") < gone && at("openai/gpt-5.6") < gone,
        "new first"
    );
    assert!(
        gone < at("anthropic/claude-sonnet-4"),
        "the gone id under `Gone (1)`"
    );
    assert!(
        fx.calls("tmux").iter().all(|c| !c
            .args
            .iter()
            .any(|a| a == "set-buffer" || a == "select-pane")),
        "nothing is copied and no pane is focused"
    );
    assert_eq!(fx.calls("cpd"), vec![], "no command runs");
}

#[test]
fn the_dialog_leaves_out_a_part_with_no_ids_and_the_non_chat_ids_of_bifrost() {
    let fx = rig();
    let _engine = engine(&fx);
    cockpit(&fx);
    cache(
        &fx,
        AT,
        &[
            "openai/gpt-5.6",
            "openai/tts-1",
            "vertex/text-embedding-004",
        ],
        &[],
    );
    let screen = first_rows(&fx);
    let commands = click_row(&fx, &screen);
    let [dialog] = &commands[..] else {
        panic!("one popup: {commands:?}")
    };
    assert!(
        dialog.contains("New (1)") && dialog.contains("openai/gpt-5.6"),
        "{dialog}"
    );
    assert!(
        !dialog.contains("Gone"),
        "a part with no ids is omitted: {dialog}"
    );
    assert!(
        !dialog.contains("tts-1") && !dialog.contains("text-embedding"),
        "the announced part, not the cache's: {dialog}"
    );
}

#[test]
fn a_click_when_a_newer_change_replaced_the_item_says_it_changed_again() {
    let fx = rig();
    let _engine = engine(&fx);
    cockpit(&fx);
    cache(&fx, "2026-10-26T09:00:00Z", &["openai/gpt-5.7"], &[]);
    let screen = first_rows(&fx);
    let commands = click_row(&fx, &screen);
    let [dialog] = &commands[..] else {
        panic!("one popup: {commands:?}")
    };
    assert!(
        dialog.contains("changed again; the item closes at the next tick"),
        "{dialog}"
    );
    assert!(
        !dialog.contains("gpt-5.7"),
        "the newer change is not this item's: {dialog}"
    );
}

fn legend_rows() -> Vec<String> {
    let (w, h) = legend::size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| legend::draw(f, f.area())).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn the_legends_cyan_tier_line_names_a_change_in_the_models_a_provider_offers() {
    let lines = legend_rows();
    let notice = lines
        .iter()
        .find(|r| r.contains(WHITE) && r.contains("a cpd notice: curation, nothing blocks; also a change in the models a provider offers"))
        .unwrap_or_else(|| panic!("the cyan tier's line:\n{}", lines.join("\n")));
    assert!(
        notice.contains("white"),
        "the line names the tier: {notice:?}"
    );
}
