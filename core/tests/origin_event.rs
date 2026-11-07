//! The optional `origin` of a `session-message` Event (contract §5.2, Plan ui-origin): the surface a message was
//! submitted from, exactly `tui` or `webui`, absent when the sender names none.

use cpd_core::events::{Kind, NewEvent};
use serde_json::json;

const TEXT: &str = "Message from the User: hi";

#[test]
fn session_message_carries_origin() {
    let event = NewEvent::session_message(TEXT, None::<&str>, "hi", true, Some("webui")).unwrap();
    assert_eq!(event.data()["origin"], json!("webui"), "{:?}", event.data());
    let event = NewEvent::session_message(TEXT, None::<&str>, "hi", true, Some("tui")).unwrap();
    assert_eq!(event.data()["origin"], json!("tui"), "{:?}", event.data());
    let event =
        NewEvent::session_message_unblocking(TEXT, None::<&str>, "hi", "u", Some("webui")).unwrap();
    assert_eq!(event.data()["origin"], json!("webui"), "{:?}", event.data());
}

#[test]
fn session_message_allows_absent_origin() {
    let event = NewEvent::session_message(TEXT, None::<&str>, "hi", true, None).unwrap();
    assert!(
        !event.data().as_object().unwrap().contains_key("origin"),
        "an absent origin is no key: {:?}",
        event.data()
    );
    let event = NewEvent::session_message_unblocking(TEXT, None::<&str>, "hi", "u", None).unwrap();
    assert!(
        !event.data().as_object().unwrap().contains_key("origin"),
        "an absent origin is no key: {:?}",
        event.data()
    );
    // The kind lists `origin` as optional, so an Event without it is valid.
    let spec = Kind::SessionMessage.spec();
    assert!(spec.optional.contains(&"origin"), "{spec:?}");
    assert!(!spec.required.contains(&"origin"), "{spec:?}");
}

#[test]
fn session_message_kind_refuses_unknown_origin() {
    for bad in ["bogus", "", "TUI", "web"] {
        assert!(
            NewEvent::session_message(TEXT, None::<&str>, "hi", true, Some(bad)).is_err(),
            "origin {bad:?} is not tui|webui"
        );
        assert!(
            NewEvent::session_message_unblocking(TEXT, None::<&str>, "hi", "u", Some(bad)).is_err(),
            "origin {bad:?} is not tui|webui"
        );
    }
}
