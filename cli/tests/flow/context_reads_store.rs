//! `context_reads` and `context delete` (decision pro-d10 item 5): deleting a Context removes its read rows.

use crate::common;

use common::fx::Fx;

#[test]
fn context_delete_removes_the_reads_of_the_context() {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "rd", "--title", "Reads", "--owner", "acme",
    ])
    .ok();
    fx.env.sql(
        "INSERT INTO context_records (context, id, kind, at, machine, text)
           VALUES ('rd', 'm1-n1', 'note', 't', 'm1', 'x');
         INSERT INTO context_reads (context, record, session, channel, read_at)
           VALUES ('rd', 'm1-n1', 's1', 'pushed', 't');",
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM context_reads WHERE context = 'rd'"),
        1
    );
    fx.cpd(&["context", "delete", "rd", "--yes"]).ok();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM context_reads WHERE context = 'rd'"),
        0
    );
}
