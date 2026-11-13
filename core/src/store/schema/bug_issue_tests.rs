//! The migration `bug-issue` (contract §4.5): `bug_records.issue` and `workflows.bug_id`, with `issue` filled for the open
//! bugs of the mapping (b6 -> .../issues/1156, b63 -> .../issues/1181); a closed bug (b1) keeps NULL.

use super::*;
use crate::error::CpdError;
use crate::store::Store;

#[test]
fn migrations_fill_issue_for_open_bugs() {
    let (_dir, home) = crate::store::tests::tmp_home();
    // The store as it is before the migration `*-bug-issue`: every migration but that one.
    let before: Vec<Named> = NAMED
        .iter()
        .copied()
        .filter(|m| !m.name.ends_with("-bug-issue"))
        .collect();
    let mut store = crate::store::tests::open_named(&home, &before).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO bug_records (id, title, symptom, status, reported_at) VALUES
                   ('b6', 'Six', 's', 'open', 't'), ('b63', 'Sixty-three', 's', 'open', 't');
                 INSERT INTO bug_records (id, title, symptom, root_fix, regression_test, status, reported_at, closed_at)
                   VALUES ('b1', 'One', 's', 'p/w', 't::x', 'closed', 't', 't2');",
            )?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    drop(store);

    let mut store = Store::open(&home, 5000).expect("upgrade a store holding bugs");
    let issues: Vec<(String, Option<String>)> = store
        .read(|c| {
            let mut q = c.prepare("SELECT id, issue FROM bug_records ORDER BY id")?;
            let rows = q
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .expect("bug_records has the column `issue`");
    assert_eq!(
        issues,
        vec![
            ("b1".to_owned(), None),
            (
                "b6".to_owned(),
                Some("https://github.com/clayton-duarte/cpd/issues/1156".to_owned())
            ),
            (
                "b63".to_owned(),
                Some("https://github.com/clayton-duarte/cpd/issues/1181".to_owned())
            ),
        ]
    );
}
