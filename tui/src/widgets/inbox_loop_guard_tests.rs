//! A click on a `loop-guard` Inbox item opens the Session that tripped the guard (live: open; archived: history; already in the chat slot: nothing).

use super::inbox::{Jump, click, item, jump};
use crate::tree::actions::Action;
use crate::tree::fixture::{archived, sample};
use cpd_core::ids::SessionId;
use cpd_core::inbox::model::{Item, Kind, Urgency};

fn sid(id: &str) -> SessionId {
    SessionId::parse(id).unwrap()
}

fn guard(session: &str) -> Item {
    let mut it = item(
        1,
        Kind::LoopGuard,
        Urgency::Red,
        "loop guard, 21 turns in 10 min",
        "2026-01-02T23:55:00.000Z",
    );
    it.session = Some(sid(session));
    it
}

#[test]
fn a_loop_guard_item_click_opens_the_live_session() {
    let tree = sample();
    let it = guard("l-sketch");
    let open = Action::Open(sid("l-sketch"));
    assert_eq!(jump(&tree, &it), Some(Jump::Run(open.clone())));
    assert_eq!(click(&tree, &it), Some(Jump::Run(open.clone())));
    assert_eq!(
        open.argv(),
        ["session", "open", "l-sketch"].map(String::from).to_vec()
    );
}

#[test]
fn a_loop_guard_item_click_opens_history_for_an_archived_session() {
    let tree = archived();
    let it = guard("f-blocked");
    let history = Action::History(sid("f-blocked"));
    assert_eq!(jump(&tree, &it), Some(Jump::Run(history.clone())));
    assert_eq!(click(&tree, &it), Some(Jump::Run(history)));
}

#[test]
fn a_loop_guard_item_click_is_a_no_op_when_its_session_is_already_in_the_chat_slot() {
    let tree = sample();
    let it = guard("l-ship");
    assert_eq!(
        jump(&tree, &it),
        Some(Jump::Run(Action::Open(sid("l-ship"))))
    );
    assert_eq!(click(&tree, &it), None);
}
