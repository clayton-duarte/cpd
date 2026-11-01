use super::T;
use crate::fakes::tmux::KeyLog;

fn two(t: &T) {
    t.server();
    t.ok(&["split-window", "-d", "-t", "%0"]);
}

fn keys(pane: &str, keys: &[&str], literal: bool) -> KeyLog {
    KeyLog {
        pane: pane.into(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
        literal,
    }
}

#[test]
fn send_keys_appends_to_the_key_log() {
    let t = T::new();
    t.server();
    t.ok(&["send-keys", "-t", "%0", "echo hi", "Enter"]);
    t.ok(&["send-keys", "-t", "%0", "C-c"]);
    assert_eq!(
        t.state().unwrap().keys(),
        [
            keys("%0", &["echo hi", "Enter"], false),
            keys("%0", &["C-c"], false)
        ]
    );
}

#[test]
fn send_keys_l_logs_the_text_as_literal() {
    let t = T::new();
    t.server();
    t.ok(&["send-keys", "-l", "-t", "%0", "Enter"]);
    assert_eq!(t.state().unwrap().keys(), [keys("%0", &["Enter"], true)]);
}

#[test]
fn send_keys_m_is_accepted() {
    let t = T::new();
    t.server();
    assert_eq!(t.t(&["send-keys", "-M", "-t", "%0"]).code, 0);
}

#[test]
fn send_keys_to_an_unknown_pane_exits_1() {
    let t = T::new();
    t.server();
    t.fail(&["send-keys", "-t", "%9", "x"], 1, "can't find pane: %9");
}

#[test]
fn capture_pane_returns_the_seeded_screen() {
    let t = T::new();
    t.server();
    t.ok(&["__ctl", "screen", "%0", "hello"]);
    assert_eq!(t.ok(&["capture-pane", "-p", "-t", "%0"]), "hello\n");
}

#[test]
fn capture_pane_is_empty_when_nothing_is_seeded() {
    let t = T::new();
    t.server();
    assert_eq!(t.ok(&["capture-pane", "-p", "-t", "%0"]), "");
}

#[test]
fn capture_pane_accepts_s_and_e() {
    let t = T::new();
    t.server();
    t.ok(&["__ctl", "screen", "%0", "x"]);
    assert_eq!(
        t.ok(&["capture-pane", "-p", "-S", "-10", "-E", "5", "-t", "%0"]),
        "x\n"
    );
}

#[test]
fn screens_are_per_pane() {
    let t = T::new();
    two(&t);
    t.ok(&["__ctl", "screen", "%1", "other"]);
    assert_eq!(t.ok(&["capture-pane", "-p", "-t", "%0"]), "");
    assert_eq!(t.ok(&["capture-pane", "-p", "-t", "%1"]), "other\n");
}

#[test]
fn display_popup_and_display_menu_record_their_argv_and_return_0() {
    let t = T::new();
    t.server();
    assert_eq!(
        t.t(&["display-popup", "-E", "-w", "80", "cpd-picker"]).code,
        0
    );
    assert_eq!(
        t.t(&["display-menu", "-T", "title", "a", "a", "run-shell true"])
            .code,
        0
    );
    let st = t.state().unwrap();
    assert_eq!(
        st.popups(),
        [["display-popup", "-E", "-w", "80", "cpd-picker"]
            .map(String::from)
            .to_vec()]
    );
    assert_eq!(st.menus().len(), 1);
    assert_eq!(st.menus()[0][0], "display-menu");
}

#[test]
fn attach_records_a_client_and_exits_0() {
    let t = T::new();
    t.server();
    assert_eq!(t.t(&["attach", "-t", "=s"]).code, 0);
    assert_eq!(t.state().unwrap().clients().len(), 1);
}

#[test]
fn list_clients_prints_the_format() {
    let t = T::new();
    t.server();
    t.ok(&["attach", "-t", "=s"]);
    assert_eq!(
        t.ok(&[
            "list-clients",
            "-F",
            "#{client_name} #{client_width} #{client_height}"
        ]),
        "/dev/pts/0 80 24\n"
    );
}

#[test]
fn bind_then_list_keys_shows_the_binding() {
    let t = T::new();
    t.server();
    t.ok(&["bind", "-n", "F5", "set -g @x y"]);
    let out = t.ok(&["list-keys"]);
    assert!(out.contains("F5") && out.contains("set -g @x y"), "{out}");
}

#[test]
fn ctl_click_status_runs_the_bound_command_with_the_range() {
    let t = T::new();
    t.server();
    t.ok(&[
        "bind",
        "-n",
        "MouseDown1Status",
        "set -g @clicked #{mouse_status_range}",
    ]);
    t.ok(&["__ctl", "click", "status", "tab-3"]);
    assert_eq!(t.ok(&["show", "-gv", "@clicked"]), "tab-3\n");
}

#[test]
fn ctl_click_status_without_a_binding_is_a_no_op() {
    let t = T::new();
    t.server();
    assert_eq!(t.t(&["__ctl", "click", "status", "tab-3"]).code, 0);
}

#[test]
fn set_buffer_then_show_buffer_round_trips() {
    let t = T::new();
    t.server();
    t.ok(&["set-buffer", "some text"]);
    assert_eq!(t.ok(&["show-buffer"]), "some text");
}

#[test]
fn resize_pane_sets_the_size() {
    let t = T::new();
    t.server();
    t.ok(&["resize-pane", "-t", "%0", "-x", "100", "-y", "30"]);
    assert_eq!(
        t.ok(&[
            "display-message",
            "-p",
            "-t",
            "%0",
            "#{pane_width}x#{pane_height}"
        ]),
        "100x30\n"
    );
}

#[test]
fn resize_pane_takes_a_percentage_of_the_window() {
    let t = T::new();
    t.server();
    let window = t.ok(&[
        "display-message",
        "-p",
        "-t",
        "%0",
        "#{window_width}x#{window_height}",
    ]);
    let (w, h) = window.trim().split_once('x').unwrap();
    let (w, h): (i64, i64) = (w.parse().unwrap(), h.parse().unwrap());
    t.ok(&["resize-pane", "-t", "%0", "-x", "20%", "-y", "30%"]);
    assert_eq!(
        t.ok(&[
            "display-message",
            "-p",
            "-t",
            "%0",
            "#{pane_width}x#{pane_height}"
        ]),
        format!("{}x{}\n", w * 20 / 100, h * 30 / 100)
    );
    assert_eq!(t.t(&["resize-pane", "-t", "%0", "-x", "x%"]).code, 1);
}

#[test]
fn resize_window_and_select_layout_are_accepted() {
    let t = T::new();
    two(&t);
    assert_eq!(
        t.t(&["resize-window", "-t", "@0", "-x", "120", "-y", "40"])
            .code,
        0
    );
    assert_eq!(t.t(&["select-layout", "-t", "@0", "tiled"]).code, 0);
}

#[test]
fn select_pane_sets_the_active_pane() {
    let t = T::new();
    two(&t);
    t.ok(&["select-pane", "-t", "%1"]);
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "@0", "#{pane_id}"]),
        "%1\n"
    );
    t.ok(&["send-keys", "x"]);
    assert_eq!(t.state().unwrap().keys(), [keys("%1", &["x"], false)]);
}

#[test]
fn select_pane_accepts_p_and_t_styles() {
    let t = T::new();
    two(&t);
    t.ok(&["select-pane", "-P", "bg=red", "-t", "%1"]);
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "@0", "#{pane_id}"]),
        "%1\n"
    );
    t.ok(&["select-pane", "-T", "title", "-t", "%0"]);
    t.ok(&["send-keys", "x"]);
    assert_eq!(t.state().unwrap().keys(), [keys("%0", &["x"], false)]);
}

#[test]
fn ctl_is_rejected_unless_it_is_the_first_argument() {
    let t = T::new();
    t.server();
    let o = t.t(&["list-sessions", ";", "__ctl", "screen", "%0", "x"]);
    assert_eq!(
        (o.code, o.stderr.trim_end()),
        (1, "__ctl must be the first argument")
    );
    assert_eq!(t.ok(&["capture-pane", "-p", "-t", "%0"]), "");
}
