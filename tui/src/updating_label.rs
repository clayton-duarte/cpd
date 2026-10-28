//! Red tests (hs-d532): a PR whose pull request conflicts reads `updating`, not `conflict`, while it has a not-ended update
//! Job: in the running colour while that Job runs, in the waiting colour while it is queued. In the Plan view's PR box
//! title, the PRs column and the legend. (`Meaning::InProgress` is the running colour of `palette`.)

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, PrKey, SessionId};
use cpd_core::store::Store;
use ratatui::text::{Line, Span};

use crate::palette::{self, Meaning};
use crate::plan::boxes;
use crate::read::{OpenPr, PrNode, PrState, PrStatus, Pull, ReviewDecision, UpdateState};
use crate::widgets::{column, legend};

const RUNNING: Meaning = Meaning::InProgress;

fn node(update: Option<UpdateState>) -> PrNode {
    PrNode {
        key: PrKey::parse("api").unwrap(),
        repo_name: "api-repo".into(),
        base: "main".into(),
        status: PrStatus::Open,
        needs: Vec::new(),
        pull: Some(Pull {
            number: 12,
            state: PrState::Open,
            url: "https://github.com/acme/repo/pull/12".into(),
            conflicting: true,
            clean: false,
            lead_working: false,
            update,
            draft: false,
        }),
        waiting_on: Vec::new(),
        superseded_by: None,
    }
}

fn open_pr(update: Option<UpdateState>, lead: Option<&str>) -> OpenPr {
    OpenPr {
        repo: "acme/repo".to_owned(),
        number: 7,
        url: "https://github.com/acme/repo/pull/7".to_owned(),
        title: "A title".to_owned(),
        draft: false,
        checks: crate::read::Checks::Passed,
        review_decision: ReviewDecision::None,
        conflicting: true,
        update,
        clean: false,
        active: false,
        lead: lead.map(|l| SessionId::parse(l).unwrap()),
        jira: None,
        lead_working: false,
        plan_failed: false,
        plan_phase: None,
        merging: false,
    }
}

/// The span of `line` whose text is `word`, if any.
fn span_of<'a>(line: &'a Line<'static>, word: &str) -> Option<&'a Span<'static>> {
    line.spans.iter().find(|s| s.content.trim() == word)
}

fn box_word(update: Option<UpdateState>, word: &str) -> (String, Option<ratatui::style::Style>) {
    let title = boxes::title(&node(update), false, 0);
    (title.to_string(), span_of(&title, word).map(|s| s.style))
}

#[test]
fn a_box_with_a_running_update_job_reads_updating_in_the_running_colour() {
    let (title, style) = box_word(Some(UpdateState::Running), "updating");
    assert!(title.contains("#12 updating"), "{title:?}");
    assert!(!title.contains("conflict"), "{title:?}");
    assert_eq!(style.expect("an `updating` span"), palette::fg_clickable(RUNNING));
}

#[test]
fn a_box_with_a_queued_update_job_reads_updating_in_the_waiting_colour() {
    let (title, style) = box_word(Some(UpdateState::Queued), "updating");
    assert!(title.contains("#12 updating"), "{title:?}");
    assert!(!title.contains("conflict"), "{title:?}");
    assert_eq!(style.expect("an `updating` span"), palette::fg_clickable(Meaning::Waiting));
}

#[test]
fn a_box_with_no_update_job_still_reads_conflict() {
    let (title, style) = box_word(None, "conflict");
    assert!(title.contains("#12 conflict") && !title.contains("updating"), "{title:?}");
    assert_eq!(style.expect("a `conflict` span"), palette::fg_clickable(Meaning::Waiting));
    // and only the absence of the update Job: with one, the same box no longer reads `conflict`
    let (with, _) = box_word(Some(UpdateState::Queued), "updating");
    assert!(with.contains("#12 updating") && !with.contains("conflict"), "{with:?}");
}

#[test]
fn a_column_pr_with_an_update_job_reads_updating() {
    for (update, meaning) in [(UpdateState::Running, RUNNING), (UpdateState::Queued, Meaning::Waiting)] {
        let [_, second] = column::pr_lines(&open_pr(Some(update), Some("l-a")), 60, 0);
        let text = second.to_string();
        assert!(text.contains(" updating") && !text.contains("conflict"), "{update:?}: {text:?}");
        assert_eq!(span_of(&second, "updating").expect("an `updating` span").style, palette::fg_clickable(meaning), "{update:?}");
    }
    // no update Job: `conflict` as before; a pull request no Plan owns has none and keeps it red
    let [_, owned] = column::pr_lines(&open_pr(None, Some("l-a")), 60, 0);
    assert_eq!(span_of(&owned, "conflict").unwrap().style, palette::fg_clickable(Meaning::Waiting));
    let [_, unowned] = column::pr_lines(&open_pr(None, None), 60, 0);
    assert!(!unowned.to_string().contains("updating"));
}

#[test]
fn the_legend_lists_updating_in_both_colours() {
    let sections = legend::sections();
    let prs = sections.iter().find(|s| s.title == "PRs (Dispatch)").expect("the PRs section");
    let cell = |row: &Vec<Line<'static>>| row.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(" | ");
    let row = |word: &str, says: &str| {
        let r = prs
            .rows
            .iter()
            .find(|r| r[0].to_string() == word && r.get(1).is_some_and(|l| l.to_string() == says))
            .unwrap_or_else(|| panic!("no `{word}` row saying {says:?}: {:?}", prs.rows.iter().map(cell).collect::<Vec<_>>()));
        r[0].spans[0].style
    };
    assert_eq!(row("#2 updating", "its update Job is fixing the conflict"), palette::fg_clickable(RUNNING));
    assert_eq!(row("#2 updating", "its update Job is queued"), palette::fg_clickable(Meaning::Waiting));
}

fn fixture(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn the_plan_read_reports_the_update_job_state() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    fixture(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
             VALUES ('lead', 'lead', 'ctx', NULL, NULL, 'n', '/s/lead.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                     '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
             VALUES ('p1', 'ctx', 'P', 'dispatch', 'lead', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             UPDATE sessions SET plan = 'p1' WHERE id = 'lead';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, created_at, changed_at)
             VALUES ('acme/repo', 12, 'https://github.com/acme/repo/pull/12', 'T', 0, 'open', 'none', 'passed', 'conflicting',
                     '2026-01-03T00:00:00.000Z', '2026-01-04T00:00:00.000Z');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request)
             VALUES ('p1', 'repo', '/repo', 'main', 'open', 'cpd/p1', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z',
                     12, 'https://github.com/acme/repo/pull/12', '2026-01-03T00:00:00.000Z',
                     (SELECT id FROM pull_requests WHERE number = 12));
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
             VALUES ('p1', 'repo', 'up-repo', 'Update', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, 'update', '{FP}', cpd_txn(), 't');"
        ),
    );
    let plan = PlanId::parse("p1").unwrap();
    let update = |store: &mut Store| {
        let boxes = crate::read::plan_boxes(store, &plan).unwrap();
        assert_eq!(boxes.context, ContextId::parse("ctx").unwrap());
        let pull = boxes.prs[0].pull.clone().expect("the PR has a pull request");
        assert!(pull.conflicting);
        pull.update
    };
    assert_eq!(update(&mut store), Some(UpdateState::Queued));
    fixture(&mut store, "UPDATE jobs SET status = 'running' WHERE key = 'up-repo';");
    assert_eq!(update(&mut store), Some(UpdateState::Running));
}
