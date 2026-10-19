//! Plans (contract §2.3, §3.1; commands.md §4, §15.5 and §19 amendments 6
//! and 8), run as the built binary against a temporary `$CPD_HOME`, temporary
//! git repos and a private tmux server, with a fake `cpd` on `PATH` for the
//! panes (`session run`) and for `watch --ensure`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{CPD, Run, TestEnv, WATCH_OK, refused, wait_until, write_executable};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, ReviewId};
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// ---------- fixtures ----------

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store(env: &TestEnv) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

fn pid(id: &str) -> PlanId {
    PlanId::parse(id).unwrap()
}

/// Where the fake `cpd session run <id>` writes its environment.
fn pane_env_file(env: &TestEnv, session: &str) -> PathBuf {
    env.root().join(format!("pane-{session}.env"))
}

const WATCH_FAILS: &str =
    "echo '{\"ok\": false, \"error\": {\"code\": \"watch_start_failed\", \"message\": \"No lock.\", \"details\": {}}}'; exit 1";

/// The env with its config, a fake `cpd`, Context `ctx` and the repo `pay`.
fn setup(env: &TestEnv) -> PathBuf {
    env.write_tmux_config("");
    env.fake_cpd_panes(WATCH_OK);
    env.cpd(&["context", "new", "ctx", "--title", "Payments"]).ok();
    env.git_repo("pay")
}

fn cpd_with(env: &TestEnv, vars: &[(&str, &str)], args: &[&str]) -> Run {
    let mut cmd = env.command(CPD);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    let out = cmd.args(args).output().unwrap();
    Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

/// `plan new` from the test root, with no PR.
fn new_plan_only(env: &TestEnv, id: &str) -> Value {
    env.cpd(&["plan", "new", id, "--context", "ctx", "--title", &format!("Plan {id}")]).ok()
}

/// `plan new` and the Plan's one PR, keyed `pay` (R34 A12: a Plan has no repo of its own).
fn new_plan(env: &TestEnv, id: &str, repo: &Path) -> Value {
    let out = new_plan_only(env, id);
    env.cpd(&["pr", "add", "pay", "--repo", repo.to_str().unwrap(), "--plan", id]).ok();
    out
}

fn show(env: &TestEnv, plan: &str) -> Value {
    env.cpd(&["plan", "show", "--plan", plan]).ok()
}

/// The environment the fake `session run` of `session` was started with.
fn pane_env(env: &TestEnv, session: &str) -> String {
    let path = pane_env_file(env, session);
    wait_until("the pane's session run", || path.exists() && fs::read_to_string(&path).is_ok_and(|t| t.contains("CPD_SESSION")));
    fs::read_to_string(path).unwrap()
}

/// The panes of the private server: `(pane, window name, @cpd_session)`.
fn panes(env: &TestEnv) -> Vec<(String, String, String)> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{window_name}\t#{@cpd_session}"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0].to_owned(), f[1].to_owned(), f[2].to_owned())
        })
        .collect()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A draft Job `key` of `plan` needing `needs`, written as `job add` would in phase `plan`.
fn add_job(env: &TestEnv, plan: &str, key: &str, needs: &[&str]) {
    add_draft(env, plan, key, needs, false);
}

/// A draft Job `key` of `plan` needing `needs`, `appended` as `job add --draft` writes it after the lock.
fn add_draft(env: &TestEnv, plan: &str, key: &str, needs: &[&str], appended: bool) {
    store(env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, created_txn, created_at)
                     VALUES ('{plan}', (SELECT key FROM prs WHERE plan = '{plan}'), '{key}', 'Job {key}', 'S2', 'Goal of {key}.', 'Context of {key}.', '[\"src/{key}.rs\"]',
                             '[\"cargo test\"]', '[]', 'draft', {appended}, {id}, 't');",
                    appended = i32::from(appended),
                    id = tx.id()
                ),
            )?;
            for need in needs {
                exec(tx, &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{key}', '{need}');"))?;
            }
            Ok(())
        })
        .unwrap();
}

fn edit_goal(env: &TestEnv, plan: &str, key: &str, goal: &str) {
    store(env)
        .write("fixture", |tx| exec(tx, &format!("UPDATE jobs SET goal = '{goal}' WHERE plan = '{plan}' AND key = '{key}';")))
        .unwrap();
}

/// A Plan `id` in phase `plan` with draft Jobs `a` and `b` (needs `a`).
fn planned(env: &TestEnv, repo: &Path, id: &str) {
    new_plan(env, id, repo);
    env.cpd(&["plan", "phase", "plan", "--plan", id]).ok();
    add_job(env, id, "a", &[]);
    add_job(env, id, "b", &["a"]);
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

// ---------- plan new ----------

#[test]
fn new_registers_the_plan_and_its_lead_and_starts_it_in_the_callers_cwd() {
    let env = TestEnv::new();
    let repo = setup(&env);
    env.cpd(&["context", "decide", "One engine per machine.", "--context", "ctx"]).ok();
    let brief = env.root().join("brief.md");
    fs::write(&brief, "Retry the failed payouts.\n").unwrap();
    // Not the repo, not the root: the directory the User runs `plan new` from.
    let home_dir = env.root().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    let cwd = home_dir.canonicalize().unwrap().display().to_string();

    let mut cmd = env.command(CPD);
    let args = ["plan", "new", "pay", "--context", "ctx", "--title", "Retry payouts", "--jira", "PAY-1", "--brief-file", brief.to_str().unwrap()];
    let out = cmd.env("PLANNOTATOR_PORT", "19432-19439").env("PLANNOTATOR_REMOTE", "0").env("PLANNOTATOR_URL_HOST", "review.test").args(args).current_dir(&home_dir).output().unwrap();
    let out = Run { exit: out.status.code().unwrap(), stdout: String::from_utf8(out.stdout).unwrap(), stderr: String::new() }.ok();
    let session = out["plan"]["lead_session"].as_str().unwrap().to_owned();
    assert_eq!(
        out["plan"],
        json!({"id": "pay", "context": "ctx", "title": "Retry payouts", "jira": "PAY-1", "phase": "grounding", "lead_session": session, "prs": []})
    );
    assert_eq!((&out["lead"]["session"], &out["lead"]["name"], &out["lead"]["forked_from"]), (&json!(session), &json!("pay"), &Value::Null));
    assert_eq!(out["lead"]["cwd"], cwd);
    assert_eq!(out["record"], "m1-p1");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM prs"), 0, "a new Plan has no PR");
    let (pane, window) = (out["lead"]["pane"].as_str().unwrap(), out["lead"]["window"].as_str().unwrap());

    // The Lead: stopped, in its parked window, with its pane recorded, in the caller's cwd.
    let lead = env.cpd(&["session", "show", &session]).ok()["session"].clone();
    assert_eq!((&lead["role"], &lead["plan"], &lead["state"], &lead["tier"]), (&json!("lead"), &json!("pay"), &json!("stopped"), &json!("O3")));
    assert_eq!((&lead["cwd"], &lead["tmux_pane"], &lead["place"]), (&json!(cwd), &json!(pane), &json!("parked")));
    assert_eq!(panes(&env), vec![(pane.to_owned(), "park-pay".to_owned(), session.clone())], "the cockpit is created with the Lead's window");
    pane_env(&env, &session);
    let path = env.tmux(&["display-message", "-p", "-t", pane, "#{pane_current_path}"]);
    assert_eq!(String::from_utf8(path.stdout).unwrap().trim(), cwd, "the pane starts in the caller's cwd");

    // The pane runs `cpd session run <id>` with the Plan's and plannotator's variables (§15.5).
    let pane_vars = pane_env(&env, &session);
    let pane_vars: Vec<&str> = pane_vars.lines().filter(|l| l.starts_with("CPD_") || l.starts_with("PLANNOTATOR_")).collect();
    for var in [
        format!("CPD_HOME={}", env.cpd_home().display()),
        format!("CPD_SESSION={session}"),
        "CPD_PLAN=pay".to_owned(),
        "PLANNOTATOR_PORT=19432-19439".to_owned(),
        "PLANNOTATOR_REMOTE=1".to_owned(),
        "PLANNOTATOR_URL_HOST=review.test".to_owned(),
    ] {
        assert!(pane_vars.contains(&var.as_str()), "{var} not in {pane_vars:?}");
    }
    let run = env.calls("cpd").into_iter().find(|c| c[0] == "session").unwrap();
    assert_eq!(run, ["session", "run", session.as_str()]);

    // Events: plan-created and session-registered together, then the record, then the window.
    let created = &events(&env, "plan-created")[0];
    assert_eq!(created["data"], json!({"title": "Retry payouts", "jira": "PAY-1", "lead_session": session, "forked_from": null, "cwd": cwd}));
    let registered = events(&env, "session-registered");
    assert_eq!(registered.len(), 1);
    assert_eq!((&registered[0]["plan"], &registered[0]["session"]), (&json!("pay"), &json!(session)));
    assert_eq!(registered[0]["data"], json!({"role": "lead", "name": "pay", "forked_from": null, "tier": "O3"}));
    assert_eq!(registered[0]["id"].as_i64().unwrap(), created["id"].as_i64().unwrap() + 1);
    let moved = events(&env, "session-moved");
    assert_eq!(moved[0]["data"], json!({"pane": pane, "window": window, "place": "parked"}));

    // The `plan` record in the Context store: no `repo`; `context show` lists the Plan's PR repos.
    let shown = env.cpd(&["context", "show", "--context", "ctx"]).ok();
    let record = shown["records"].as_array().unwrap().iter().find(|r| r["kind"] == "plan").unwrap().clone();
    assert_eq!(record["text"], "Retry payouts");
    assert_eq!(record["plan"], json!({"id": "pay", "machine": "m1", "jira": "PAY-1"}));
    assert_eq!(record["repos"], json!([]), "no PR yet");
    assert!(!record.to_string().contains("\"repo\""), "{record}");
    assert!(!env.cpd_home().join("contexts").exists(), "the record is in the database only");
    let real = repo.canonicalize().unwrap().display().to_string();
    let other = env.git_repo("web");
    env.cpd(&["pr", "add", "api", "--repo", &real, "--plan", "pay"]).ok();
    env.cpd(&["pr", "add", "api2", "--repo", &real, "--plan", "pay"]).ok();
    env.cpd(&["pr", "add", "web", "--repo", other.to_str().unwrap(), "--plan", "pay"]).ok();
    env.cpd(&["pr", "add", "gone", "--repo", env.git_repo("old").to_str().unwrap(), "--plan", "pay"]).ok();
    env.cpd(&["pr", "cancel", "gone", "--plan", "pay"]).ok();
    let shown = env.cpd(&["context", "show", "--context", "ctx"]).ok();
    let record = shown["records"].as_array().unwrap().iter().find(|r| r["kind"] == "plan").unwrap().clone();
    let web = other.canonicalize().unwrap().display().to_string();
    assert_eq!(record["repos"], json!([real, web]), "each repo once, in PR key order, no cancelled PR");
    assert!(shown["markdown"].as_str().unwrap().contains(&format!("pay (on m1, repos {real}, {web}, PAY-1): Retry payouts")), "{shown}");

    // The first message: the header, the brief, then every record (a fresh Lead).
    let message = fs::read_to_string(home(&env).lead_message(&pid("pay"))).unwrap();
    let header = "You are the Lead of Plan pay \"Retry payouts\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\nRetry the failed payouts.\n\n# Payments\n";
    assert!(message.starts_with(header), "{message}");
    assert!(message.contains("One engine per machine."), "{message}");
    assert!(message.contains("m1-p1"), "{message}");
}

/// G11: `set -p … @cpd_session` fails after `new-window`: the window is killed, the error returned.
#[test]
fn new_leaves_no_window_when_a_step_after_new_window_fails() {
    let env = TestEnv::new();
    setup(&env);
    new_plan_only(&env, "first");
    let windows = || String::from_utf8(env.tmux(&["list-windows", "-a", "-F", "#{window_name}"]).stdout).unwrap();
    let before = windows();
    assert!(before.contains("park-first"), "{before}");

    let path = std::env::var_os("PATH").unwrap_or_default();
    let real = std::env::split_paths(&path).map(|d| d.join("tmux")).find(|p| p.is_file()).expect("tmux on PATH");
    env.fake("tmux", &format!("case \" $* \" in *\" set \"*\" @cpd_session \"*) echo refused >&2; exit 1;; esac\nexec '{}' \"$@\"", real.display()));
    let run = env.cpd(&["plan", "new", "second", "--context", "ctx", "--title", "Plan second"]);

    assert_eq!(run.error()["code"], "tmux_failed", "{run:?}");
    assert_eq!(windows(), before, "plan new left a window");
}

/// `plan new` with a brief file whose text is `text`.
fn new_with_brief(env: &TestEnv, id: &str, text: &str) -> Run {
    let file = env.root().join(format!("{id}-brief.md"));
    fs::write(&file, text).unwrap();
    env.cpd(&["plan", "new", id, "--context", "ctx", "--title", &format!("Plan {id}"), "--brief-file", file.to_str().unwrap()])
}

fn briefs(env: &TestEnv) -> Vec<Value> {
    let shown = env.cpd(&["context", "show", "--context", "ctx"]).ok();
    shown["records"].as_array().unwrap().iter().filter(|r| r["kind"] == "brief").cloned().collect()
}

#[test]
fn new_with_a_brief_file_writes_one_brief_record_and_the_lead_gets_it_in_full() {
    let env = TestEnv::new();
    setup(&env);
    let text = "Retry the failed payouts.\n\nMore detail, second paragraph.\n";
    let out = new_with_brief(&env, "pay", text).ok();
    assert_eq!(out["record"], "m1-p1", "the output still names the plan record");

    // One `brief` record, letter `b`, its Plan `{id, machine}`, the file's text, one `context-record` Event.
    let rows: Vec<(String, String, String, Option<String>)> = store(&env)
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT id, text, plan_id || '/' || plan_machine, plan_jira FROM context_records WHERE kind = 'brief'")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(rows, [("m1-b1".to_owned(), text.to_owned(), "pay/m1".to_owned(), None)]);
    let recorded: Vec<Value> = events(&env, "context-record").into_iter().filter(|e| e["data"]["kind"] == "brief").collect();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0]["data"]["record"], "m1-b1");

    // `context show` lists it short, under its Plan; `--record` prints it whole.
    let shown = env.cpd(&["context", "show", "--context", "ctx"]).ok();
    let brief = &briefs(&env)[0];
    assert_eq!((&brief["text"], &brief["short"], &brief["plan"]), (&json!("Retry the failed payouts."), &json!(true), &json!({"id": "pay", "machine": "m1"})));
    let md = shown["markdown"].as_str().unwrap();
    assert!(md.contains("  - brief m1-b1 (") && md.contains("): Retry the failed payouts.\n"), "{md}");
    assert!(!md.contains("second paragraph"), "{md}");
    let full = env.cpd(&["context", "show", "--context", "ctx", "--record", "m1-b1"]).ok();
    assert_eq!(full["records"][0]["text"], text);
    assert!(full["records"][0].get("short").is_none());
    assert!(full["markdown"].as_str().unwrap().contains("second paragraph"));

    // The Lead's first message: header, the full brief, then the records; its own brief not listed twice.
    let message = fs::read_to_string(home(&env).lead_message(&pid("pay"))).unwrap();
    let header = "You are the Lead of Plan pay \"Plan pay\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\nRetry the failed payouts.\n\nMore detail, second paragraph.\n\n# Payments\n";
    assert!(message.starts_with(header), "{message}");
    assert_eq!(message.matches("Retry the failed payouts.").count(), 1, "{message}");

    // Another Plan's Lead sees this brief short.
    new_with_brief(&env, "web", "Build the web page.\nSecond line.\n").ok();
    let message = fs::read_to_string(home(&env).lead_message(&pid("web"))).unwrap();
    assert!(message.contains("brief m1-b1") && message.contains("Retry the failed payouts."), "{message}");
    assert!(!message.contains("second paragraph"), "{message}");
    assert_eq!(message.matches("Build the web page.").count(), 1, "{message}");
    assert_eq!(briefs(&env).len(), 2);
}

#[test]
fn new_again_with_the_brief_writes_no_second_brief_record() {
    let env = TestEnv::new();
    setup(&env);
    new_with_brief(&env, "pay", "The brief.\n").ok();
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");
    let again = new_with_brief(&env, "pay", "The brief.\n").ok();
    assert_eq!(again["already"], true);
    assert_eq!(briefs(&env).len(), 1);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns, "nothing written");

    // A first run that stopped before the brief record (here: run without the file): the rerun writes it, once.
    new_plan_only(&env, "web");
    assert_eq!(briefs(&env).len(), 1);
    assert_eq!(new_with_brief(&env, "web", "The web brief.\n").ok()["already"], true);
    assert_eq!(new_with_brief(&env, "web", "The web brief.\n").ok()["already"], true);
    assert_eq!(briefs(&env).len(), 2);

    // A Plan without a brief file has no brief record.
    new_plan_only(&env, "api");
    assert_eq!(briefs(&env).len(), 2);
}

// ---------- the first message: the digest and the records after it (hs-d194 D6) ----------

/// Notes `note-<from>` to `note-<to>` in Context `ctx`; the `seq` of each.
fn notes(env: &TestEnv, from: u32, to: u32) -> Vec<i64> {
    (from..=to).map(|n| env.cpd(&["context", "note", &format!("note-{n:02}"), "--context", "ctx"]).ok()["seq"].as_i64().unwrap()).collect()
}

/// The Context's digest file `<upto>.md`, as the distiller leaves it (a first message reads it, it does not validate it).
fn digest_at(env: &TestEnv, upto: i64, text: &str) {
    let context = ContextId::parse("ctx").unwrap();
    fs::create_dir_all(home(env).context_digest_dir(&context)).unwrap();
    fs::write(home(env).context_digest(&context, u64::try_from(upto).unwrap()), text).unwrap();
}

const LEAD_HEADER: &str = "You are the Lead of Plan pay \"Plan pay\" in Context ctx. Phase grounding: follow the cpd-lead skill.";

fn lead_message(env: &TestEnv, plan: &str) -> String {
    fs::read_to_string(home(env).lead_message(&pid(plan))).unwrap()
}

/// The positions of `needles` in `text`, each after the one before it.
fn in_order(text: &str, needles: &[&str]) {
    let mut at = 0;
    for needle in needles {
        let found = text[at..].find(needle).unwrap_or_else(|| panic!("{needle:?} after byte {at} in\n{text}"));
        at += found + needle.len();
    }
}

#[test]
fn a_first_message_with_no_digest_holds_every_record() {
    let env = TestEnv::new();
    setup(&env);
    notes(&env, 1, 25);
    new_plan_only(&env, "pay");
    let shown = env.cpd(&["context", "show", "--context", "ctx"]).ok();
    let message = lead_message(&env, "pay");
    assert_eq!(message, format!("{LEAD_HEADER}\n\n{}\n", shown["markdown"].as_str().unwrap()), "today's message: the Context as `context show` renders it");
    assert!(message.contains("note-01") && message.contains("note-25"), "{message}");
    assert!(!message.contains("more: cpd context show"), "{message}");
}

#[test]
fn a_first_message_holds_the_digest_the_first_20_records_after_it_and_a_pointer() {
    let env = TestEnv::new();
    setup(&env);
    let before = notes(&env, 1, 5);
    let upto = *before.last().unwrap();
    digest_at(&env, upto, "# Payments: digest\n\nThe ledger rules [m1-n1, m1-n2]; retries [m1-n3, m1-n4, m1-n5].\n");
    let after = notes(&env, 6, 30);
    new_with_brief(&env, "pay", "Retry the failed payouts.\n").ok();

    let message = lead_message(&env, "pay");
    // Newer than the digest: 25 notes, the `plan` record and this Plan's own brief (given in full, so not listed): 26 to list, 20 listed.
    let listed: Vec<String> = (6..=25).map(|n| format!("note-{n:02}")).collect();
    let first: Vec<&str> = ["Retry the failed payouts.", "# Payments: digest", "The ledger rules [m1-n1, m1-n2]"]
        .into_iter()
        .chain(listed.iter().map(String::as_str))
        .collect();
    in_order(&message, &first);
    assert!(message.starts_with(&format!("{LEAD_HEADER}\n\nRetry the failed payouts.\n\n# Payments: digest\n")), "{message}");
    for n in (1..=5).chain(26..=30) {
        assert!(!message.contains(&format!("note-{n:02}")), "note-{n:02} is in the digest or past the 20, not listed: {message}");
    }
    assert_eq!(message.matches("Retry the failed payouts.").count(), 1, "own brief not listed again: {message}");
    assert!(!message.contains("m1-p1"), "the `plan` record is among the 6 not listed: {message}");
    let pointer = format!("and 6 more: cpd context show --context ctx --since {}", after[19]);
    assert!(message.ends_with(&format!("\n\n{pointer}\n")), "{message}");

    // The pointer lists what the message left out, the `plan` record too.
    let rest = env.cpd(&["context", "show", "--context", "ctx", "--since", &after[19].to_string()]).ok();
    let markdown = rest["markdown"].as_str().unwrap();
    for n in 26..=30 {
        assert!(markdown.contains(&format!("note-{n:02}")), "{markdown}");
    }
    assert!(markdown.contains("m1-p1"), "{markdown}");
    assert!(!markdown.contains("note-25"), "{markdown}");
}

#[test]
fn a_first_message_names_no_more_records_up_to_20_and_the_count_beyond() {
    let env = TestEnv::new();
    setup(&env);
    let upto = *notes(&env, 1, 3).last().unwrap();
    digest_at(&env, upto, "Digest [m1-n1, m1-n2, m1-n3].\n");
    notes(&env, 4, 22);
    // 19 notes and this Plan's `plan` record: exactly 20 newer records.
    new_plan_only(&env, "pay");
    let message = lead_message(&env, "pay");
    assert!(message.contains("Digest [m1-n1, m1-n2, m1-n3].") && message.contains("note-04") && message.contains("note-22") && message.contains("m1-p1"), "{message}");
    assert!(!message.contains("note-01") && !message.contains("more: cpd context show"), "{message}");
    // One more Plan makes 21: the 21st is counted and pointed to.
    new_plan_only(&env, "web");
    let message = lead_message(&env, "web");
    let plan = env.cpd(&["context", "show", "--context", "ctx", "--record", "m1-p1"]).ok()["records"][0]["seq"].as_i64().unwrap();
    assert!(message.ends_with(&format!("\n\nand 1 more: cpd context show --context ctx --since {}\n", plan)), "{message}");
    assert!(message.contains("m1-p1") && !message.contains("m1-p2"), "{message}");
}

#[test]
fn a_first_message_with_a_digest_and_nothing_newer_is_the_digest() {
    let env = TestEnv::new();
    setup(&env);
    let upto = env.cpd(&["context", "note", "A note.", "--context", "ctx"]).ok()["seq"].as_i64().unwrap();
    digest_at(&env, upto + 1, "Digest [m1-n1].\n");
    digest_at(&env, upto + 10, "A later digest [m1-n1], past every record.\n");
    new_plan_only(&env, "pay");
    let message = lead_message(&env, "pay");
    assert_eq!(message, format!("{LEAD_HEADER}\n\nA later digest [m1-n1], past every record.\n"), "the digest with the highest upto_seq, and no records after it: {message}");
}

#[test]
fn a_managers_first_message_is_the_same_digest_and_records() {
    let env = TestEnv::new();
    setup(&env);
    let upto = *notes(&env, 1, 5).last().unwrap();
    digest_at(&env, upto, "# Payments: digest\n\nEverything [m1-n1, m1-n2, m1-n3, m1-n4, m1-n5].\n");
    let after = notes(&env, 6, 30);
    new_plan_only(&env, "pay");
    env.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let message = fs::read_to_string(home(&env).manager_message(&ContextId::parse("ctx").unwrap())).unwrap();
    let header = "You are the Manager of Context ctx \"Payments\". Follow the cpd-manager skill.";
    let listed: Vec<String> = (6..=25).map(|n| format!("note-{n:02}")).collect();
    let all: Vec<&str> = ["# Payments: digest", "Everything [m1-n1"].into_iter().chain(listed.iter().map(String::as_str)).collect();
    assert!(message.starts_with(&format!("{header}\n\n# Payments: digest\n")), "{message}");
    in_order(&message, &all);
    assert!(!message.contains("note-26") && !message.contains("note-05"), "{message}");
    // 25 notes and the `plan` record are newer than the digest: 6 are not listed.
    assert!(message.ends_with(&format!("\n\nand 6 more: cpd context show --context ctx --since {}\n", after[19])), "{message}");
}

#[test]
fn new_refuses_an_empty_brief_file_and_writes_nothing() {
    let env = TestEnv::new();
    setup(&env);
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");
    refused(&new_with_brief(&env, "pay", ""), "usage", 2);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans"), 0);
}

#[test]
fn new_takes_neither_repo_nor_base() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let repo_arg = repo.to_str().unwrap();
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");
    for extra in [["--repo", repo_arg], ["--base", "main"]] {
        let mut args = vec!["plan", "new", "pay", "--context", "ctx", "--title", "T"];
        args.extend(extra);
        refused(&env.cpd(&args), "usage", 2);
    }
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans"), 0);
    assert!(panes(&env).is_empty());
}

#[test]
fn new_again_with_the_same_flags_is_already_and_redoes_missing_steps() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let first = new_plan_only(&env, "pay");
    let session = first["plan"]["lead_session"].as_str().unwrap().to_owned();
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");

    // From another directory: still `already`, and the stored cwd stays.
    let again = env.cpd_in(&repo, &["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!((&again["plan"], &again["record"]), (&first["plan"], &first["record"]));
    assert_eq!((&again["lead"]["pane"], &again["lead"]["window"]), (&first["lead"]["pane"], &first["lead"]["window"]));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns, "nothing written");
    assert_eq!(panes(&env).len(), 1);

    // A crash after the first transaction: no pane, no message. Rerunning redoes both.
    store(&env).write("fixture", |tx| exec(tx, &format!("UPDATE sessions SET tmux_pane = NULL WHERE id = '{session}';"))).unwrap();
    fs::remove_file(home(&env).lead_message(&pid("pay"))).unwrap();
    env.tmux(&["kill-server"]);
    let redone = env.cpd_in(&repo, &["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok();
    assert_eq!(redone["already"], true);
    assert_eq!(redone["lead"]["cwd"], first["lead"]["cwd"]);
    let path = env.tmux(&["display-message", "-p", "-t", redone["lead"]["pane"].as_str().unwrap(), "#{pane_current_path}"]);
    assert_eq!(String::from_utf8(path.stdout).unwrap().trim(), first["lead"]["cwd"].as_str().unwrap(), "redone in the stored cwd");
    assert_ne!(redone["lead"]["pane"], Value::Null);
    assert_eq!(panes(&env).len(), 1);
    assert!(home(&env).lead_message(&pid("pay")).exists());
    assert_eq!(events(&env, "session-moved").len(), 2);
    assert_eq!(events(&env, "plan-created").len(), 1);

    // Other flags: plan_exists.
    let error = refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Other"]), "plan_exists", 3);
    assert_eq!(error["details"]["plan"]["title"], "Plan pay");
    refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay", "--name", "x"]), "plan_exists", 3);
}

#[test]
fn new_again_in_an_archived_context_is_already_not_archived() {
    let env = TestEnv::new();
    setup(&env);
    let first = new_plan_only(&env, "pay");
    env.cpd(&["context", "archive", "ctx"]).ok();
    let again = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["plan"], first["plan"]);
    // A new Plan, and a rerun with other flags, are still refused in this order.
    refused(&env.cpd(&["plan", "new", "other", "--context", "ctx", "--title", "T"]), "archived", 3);
    refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Other"]), "plan_exists", 3);
}

#[test]
fn new_and_rename_titles_are_1_to_200_characters_with_no_control_characters() {
    let env = TestEnv::new();
    setup(&env);
    let (long, ok) = ("a".repeat(201), "a".repeat(200));
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");
    for title in [long.as_str(), "a\nb", "a\tb", "a\u{7f}b"] {
        refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", title]), "usage", 2);
        refused(&env.cpd(&["plan", "rename", "--plan", "nope", "--title", title]), "usage", 2);
    }
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);
    env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", &ok]).ok();
    for title in [long.as_str(), "a\nb", "a\tb"] {
        refused(&env.cpd(&["plan", "rename", "--plan", "pay", "--title", title]), "usage", 2);
    }
    assert!(events(&env, "plan-renamed").is_empty());
    let renamed = "b".repeat(200);
    assert_eq!(env.cpd(&["plan", "rename", "--plan", "pay", "--title", &renamed]).ok()["plan"]["title"], renamed.as_str());
}

#[test]
fn new_refusals_write_nothing() {
    let env = TestEnv::new();
    setup(&env);
    let new = |extra: &[&str]| {
        let mut args = vec!["plan", "new", "pay", "--title", "T"];
        args.extend(extra);
        env.cpd(&args)
    };
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");

    refused(&new(&["--context", "nope"]), "context_not_found", 4);
    refused(&new(&["--context", "ctx", "--brief-file", "/no/such/brief"]), "file_unreadable", 2);
    refused(&env.cpd(&["plan", "new", "Bad", "--context", "ctx", "--title", "T"]), "invalid_plan_id", 2);
    refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx"]), "usage", 2);
    refused(&new(&["--context", "ctx", "--name", ""]), "invalid_name", 2);
    refused(&new(&["--context", "ctx", "--jira", "nope"]), "usage", 2);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans"), 0);
}

#[test]
fn pr_add_defaults_the_base_to_origin_head_and_needs_it_on_origin() {
    let env = TestEnv::new();
    let origin = setup(&env);
    env.git(&origin, &["branch", "trunk"]);
    env.git(&origin, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
    let clone = env.root().join("repos").join("clone");
    env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
    // From a subdirectory of a linked worktree: the repo is the main worktree.
    let linked = env.root().join("repos").join("clone-linked");
    env.git(&clone, &["worktree", "add", "-q", linked.to_str().unwrap(), "-b", "side"]);
    fs::create_dir_all(linked.join("sub")).unwrap();

    new_plan_only(&env, "pay");
    let out = env.cpd(&["pr", "add", "pay", "--repo", linked.join("sub").to_str().unwrap(), "--plan", "pay"]).ok();
    assert_eq!(out["pr"]["base"], "trunk");
    assert_eq!(out["pr"]["repo"], clone.canonicalize().unwrap().display().to_string());
    let error = refused(
        &env.cpd(&["pr", "add", "other", "--repo", clone.to_str().unwrap(), "--base", "side", "--plan", "pay"]),
        "base_not_found",
        4,
    );
    assert_eq!(error["details"]["ref"], "refs/remotes/origin/side", "with an origin, the base must be on it");
}

// ---------- plan phase ----------

#[test]
fn phase_plan_writes_the_phase_only_and_a_later_plan_is_a_fresh_lead() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let first = new_plan(&env, "pay", &repo);
    let lead = first["plan"]["lead_session"].as_str().unwrap().to_owned();
    let (sessions, windows, events_before) = (
        query::<i64>(&env, "SELECT count(*) FROM sessions"),
        panes(&env).len(),
        query::<i64>(&env, "SELECT count(*) FROM events"),
    );

    let out = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "phase": "plan"}), "no `grounded_lead`");
    let phase = &events(&env, "plan-phase")[0];
    assert_eq!((&phase["plan"], &phase["data"]), (&json!("pay"), &json!({"from": "grounding", "to": "plan"})));
    // No Session, no window, no Event but `plan-phase`; no `context-grounded` ever.
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions"), sessions);
    assert_eq!(panes(&env).len(), windows);
    assert!(panes(&env).iter().all(|p| p.1 != "ctx-ctx"));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM events"), events_before + 1);
    assert_eq!(events(&env, "context-grounded").len(), 0);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions WHERE plan IS NULL"), 0);

    // Already in phase plan: nothing is written.
    let again = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(again, json!({"ok": true, "plan": "pay", "phase": "plan", "already": true}));
    assert_eq!(events(&env, "plan-phase").len(), 1);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM events"), events_before + 1);

    // A later Plan of the Context is a fresh Lead, never a fork: its first message holds every record.
    env.cpd(&["context", "note", "Recorded after the first Plan.", "--context", "ctx"]).ok();
    let second = new_plan(&env, "refund", &repo);
    assert_eq!(second["lead"]["forked_from"], Value::Null);
    let session = second["lead"]["session"].as_str().unwrap();
    assert_ne!(session, lead);
    assert_eq!(query::<Option<String>>(&env, &format!("SELECT forked_from FROM sessions WHERE id = '{session}'")), None);
    assert_eq!(events(&env, "plan-created").last().unwrap()["data"]["forked_from"], Value::Null);
    assert_eq!(events(&env, "session-registered").last().unwrap()["data"]["forked_from"], Value::Null);
    let message = fs::read_to_string(home(&env).lead_message(&pid("refund"))).unwrap();
    assert!(message.contains("Recorded after the first Plan."), "{message}");
    assert!(message.contains("m1-p1") && message.contains("m1-p2"), "every record, the first Plan's too: {message}");
}

#[test]
fn phase_refusals() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let error = refused(&env.cpd(&["plan", "phase", "dispatch", "--plan", "pay"]), "illegal_transition", 3);
    assert_eq!((&error["details"]["from"], &error["details"]["to"]), (&json!("grounding"), &json!("dispatch")));
    refused(&env.cpd(&["plan", "phase", "grounding", "--plan", "pay"]), "illegal_transition", 3);
    refused(&env.cpd(&["plan", "phase", "bogus", "--plan", "pay"]), "usage", 2);
    refused(&env.cpd(&["plan", "phase", "plan"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "phase", "plan", "--plan", "nope"]), "plan_not_found", 4);
    store(&env)
        .write("fixture", |tx| exec(tx, "UPDATE plans SET phase = 'plan' WHERE id = 'pay'; UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
                 UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay';"))
        .unwrap();
    let error = refused(&cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "phase", "plan"]), "illegal_transition", 3);
    assert_eq!(error["details"]["from"], "dispatch");
}

/// A Plan with no PR yet (`plan new` alone) is found by every Plan read: phase, list, show, archive and unarchive.
#[test]
fn a_plan_with_no_pr_is_read_by_phase_list_show_archive_and_unarchive() {
    let env = TestEnv::new();
    setup(&env);
    new_plan_only(&env, "pay");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM prs"), 0);

    assert_eq!(show(&env, "pay")["prs"], json!([]));
    let ids = |env: &TestEnv| -> Vec<String> {
        env.cpd(&["plan", "list"]).ok()["plans"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(ids(&env), ["pay"]);

    let archived = env.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(archived["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert_ne!(show(&env, "pay")["plan"]["archived_at"], Value::Null);
    env.cpd(&["plan", "unarchive", "--plan", "pay"]).ok();
    assert_eq!(show(&env, "pay")["plan"]["archived_at"], Value::Null);

    let out = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!((&out["plan"], &out["phase"]), (&json!("pay"), &json!("plan")));
    assert_eq!(show(&env, "pay")["plan"]["phase"], "plan");
    // A Plan with no PR has no Job: lock keeps its own refusal.
    refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "no_draft_jobs", 3);
}

/// Plans moving to phase `plan` at once all move, and none registers a Session.
#[test]
fn plans_moving_to_phase_plan_at_once_all_move() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let plans = ["pa", "pb", "pc", "pd", "pe", "pf"];
    for plan in plans {
        new_plan(&env, plan, &repo);
    }
    let sessions = query::<i64>(&env, "SELECT count(*) FROM sessions");
    let barrier = std::sync::Barrier::new(plans.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = plans
            .iter()
            .map(|plan| {
                let (env, barrier) = (&env, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    env.cpd(&["plan", "phase", "plan", "--plan", plan]).ok()
                })
            })
            .collect();
        handles.into_iter().for_each(|h| drop(h.join().unwrap()));
    });
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions"), sessions);
    assert_eq!(events(&env, "context-grounded").len(), 0);
    assert_eq!(events(&env, "plan-phase").len(), plans.len());
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans WHERE phase = 'plan'"), plans.len() as i64);
}

// ---------- plan list, plan show ----------

#[test]
fn list_and_show_are_snapshots_with_an_events_cursor() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    new_plan(&env, "refund", &repo);
    env.cpd(&["context", "new", "other", "--title", "Other"]).ok();

    let list = env.cpd(&["plan", "list"]).ok();
    let cursor = list["events_cursor"].as_i64().unwrap();
    assert_eq!(cursor, env.cpd(&["events", "list", "--since", "0", "--limit", "5000"]).ok()["next"].as_i64().unwrap());
    let plans = list["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 2);
    let pay = &plans[0];
    assert_eq!((&pay["id"], &pay["context"], &pay["phase"]), (&json!("pay"), &json!("ctx"), &json!("plan")));
    assert_eq!(pay["counts"], json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 0}));
    assert_eq!(pay["prs"][0]["repo"], repo.canonicalize().unwrap().display().to_string());
    assert_eq!((&pay["prs"][0]["key"], &pay["prs"][0]["pr"]), (&json!("pay"), &Value::Null));
    let ids = |args: &[&str]| -> Vec<String> {
        env.cpd(args).ok()["plans"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(ids(&["plan", "list", "--phase", "grounding"]), ["refund"]);
    assert_eq!(ids(&["plan", "list", "--context", "ctx", "--phase", "plan"]), ["pay"]);
    assert!(ids(&["plan", "list", "--context", "other"]).is_empty());
    assert_eq!(cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "list"]).ok()["plans"].as_array().unwrap().len(), 2, "filters only");
    refused(&env.cpd(&["plan", "list", "--phase", "bogus"]), "usage", 2);

    let out = show(&env, "pay");
    let plan = &out["plan"];
    assert_eq!((&plan["id"], &plan["phase"]), (&json!("pay"), &json!("plan")));
    let pr = &out["prs"][0];
    assert_eq!((&pr["lead_worktree"], &pr["pr"]), (&Value::Null, &Value::Null));
    assert_eq!(pr["pull_request"], Value::Null);
    let jobs = out["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!((&jobs[1]["key"], &jobs[1]["needs"], &jobs[1]["status"]), (&json!("b"), &json!(["a"]), &json!("draft")));
    assert_eq!((&jobs[1]["touches"], &jobs[1]["attempts"], &jobs[1]["fingerprint"]), (&json!(["src/b.rs"]), &json!([]), &Value::Null));
    assert_eq!((&out["review"], &out["escalations"], &out["tamper"]), (&Value::Null, &json!([]), &json!([])));
    assert_eq!(out["counts"]["draft"], 2);
    assert!(out["events_cursor"].as_i64().unwrap() >= cursor);
    assert_eq!(cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "show"]).ok()["plan"]["id"], "pay");

    refused(&env.cpd(&["plan", "show"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "show", "--plan", "nope"]), "plan_not_found", 4);
    refused(&env.cpd(&["plan", "show", "--plan", "Bad"]), "invalid_plan_id", 2);
}

#[test]
fn show_lists_attempts_open_escalations_and_tampered_fingerprints() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    let fingerprint = "f".repeat(64);
    store(&env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
                     UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay';
                     UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE plan = 'pay';
                     UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'a';
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 't1');
                     INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                       VALUES ('pay', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', 1, 't2');"
                ),
            )
        })
        .unwrap();
    let out = show(&env, "pay");
    let a = &out["jobs"][0];
    assert_eq!(
        a["attempts"],
        json!([{"id": "a-a1", "number": 1, "tier": "S2", "origin": "ladder", "status": "running", "outcome": null,
                "cause": null, "worker_session": null, "started_at": "t1", "ended_at": null}])
    );
    assert_eq!(out["escalations"], json!([{"id": "a-a1-q1", "attempt": "a-a1", "question": "Which?", "asked_at": "t2"}]));
    assert_eq!(out["counts"]["running"], 1);
    let tamper = out["tamper"].as_array().unwrap();
    assert_eq!(tamper.len(), 2, "both Locked Jobs differ from the stored fingerprint: {tamper:?}");
    assert_eq!((&tamper[0]["kind"], &tamper[0]["job"], &tamper[0]["expected"]), (&json!("fingerprint"), &json!("a"), &json!(fingerprint)));
}

// ---------- plan render ----------

#[test]
fn render_writes_the_plan_markdown_and_its_fingerprint() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    add_job(&env, "pay", "gone", &[]);
    store(&env).write("fixture", |tx| exec(tx, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'gone';")).unwrap();

    let out = env.cpd(&["plan", "render", "--plan", "pay"]).ok();
    let path = home(&env).plan_md(&pid("pay"));
    assert_eq!(out["path"], path.display().to_string());
    assert_eq!(out["draft_jobs"], 2);
    let (pr_key, pr_repo, pr_base): (String, String, String) = store(&env)
        .read(|c| Ok(c.query_row("SELECT key, repo, base FROM prs WHERE plan = 'pay'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    let spec = |key: &str, needs: &str| {
        format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"key\":\"{key}\",\"needs\":[{needs}],\"out_of_scope\":[],\"pr\":\"{pr_key}\",\"tier\":\"S2\",\"title\":\"Job {key}\",\"touches\":[\"src/{key}.rs\"]}}"
        )
    };
    let prs = format!("[{{\"base\":\"{pr_base}\",\"key\":\"{pr_key}\",\"needs\":[],\"repo\":{}}}]", serde_json::to_string(&pr_repo).unwrap());
    assert_eq!(out["fingerprint"], sha256(&format!("{{\"jobs\":[{},{}],\"prs\":{prs}}}", spec("a", ""), spec("b", "\"a\""))));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Plan: Plan pay\n\nContext: `ctx` · Phase: `plan`\n"), "{text}");
    assert!(text.contains("| `b` | Job b | S2 | draft | `a` | `src/b.rs` |"), "{text}");
    assert!(text.contains("```mermaid\ngraph TD\n  j1[\"a\"]\n  j2[\"b\"]\n  j1 --> j2\n```"), "{text}");
    assert!(text.contains("\n## PR pay\n"), "{text}");
    assert!(text.contains("\n### `a`: Job a\n"), "{text}");
    assert!(text.contains("\n#### Goal\n\nGoal of b.\n"), "{text}");
    assert_eq!(out["prs"], 1);
    assert!(!text.contains("gone"), "cancelled Jobs are left out: {text}");

    // Idempotent: the same Jobs render the same file.
    let again = env.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_eq!(again, out);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    refused(&env.cpd(&["plan", "render"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "render", "--plan", "nope"]), "plan_not_found", 4);
}

// ---------- plan review, plan decide ----------

#[test]
fn review_requests_a_pending_review_once_per_fingerprint() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let error = refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "illegal_phase", 3);
    assert_eq!(error["details"]["phase"], "grounding");
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "no_draft_jobs", 3);
    add_job(&env, "pay", "a", &[]);

    let out = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    let fingerprint = env.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"].clone();
    let review_path = home(&env).plan_review_md(&pid("pay"), ReviewId::parse("r1").unwrap());
    assert_eq!((&out["review"]["id"], &out["review"]["status"], &out["review"]["fingerprint"]), (&json!("r1"), &json!("pending"), &fingerprint));
    assert_eq!(out["path"], review_path.display().to_string());
    assert_eq!(out["next"], "The User reviews the plan in plannotator. Stop and wait for the decision message.");
    assert_eq!(fs::read_to_string(&review_path).unwrap(), fs::read_to_string(home(&env).plan_md(&pid("pay"))).unwrap());
    let event = &events(&env, "plan-review")[0];
    assert_eq!((&event["plan"], &event["data"]), (&json!("pay"), &json!({"review": "r1", "fingerprint": fingerprint, "status": "pending"})));

    // Same Jobs: the pending review again.
    let again = cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "review"]).ok();
    assert_eq!((&again["already"], &again["review"]), (&json!(true), &out["review"]));
    assert_eq!(events(&env, "plan-review").len(), 1);

    // Changed Jobs: a new review; the copy of r1 is what the User saw.
    let r1 = fs::read_to_string(&review_path).unwrap();
    edit_goal(&env, "pay", "a", "A new goal.");
    let r2 = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    assert_eq!(r2["review"]["id"], "r2");
    assert_ne!(r2["review"]["fingerprint"], fingerprint);
    assert_eq!(fs::read_to_string(&review_path).unwrap(), r1);
    assert!(fs::read_to_string(r2["path"].as_str().unwrap()).unwrap().contains("A new goal."));
    assert_eq!(show(&env, "pay")["review"]["id"], "r2");
    refused(&env.cpd(&["plan", "review"]), "plan_required", 2);
}

#[test]
fn decide_records_the_decision_once() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    let feedback = env.root().join("feedback.md");
    fs::write(&feedback, "Split job b.").unwrap();

    refused(&env.cpd(&["plan", "decide", "r1", "--plan", "pay"]), "usage", 2);
    refused(&env.cpd(&["plan", "decide", "r1", "--approve", "--reject", "--plan", "pay"]), "usage", 2);
    refused(&env.cpd(&["plan", "decide", "x1", "--approve", "--plan", "pay"]), "invalid_review_id", 2);
    refused(&env.cpd(&["plan", "decide", "r9", "--approve", "--plan", "pay"]), "review_not_found", 4);
    refused(&env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay", "--feedback-file", "/no/file"]), "file_unreadable", 2);

    let out = env.cpd(&["plan", "decide", "r1", "--reject", "--feedback-file", feedback.to_str().unwrap(), "--plan", "pay"]).ok();
    assert_eq!((&out["review"]["status"], &out["review"]["feedback"], &out["current"], &out["next"]), (&json!("rejected"), &json!("Split job b."), &json!(true), &Value::Null));
    assert!(out["review"]["decided_at"].is_string());
    let decided = events(&env, "plan-review").last().unwrap().clone();
    assert_eq!(decided["data"]["status"], "rejected");
    assert_eq!(decided["data"]["feedback"], "Split job b.");
    let again = env.cpd(&["plan", "decide", "r1", "--reject", "--plan", "pay"]).ok();
    assert_eq!(again["already"], true);
    let error = refused(&env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"]), "review_decided", 3);
    assert_eq!(error["details"]["status"], "rejected");
    assert_eq!(events(&env, "plan-review").len(), 2);

    // Approved, then the Jobs change: no longer current, so no plan lock.
    let r2 = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    assert_eq!(r2["review"]["id"], "r2");
    let out = env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&out["review"]["status"], &out["review"]["feedback"], &out["current"], &out["next"]), (&json!("approved"), &Value::Null, &json!(true), &json!("cpd plan lock")));
    edit_goal(&env, "pay", "a", "Changed.");
    let again = env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&again["already"], &again["current"], &again["next"]), (&json!(true), &json!(false), &Value::Null));
}

// ---------- a review of the drafts in a phase after plan (hs-d173, hs-d174, hs-d179) ----------

/// The Locked-Job fingerprint of a Job written by `add_job` (contract §2.3).
fn locked_fingerprint_of(key: &str, needs: &[&str]) -> String {
    let needs: Vec<String> = needs.iter().map(|n| format!("\"{n}\"")).collect();
    sha256(&format!(
        "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"needs\":[{}],\"out_of_scope\":[],\"tier\":\"S2\",\"touches\":[\"src/{key}.rs\"]}}",
        needs.join(",")
    ))
}

fn job_status(env: &TestEnv, key: &str) -> String {
    query(env, &format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
}

#[test]
fn a_review_after_the_lock_covers_the_drafts_and_its_approval_queues_exactly_them() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    // Locked, every Job queued: there is nothing to review.
    refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "no_draft_jobs", 3);

    add_draft(&env, "pay", "c", &["a"], true);
    add_draft(&env, "pay", "d", &["c"], true);
    let out = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    let spec = |key: &str, needs: &str| {
        format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"key\":\"{key}\",\"needs\":[{needs}],\"out_of_scope\":[],\"pr\":\"pay\",\"tier\":\"S2\",\"title\":\"Job {key}\",\"touches\":[\"src/{key}.rs\"]}}"
        )
    };
    // The fingerprint covers exactly the drafts, and no PR.
    let fingerprint = sha256(&format!("{{\"jobs\":[{},{}],\"prs\":[]}}", spec("c", "\"a\""), spec("d", "\"c\"")));
    assert_eq!((&out["review"]["id"], &out["review"]["status"], &out["review"]["fingerprint"]), (&json!("r2"), &json!("pending"), &json!(fingerprint)));
    assert_eq!(events(&env, "plan-review").last().unwrap()["data"], json!({"review": "r2", "fingerprint": fingerprint, "status": "pending"}));

    // The render marks the drafts under review, and the Locked Jobs as context.
    let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();
    assert!(text.contains("| Key | Title | Tier | Status | Needs | Touches | Review |"), "{text}");
    assert!(text.contains("| `c` | Job c | S2 | draft | `a` | `src/c.rs` | **under review** |"), "{text}");
    assert!(text.contains("| `a` | Job a | S2 | queued | none | `src/a.rs` | Locked |"), "{text}");
    assert!(text.contains("\nStatus: `draft` · **under review** · Tier: `S2` · Needs: `c`\n"), "{text}");
    assert!(text.contains("\nStatus: `queued` · Locked · Tier: `S2` · Needs: `a`\n"), "{text}");
    assert_eq!(env.cpd(&["plan", "render", "--plan", "pay"]).ok()["draft_jobs"], 2);

    // A PR appended after the lock is not reviewed: the review stays current.
    let web = env.git_repo("web");
    env.cpd(&["pr", "add", "web", "--repo", web.to_str().unwrap(), "--base", "main", "--plan", "pay"]).ok();
    assert_eq!(show(&env, "pay")["review"]["current"], true);

    // Rejected: the drafts stay drafts, editable.
    let out = env.cpd(&["plan", "decide", "r2", "--reject", "--plan", "pay"]).ok();
    assert!(out.get("jobs").is_none(), "{out}");
    assert!(events(&env, "plan-review").last().unwrap()["data"].get("jobs").is_none());
    assert_eq!((job_status(&env, "c"), job_status(&env, "d")), ("draft".into(), "draft".into()));
    edit_goal(&env, "pay", "d", "Goal of d.");

    // Approved and current: in one transaction exactly the drafts are queued with their fingerprints.
    let phase_events = events(&env, "plan-phase").len();
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    let out = env.cpd(&["plan", "decide", "r3", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&out["review"]["status"], &out["current"], &out["next"], &out["jobs"]), (&json!("approved"), &json!(true), &Value::Null, &json!(["c", "d"])));
    let decided = events(&env, "plan-review").last().unwrap().clone();
    assert_eq!(decided["data"], json!({"review": "r3", "fingerprint": fingerprint, "status": "approved", "feedback": null, "jobs": ["c", "d"]}));
    assert!(decided["text"].as_str().unwrap().contains("no lock is needed"), "{decided}");
    let shown = show(&env, "pay");
    assert_eq!(shown["plan"]["phase"], "dispatch");
    assert_eq!(events(&env, "plan-phase").len(), phase_events, "the phase does not change");
    for (key, needs) in [("c", &["a"]), ("d", &["c"])] {
        let job = shown["jobs"].as_array().unwrap().iter().find(|j| j["key"] == key).unwrap();
        assert_eq!((&job["status"], &job["appended"]), (&json!("queued"), &json!(true)), "{key}");
        assert_eq!(job["fingerprint"], locked_fingerprint_of(key, needs), "{key}");
    }
    // The tamper check holds for them: the fingerprints match what the engine recomputes.
    assert_eq!(shown["tamper"], json!([]));
    // Idempotent: a rerun reports the same Jobs and writes nothing.
    let again = env.cpd(&["plan", "decide", "r3", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&again["already"], &again["jobs"], &again["next"]), (&json!(true), &json!(["c", "d"]), &Value::Null));
    assert_eq!(events(&env, "plan-review").len(), 6, "r1 and r2 requested and decided, r3 requested and approved once");

    // A stale approval queues nothing and asks for a new review.
    add_draft(&env, "pay", "e", &[], true);
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    edit_goal(&env, "pay", "e", "Changed.");
    let out = env.cpd(&["plan", "decide", "r4", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&out["current"], &out["next"]), (&json!(false), &json!("cpd plan review")));
    assert!(out.get("jobs").is_none(), "{out}");
    assert!(events(&env, "plan-review").last().unwrap()["data"].get("jobs").is_none());
    assert_eq!(job_status(&env, "e"), "draft");
}

// ---------- plan lock ----------

/// A Plan `pay` in phase `plan` with an approved, current review.
fn approved(env: &TestEnv, repo: &Path) {
    planned(env, repo, "pay");
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"]).ok();
}

fn watch_calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd").into_iter().filter(|c| c[0] == "watch").collect()
}

#[test]
fn lock_creates_the_lead_worktree_queues_the_drafts_and_starts_the_engine() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    let head = env.git(&repo, &["rev-parse", "HEAD"]);

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    let worktree = repo.parent().unwrap().canonicalize().unwrap().join("pay.pay.pay");
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "dispatch", "jobs": ["a", "b"],
               "prs": [{"key": "pay", "status": "dispatch", "needs": [], "lead_branch": "cpd/pay/pay", "lead_worktree": worktree.display().to_string(),
                        "base_sha": head, "reused": false, "pushed": false}],
               "watch": {"started": true, "pane": "%9"}})
    );
    assert_eq!(env.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay/pay");
    assert_eq!(watch_calls(&env), vec![vec!["watch".to_owned(), "--ensure".to_owned()]]);

    let shown = show(&env, "pay");
    assert_eq!((&shown["plan"]["phase"], &shown["prs"][0]["lead_worktree"]), (&json!("dispatch"), &json!(worktree.display().to_string())));
    assert!(shown["plan"]["locked_at"].is_string());
    for job in shown["jobs"].as_array().unwrap() {
        assert_eq!(job["status"], "queued");
        let needs: Vec<String> = job["needs"].as_array().unwrap().iter().map(|n| format!("\"{}\"", n.as_str().unwrap())).collect();
        let key = job["key"].as_str().unwrap();
        let canonical = format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"needs\":[{}],\"out_of_scope\":[],\"tier\":\"S2\",\"touches\":[\"src/{key}.rs\"]}}",
            needs.join(",")
        );
        assert_eq!(job["fingerprint"], sha256(&canonical), "the Locked-Job fingerprint of {key}");
    }
    assert_eq!(shown["tamper"], json!([]));
    let phase = events(&env, "plan-phase").last().unwrap().clone();
    assert_eq!(phase["data"], json!({"from": "plan", "to": "dispatch", "jobs": ["a", "b"], "prs": ["pay"]}));
    let started = events(&env, "pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!((&started[0]["data"]["pr"], &started[0]["data"]["by"]), (&json!("pay"), &json!("lock")));

    // Already: still makes sure the engine runs.
    let again = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    assert_eq!((&again["already"], &again["phase"], &again["watch"]["started"]), (&json!(true), &json!("dispatch"), &json!(true)));
    assert_eq!(watch_calls(&env).len(), 2);
    assert_eq!(events(&env, "plan-phase").len(), 2);
}

// ---------- plan lock and the remote Lead Branch (R35) ----------

/// A bare `origin` for `repo`, holding `main`; returns its path.
fn with_origin(env: &TestEnv, repo: &Path) -> PathBuf {
    let origin = env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.git(repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

/// Commits an empty commit on `main` of `repo` and pushes it.
fn push_main_commit(env: &TestEnv, repo: &Path, message: &str) -> String {
    env.git(repo, &["commit", "-q", "--allow-empty", "-m", message]);
    env.git(repo, &["push", "-q", "origin", "main"]);
    env.git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn lock_creates_and_pushes_the_lead_branch_when_the_remote_has_none() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let origin = with_origin(&env, &repo);
    approved(&env, &repo);
    let head = env.git(&repo, &["rev-parse", "HEAD"]);

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!((&out["prs"][0]["base_sha"], &out["prs"][0]["pushed"], &out["prs"][0]["reused"]), (&json!(head), &json!(true), &json!(false)));
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay/pay"]), format!("{head}\trefs/heads/cpd/pay/pay"));
    let worktree = repo.parent().unwrap().canonicalize().unwrap().join("pay.pay.pay");
    assert_eq!(env.git(&worktree, &["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/cpd/pay/pay");
    assert_eq!(env.git(&origin, &["rev-parse", "cpd/pay/pay"]), head);
    assert!(events(&env, "error").is_empty());
}

#[test]
fn lock_reuses_a_remote_lead_branch_that_contains_the_base() {
    let env = TestEnv::new();
    let repo = setup(&env);
    with_origin(&env, &repo);
    approved(&env, &repo);
    // A commit only the remote has, on top of the base.
    env.git(&repo, &["commit", "-q", "--allow-empty", "-m", "only on the remote"]);
    let remote_tip = env.git(&repo, &["rev-parse", "HEAD"]);
    env.git(&repo, &["push", "-q", "origin", "HEAD:refs/heads/cpd/pay/pay"]);
    env.git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay/pay"]), "");

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    let worktree = repo.parent().unwrap().canonicalize().unwrap().join("pay.pay.pay");
    assert_eq!((&out["prs"][0]["base_sha"], &out["prs"][0]["reused"], &out["prs"][0]["pushed"]), (&json!(remote_tip), &json!(true), &json!(false)));
    assert_eq!(env.git(&worktree, &["rev-parse", "HEAD"]), remote_tip);
    assert_eq!(env.git(&repo, &["rev-parse", "refs/heads/cpd/pay/pay"]), remote_tip);
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay/pay"]), format!("{remote_tip}\trefs/heads/cpd/pay/pay"));
    assert!(events(&env, "error").is_empty());
}

#[test]
fn lock_refuses_a_remote_lead_branch_that_does_not_contain_the_base() {
    let env = TestEnv::new();
    let repo = setup(&env);
    with_origin(&env, &repo);
    approved(&env, &repo);
    // cpd/pay/pay at the old main; then main moves on, so origin/main is not its ancestor.
    env.git(&repo, &["push", "-q", "origin", "main:refs/heads/cpd/pay/pay"]);
    let remote_branch = env.git(&repo, &["rev-parse", "HEAD"]);
    push_main_commit(&env, &repo, "main moves on");

    let error = refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "branch_exists", 3);

    assert_eq!((&error["details"]["branch"], &error["details"]["pr"]), (&json!("cpd/pay/pay"), &json!("pay")));
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay/pay"]), "");
    assert!(!repo.parent().unwrap().join("pay.pay.pay").exists());
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay/pay"]), format!("{remote_branch}\trefs/heads/cpd/pay/pay"));
    assert_eq!(show(&env, "pay")["plan"]["phase"], "plan");
    assert!(watch_calls(&env).is_empty());
}

#[test]
fn lock_without_an_origin_remote_is_as_before() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    assert_eq!(env.git(&repo, &["remote"]), "");
    assert_eq!(show(&env, "pay")["plan"]["phase"], "dispatch");
    assert!(events(&env, "error").is_empty());
}

#[test]
fn a_push_failure_at_lock_still_locks_and_appends_one_push_failed_error() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let origin = with_origin(&env, &repo);
    let hook = origin.join("hooks").join("pre-receive");
    write_executable(&hook, "#!/bin/sh\necho refused by hook >&2\nexit 1\n");
    approved(&env, &repo);

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!(out["phase"], "dispatch");
    assert_eq!(show(&env, "pay")["plan"]["phase"], "dispatch");
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay/pay"]), "");
    let errors = events(&env, "error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["data"]["code"], "push_failed");
    assert_eq!(errors[0]["data"]["source"], "plan lock");
    assert!(errors[0]["data"]["details"]["stderr"].as_str().unwrap().contains("refused by hook"));
}

#[test]
fn lock_stays_locked_when_the_engine_cannot_start() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    env.fake_cpd_panes(WATCH_FAILS);
    let error = refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "watch_start_failed", 1);
    assert_eq!(error["details"]["next"], "cpd watch --ensure");
    assert_eq!(error["details"]["error"]["message"], "No lock.");
    assert_eq!(show(&env, "pay")["plan"]["phase"], "dispatch", "the Plan stays locked");

    // An engine that prints nothing usable fails the same way.
    env.fake_cpd_panes("echo nonsense; exit 0");
    let error = refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "watch_start_failed", 1);
    assert_eq!(error["details"]["error"]["stdout"], "nonsense");
}

#[test]
fn lock_refusals_change_nothing() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let lock = || env.cpd(&["plan", "lock", "--plan", "pay"]);
    let error = refused(&lock(), "illegal_transition", 3);
    assert_eq!(error["details"]["from"], "grounding");
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    refused(&lock(), "no_draft_jobs", 3);

    add_job(&env, "pay", "b", &[]);
    add_job(&env, "pay", "a", &["b"]);
    let error = refused(&lock(), "plan_not_approved", 3);
    assert_eq!(error["details"]["review"], Value::Null);
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    refused(&lock(), "plan_not_approved", 3);
    env.cpd(&["plan", "decide", "r1", "--reject", "--plan", "pay"]).ok();
    let error = refused(&lock(), "plan_not_approved", 3);
    assert_eq!(error["details"]["review"]["status"], "rejected");
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    edit_goal(&env, "pay", "a", "Changed after approval.");
    refused(&lock(), "plan_not_approved", 3);

    // A cycle (b needs a, a needs b), approved.
    store(&env).write("fixture", |tx| exec(tx, "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'b', 'a');")).unwrap();
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r3", "--approve", "--plan", "pay"]).ok();
    let error = refused(&lock(), "dependency_cycle", 3);
    assert_eq!(error["details"]["cycle"].as_array().unwrap().len(), 3);
    store(&env).write("fixture", |tx| exec(tx, "DELETE FROM dependencies WHERE plan = 'pay' AND job = 'b';")).unwrap();
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r4", "--approve", "--plan", "pay"]).ok();

    // Something at the Lead worktree's path, then the branch.
    let path = repo.parent().unwrap().join("pay.pay.pay");
    fs::create_dir_all(&path).unwrap();
    refused(&lock(), "worktree_exists", 3);
    fs::remove_dir(&path).unwrap();
    env.git(&repo, &["branch", "cpd/pay/pay"]);
    refused(&lock(), "branch_exists", 3);
    env.git(&repo, &["branch", "-D", "cpd/pay/pay"]);

    // pre-start fails: the worktree and branch are removed.
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(repo.join(".config/wt.toml"), "pre-start = \"echo setting up; exit 3\"\n").unwrap();
    env.git(&repo, &["add", "-A"]);
    env.git(&repo, &["commit", "-q", "-m", "setup"]);
    let error = refused(&lock(), "setup_failed", 1);
    assert_eq!(error["details"]["log"], home(&env).root().join("run/pay/logs/lead-pay.pre-start.log").display().to_string());
    assert_eq!((&error["details"]["pr"], &error["details"]["step"]), (&json!("pay"), &json!("pre-start")));
    assert!(!path.exists());
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay/pay"]), "");

    assert_eq!(show(&env, "pay")["plan"]["phase"], "plan");
    assert_eq!(show(&env, "pay")["counts"]["draft"], 2);
    assert!(watch_calls(&env).is_empty());
    refused(&env.cpd(&["plan", "lock"]), "plan_required", 2);
}

// ---- plan rename ----------------------------------------------------------

#[test]
fn rename_sets_the_title_and_appends_one_event() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let out = env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Retry payouts"]).ok();
    assert_eq!(out["plan"], json!({"id": "pay", "context": "ctx", "title": "Retry payouts"}));
    assert!(out.get("already").is_none());
    assert_eq!(show(&env, "pay")["plan"]["title"], "Retry payouts");
    let renamed = events(&env, "plan-renamed");
    assert_eq!(renamed.len(), 1);
    assert_eq!((renamed[0]["context"].as_str(), renamed[0]["plan"].as_str()), (Some("ctx"), Some("pay")));
    assert_eq!(renamed[0]["data"], json!({"title": "Retry payouts"}));

    // CPD_PLAN selects the Plan; the same title is already and writes nothing.
    let again = cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "rename", "--title", "Retry payouts"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(events(&env, "plan-renamed").len(), 1);
}

/// `@cpd_title` of `pane`.
fn pane_title(env: &TestEnv, pane: &str) -> String {
    let out = env.tmux(&["show", "-pqv", "-t", pane, "@cpd_title"]);
    String::from_utf8(out.stdout).unwrap().trim_end_matches('\n').to_owned()
}

#[test]
fn the_leads_pane_carries_the_plans_title_and_plan_rename_keeps_it_current() {
    let env = TestEnv::new();
    setup(&env);
    let out = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Retry payouts"]).ok();
    let (lead, pane) = (out["lead"]["session"].as_str().unwrap().to_owned(), out["lead"]["pane"].as_str().unwrap().to_owned());
    assert_eq!(pane_title(&env, &pane), "Retry payouts");

    env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Retry the payouts"]).ok();
    assert_eq!(pane_title(&env, &pane), "Retry the payouts");

    // A side fork shows its own name; renaming the Plan leaves it alone.
    let fork = env.cpd(&["session", "fork", &lead, "--name", "side"]).ok()["session"].clone();
    let fork_pane = fork["pane"].as_str().unwrap();
    assert_eq!(pane_title(&env, fork_pane), "side");
    env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Third"]).ok();
    assert_eq!((pane_title(&env, &pane), pane_title(&env, fork_pane)), ("Third".to_owned(), "side".to_owned()));

    // Renaming the Plan's Lead Session never replaces the Plan's title.
    env.cpd(&["session", "rename", "lead name", "--session", &lead]).ok();
    assert_eq!(pane_title(&env, &pane), "Third");

    // A rerun with the same title repairs a header that was cleared.
    env.tmux(&["set", "-p", "-u", "-t", &pane, "@cpd_title"]);
    assert_eq!(pane_title(&env, &pane), "");
    let again = env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Third"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(pane_title(&env, &pane), "Third");

    // No pane, nothing to do.
    env.tmux(&["kill-pane", "-t", &pane]);
    env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Fourth"]).ok();
}

#[test]
fn a_leads_dynamic_title_survives_plan_rename_and_session_rename() {
    let env = TestEnv::new();
    setup(&env);
    let out = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Retry payouts"]).ok();
    let (lead, pane) = (out["lead"]["session"].as_str().unwrap().to_owned(), out["lead"]["pane"].as_str().unwrap().to_owned());
    // Untitled: the header follows the rename (the test above). Titled: it keeps the title.
    store(&env)
        .write("fixture", |tx| exec(tx, &format!("UPDATE sessions SET title = 'Dynamic subject', titled_at = 't' WHERE id = '{lead}';")))
        .unwrap();
    env.tmux(&["set", "-p", "-t", &pane, "@cpd_title", "Dynamic subject"]);

    env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Retry the payouts"]).ok();
    assert_eq!(pane_title(&env, &pane), "Dynamic subject");
    assert_eq!(query::<String>(&env, "SELECT title FROM plans WHERE id = 'pay'"), "Retry the payouts");
    assert_eq!(events(&env, "plan-renamed").len(), 1);

    env.cpd(&["session", "rename", "lead name", "--session", &lead]).ok();
    assert_eq!(pane_title(&env, &pane), "Dynamic subject");

    // A rerun with the same title repairs a cleared header to the dynamic title.
    env.tmux(&["set", "-p", "-u", "-t", &pane, "@cpd_title"]);
    let again = env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Retry the payouts"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(pane_title(&env, &pane), "Dynamic subject");
}

#[test]
fn rename_refusals() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    refused(&env.cpd(&["plan", "rename", "--plan", "nope", "--title", "X"]), "plan_not_found", 4);
    refused(&env.cpd(&["plan", "rename", "--title", "X"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "rename", "--plan", "pay", "--title", ""]), "usage", 2);
    assert!(events(&env, "plan-renamed").is_empty());
}

#[test]
fn rename_works_on_an_archived_plan() {
    let env = TestEnv::new();
    setup(&env);
    new_plan_only(&env, "pay");
    env.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    let out = env.cpd(&["plan", "rename", "--plan", "pay", "--title", "Archived and renamed"]).ok();
    assert_eq!(out["plan"]["title"], "Archived and renamed");
    let plan = show(&env, "pay");
    assert_eq!(plan["plan"]["title"], "Archived and renamed");
    assert_ne!(plan["plan"]["archived_at"], Value::Null, "still archived");
    assert_eq!(events(&env, "plan-renamed").len(), 1);
}

// ---- plan archive and unarchive -------------------------------------------

/// Plan `pay` with its Lead and one side fork, each with a live parked pane.
fn plan_with_fork(env: &TestEnv) -> (String, String) {
    let repo = setup(env);
    let lead = new_plan(env, "pay", &repo)["lead"]["session"].as_str().unwrap().to_owned();
    pane_env(env, &lead);
    let fork = env.cpd(&["session", "fork", &lead]).ok()["session"]["id"].as_str().unwrap().to_owned();
    pane_env(env, &fork);
    (lead, fork)
}

fn has_pane(env: &TestEnv, session: &str) -> bool {
    panes(env).iter().any(|p| p.2 == session)
}

fn archived_ats(env: &TestEnv) -> i64 {
    query(
        env,
        "SELECT count(*) FROM (SELECT archived_at FROM contexts UNION ALL SELECT archived_at FROM plans
                              UNION ALL SELECT archived_at FROM sessions) WHERE archived_at IS NOT NULL",
    )
}

fn event_count(env: &TestEnv) -> i64 {
    query(env, "SELECT count(*) FROM events")
}

#[test]
fn archive_covers_the_plan_its_lead_and_forks_kills_their_panes_and_appends_one_event_each() {
    let env = TestEnv::new();
    let (lead, fork) = plan_with_fork(&env);
    let (lead_pane, fork_pane) = (
        panes(&env).into_iter().find(|p| p.2 == lead).unwrap().0,
        panes(&env).into_iter().find(|p| p.2 == fork).unwrap().0,
    );
    let listed = env.cpd(&["plan", "list"]).ok();
    assert_eq!(listed["plans"][0]["archived_at"], Value::Null);
    assert_eq!(show(&env, "pay")["plan"]["archived_at"], Value::Null);

    let out = env.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    let mut sessions: Vec<&str> = out["archived"].as_array().unwrap()[1..].iter().map(|o| o["id"].as_str().unwrap()).collect();
    sessions.sort();
    let mut expected = vec![lead.as_str(), fork.as_str()];
    expected.sort();
    assert_eq!(sessions, expected);
    assert!(out["archived"].as_array().unwrap()[1..].iter().all(|o| o["kind"] == "session"), "{out}");
    let mut closed: Vec<&str> = out["closed"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
    closed.sort();
    let mut panes_closed = vec![lead_pane.as_str(), fork_pane.as_str()];
    panes_closed.sort();
    assert_eq!(closed, panes_closed);
    assert!(!has_pane(&env, &lead) && !has_pane(&env, &fork));

    assert_eq!(archived_ats(&env), 3);
    let plan_events = events(&env, "plan-archived");
    assert_eq!(plan_events.len(), 1);
    assert_eq!((plan_events[0]["plan"].as_str(), &plan_events[0]["data"]), (Some("pay"), &json!({"archived": true})));
    let session_events = events(&env, "session-archived");
    assert_eq!(session_events.len(), 2);
    assert!(session_events.iter().all(|e| e["data"] == json!({"archived": true}) && e["plan"] == "pay"));
    let at = show(&env, "pay")["plan"]["archived_at"].clone();
    assert!(at.is_string());
    assert_eq!(env.cpd(&["plan", "list"]).ok()["plans"][0]["archived_at"], at);
    assert_eq!(query::<String>(&env, &format!("SELECT state FROM sessions WHERE id = '{lead}'")), "stopped");

    // Again: nothing is written.
    let before = event_count(&env);
    let again = env.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(again, json!({"ok": true, "archived": [], "closed": [], "already": true}));
    assert_eq!(event_count(&env), before);
}

#[test]
fn unarchive_clears_the_cascade_with_one_event_each_and_touches_no_pane() {
    let env = TestEnv::new();
    let (lead, fork) = plan_with_fork(&env);
    env.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    let events_before = event_count(&env);

    let out = env.cpd(&["plan", "unarchive", "--plan", "pay"]).ok();
    assert_eq!(out["archived"].as_array().unwrap().len(), 3, "{out}");
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert_eq!(out["closed"], json!([]));
    assert_eq!(archived_ats(&env), 0);
    assert_eq!(event_count(&env), events_before + 3);
    assert!(events(&env, "plan-archived")[1]["data"] == json!({"archived": false}));
    assert!(!has_pane(&env, &lead) && !has_pane(&env, &fork), "no pane is started");
    assert_eq!(show(&env, "pay")["plan"]["archived_at"], Value::Null);

    let again = env.cpd(&["plan", "unarchive", "--plan", "pay"]).ok();
    assert_eq!(again, json!({"ok": true, "archived": [], "closed": [], "already": true}));
    assert_eq!(event_count(&env), events_before + 3);
}

#[test]
fn unarchive_inside_an_archived_context_is_refused() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    env.cpd(&["context", "archive", "ctx"]).ok();
    let events_before = event_count(&env);
    let archived = archived_ats(&env);
    let error = refused(&env.cpd(&["plan", "unarchive", "--plan", "pay"]), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");
    assert_eq!((event_count(&env), archived_ats(&env)), (events_before, archived));
    refused(&env.cpd(&["plan", "unarchive", "--plan", "nope"]), "plan_not_found", 4);
    refused(&env.cpd(&["plan", "archive", "--plan", "nope"]), "plan_not_found", 4);
    refused(&env.cpd(&["plan", "archive"]), "plan_required", 2);
}

#[test]
fn plan_new_in_an_archived_context_is_refused_before_any_write() {
    let env = TestEnv::new();
    setup(&env);
    env.cpd(&["context", "archive", "ctx"]).ok();
    let events_before = event_count(&env);
    let args = ["plan", "new", "pay", "--context", "ctx", "--title", "Pay"];
    let error = refused(&env.cpd(&args), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans"), 0);
    assert_eq!(event_count(&env), events_before);
    assert!(panes(&env).is_empty());

    env.cpd(&["context", "unarchive", "ctx"]).ok();
    env.cpd(&args).ok();
}

const JOB: &str = "INSERT INTO jobs (plan, key, pr, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'pay', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '$STATUS', 0, '$FP', 1, 't');";
const ATTEMPT: &str = "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', '$STATUS', 'cpd/pay--api-a1', '/w', 't');";

/// Job `api` of `pay`; a locked one (not `draft`) has its fingerprint.
fn job(status: &str) -> String {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "a".repeat(64)) };
    JOB.replace("'$FP'", &fingerprint).replace("$STATUS", status)
}

#[test]
fn archive_refused_by_any_one_blocker_changes_nothing() {
    let review = format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('pay', 'r1', 1, '{}', 'pending', 't');",
        "a".repeat(64)
    );
    let escalation = format!(
        "{}{}INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
         VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Which?', 'open', 1, 't');",
        job("draft"),
        ATTEMPT.replace("$STATUS", "running")
    );
    // One blocker at a time, each in its own environment: Jobs are append-only.
    for (kind, apply) in [("job", job("queued")), ("job", job("running")), ("escalation", escalation), ("review", review), ("session", String::new())] {
        let env = TestEnv::new();
        let (_, fork) = plan_with_fork(&env);
        let apply = if kind == "session" { format!("UPDATE sessions SET state = 'working' WHERE id = '{fork}'") } else { apply };
        store(&env).write("fixture", |tx| exec(tx, &apply)).unwrap();
        let (events_before, panes_before) = (event_count(&env), panes(&env));
        let error = refused(&env.cpd(&["plan", "archive", "--plan", "pay"]), "archive_blocked", 3);
        let listed = error["details"]["blockers"].as_array().unwrap();
        assert_eq!(listed.len(), 1, "{kind}: {error}");
        assert_eq!(listed[0]["kind"], kind, "{error}");
        assert_eq!(archived_ats(&env), 0, "{kind}");
        assert_eq!(event_count(&env), events_before, "{kind}");
        assert_eq!(panes(&env), panes_before, "{kind}");
    }
}

#[test]
fn archive_lists_every_blocker() {
    let env = TestEnv::new();
    plan_with_fork(&env);
    store(&env).write("fixture", |tx| exec(tx, &job("queued"))).unwrap();
    store(&env)
        .write("fixture", |tx| {
            exec(tx, &format!("INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('pay', 'r1', 1, '{}', 'pending', 't');", "a".repeat(64)))
        })
        .unwrap();
    let error = refused(&env.cpd(&["plan", "archive", "--plan", "pay"]), "archive_blocked", 3);
    let kinds: Vec<&str> = error["details"]["blockers"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["job", "review"]);
}

#[test]
fn a_plan_in_phase_pr_with_no_live_job_archives() {
    let env = TestEnv::new();
    let (lead, _) = plan_with_fork(&env);
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    store(&env)
        .write("fixture", |tx| {
            exec(tx, "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay'")?;
            exec(tx, "UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay'")?;
            exec(
                tx,
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                 VALUES ('acme/pay', 1, 'https://github.com/acme/pay/pull/1', 'Pay', 0, 'open', 'none', 'none', 't', 't')",
            )?;
            exec(
                tx,
                "UPDATE prs SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/pay/pull/1', pr_opened_at = 't',
                        pull_request = (SELECT id FROM pull_requests WHERE number = 1) WHERE plan = 'pay'",
            )?;
            exec(tx, "UPDATE plans SET phase = 'deploy' WHERE id = 'pay'")
        })
        .unwrap();
    let out = env.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert!(!has_pane(&env, &lead));
    assert_eq!(show(&env, "pay")["plan"]["phase"], "deploy");
    assert!(show(&env, "pay")["plan"]["archived_at"].is_string());
}

#[test]
fn archive_refuses_a_lead_in_the_chat_slot_before_closing_any_pane() {
    let env = TestEnv::new();
    let (lead, fork) = plan_with_fork(&env);
    // The chat slot: the pane of window `main` running a Lead (the fork).
    let fork_pane = panes(&env).into_iter().find(|p| p.2 == fork).unwrap().0;
    env.tmux(&["rename-window", "-t", &fork_pane, "main"]);
    let (events_before, panes_before) = (event_count(&env), panes(&env));
    let error = refused(&env.cpd(&["plan", "archive", "--plan", "pay"]), "in_chat_slot", 3);
    assert_eq!(error["details"]["next"], "cpd session park");
    assert!(has_pane(&env, &lead));
    assert_eq!((event_count(&env), panes(&env), archived_ats(&env)), (events_before, panes_before, 0));
}

// ---------- stub drafts (hs-d241, R1; commands.md §4.5, §4.6, §4.8, §5.1, §5.2) ----------

/// What `job edit --tier` checks: the `[tiers]` of the config and a fake pi install whose Tier catalog
/// is cached, as in `cli/tests/job.rs`, so no node runs.
fn seed_tiers(env: &TestEnv) {
    let config_path = env.cpd_home().join("config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, format!("{config}[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n")).unwrap();
    env.fake_pi("exit 0");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let rows: String = [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")]
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    store(env).write("fixture", |tx| exec(tx, &rows)).unwrap();
}

/// A Spec file for `job edit --spec`.
fn spec_file(env: &TestEnv, name: &str) -> String {
    let path = env.root().join(format!("{name}.json"));
    let spec = json!({"goal": format!("Goal of {name}."), "context": "Why.", "touches": [format!("src/{name}.rs")], "acceptance": ["cargo test"], "out_of_scope": []});
    fs::write(&path, spec.to_string()).unwrap();
    path.to_str().unwrap().to_owned()
}

/// `job add <key> --draft --title <t>` in Plan `pay`, a stub, with `extra` flags.
fn stub(env: &TestEnv, key: &str, extra: &[&str]) -> Value {
    let title = format!("Planned {key}");
    let mut args = vec!["job", "add", key, "--draft", "--title", &title, "--plan", "pay"];
    args.extend(extra);
    env.cpd(&args).ok()
}

#[test]
fn a_stub_added_in_dispatch_refuses_the_review_until_it_is_completed_then_is_approved_and_queued() {
    let env = TestEnv::new();
    let repo = setup(&env);
    seed_tiers(&env);
    approved(&env, &repo);
    env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    // A follow-up batch is registered at once: two stubs, one with its Tier, and a stub that is dropped again.
    let out = stub(&env, "next-1", &[]);
    assert_eq!((&out["job"]["status"], &out["job"]["tier"], &out["job"]["appended"]), (&json!("draft"), &Value::Null, &json!(true)));
    stub(&env, "next-2", &["--tier", "O3", "--needs", "next-1"]);
    stub(&env, "dropped", &[]);
    env.cpd(&["job", "cancel", "dropped", "--reason", "Not needed.", "--plan", "pay"]).ok();
    add_draft(&env, "pay", "done-1", &["a"], true);
    let shown = show(&env, "pay");
    let listed = shown["jobs"].as_array().unwrap().iter().find(|j| j["key"] == "next-1").unwrap();
    assert_eq!((&listed["status"], &listed["tier"], &listed["goal"], &listed["touches"]), (&json!("draft"), &Value::Null, &Value::Null, &Value::Null));
    assert_eq!(shown["counts"]["draft"], 3);
    // The Plan view of the Context holds the stub's registered work too.
    let pending = env.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let jobs = &pending["plans"][0]["prs"][0]["jobs"];
    let pending_stub = jobs.as_array().unwrap().iter().find(|j| j["key"] == "next-1").unwrap();
    assert_eq!((&pending_stub["status"], &pending_stub["touches"]), (&json!("draft"), &json!([])));

    // The review is refused, listing each stub and what it lacks (the cancelled one is not a draft); nothing is written.
    let (reviews, evs) = (events(&env, "plan-review").len(), query::<i64>(&env, "SELECT count(*) FROM events"));
    let error = refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "draft_incomplete", 3);
    assert_eq!(error["details"]["jobs"], json!([{"key": "next-1", "missing": ["tier", "spec"]}, {"key": "next-2", "missing": ["spec"]}]));
    assert_eq!(error["details"]["plan"], "pay");
    assert!(error["message"].as_str().unwrap().contains("next-1, next-2"), "{error}");
    assert_eq!((events(&env, "plan-review").len(), query::<i64>(&env, "SELECT count(*) FROM events")), (reviews, evs));
    assert!(!home(&env).plan_review_md(&pid("pay"), ReviewId::parse("r2").unwrap()).exists());
    // `plan render` still shows them, and the fingerprint covers them as they are.
    let before = env.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"].clone();

    // Completing one is not enough; completing both lets the review through, with a new fingerprint.
    let spec_1 = spec_file(&env, "next-1");
    let out = env.cpd(&["job", "edit", "next-1", "--tier", "S2", "--spec", &spec_1, "--plan", "pay"]).ok();
    assert_eq!(out["fields"], json!(["tier", "spec"]));
    let error = refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "draft_incomplete", 3);
    assert_eq!(error["details"]["jobs"], json!([{"key": "next-2", "missing": ["spec"]}]));
    let spec_2 = spec_file(&env, "next-2");
    env.cpd(&["job", "edit", "next-2", "--spec", &spec_2, "--plan", "pay"]).ok();
    let out = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    assert_eq!((&out["review"]["id"], &out["review"]["status"]), (&json!("r2"), &json!("pending")));
    assert_ne!(out["review"]["fingerprint"], before, "completing a stub changes the plan fingerprint");

    // Approved and current: exactly the complete drafts are queued, each with its Locked-Job fingerprint.
    let out = env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&out["current"], &out["jobs"]), (&json!(true), &json!(["done-1", "next-1", "next-2"])));
    let shown = show(&env, "pay");
    for key in ["done-1", "next-1", "next-2"] {
        assert_eq!(job_status(&env, key), "queued", "{key}");
    }
    assert_eq!(job_status(&env, "dropped"), "cancelled");
    assert_eq!(shown["tamper"], json!([]));
    assert_eq!(shown["counts"]["draft"], 0);
}

#[test]
fn a_stub_in_phase_plan_refuses_the_review_and_the_lock_and_is_never_queued() {
    let env = TestEnv::new();
    let repo = setup(&env);
    seed_tiers(&env);
    approved(&env, &repo);
    stub(&env, "later", &[]);
    stub(&env, "also", &["--tier", "S2"]);
    let lock = || env.cpd(&["plan", "lock", "--plan", "pay"]);

    // The approved review no longer covers the Jobs; the lock names the stubs before it names the review.
    let error = refused(&lock(), "draft_incomplete", 3);
    assert_eq!(error["details"]["jobs"], json!([{"key": "also", "missing": ["spec"]}, {"key": "later", "missing": ["tier", "spec"]}]));
    let error = refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "draft_incomplete", 3);
    assert_eq!(error["details"]["jobs"], json!([{"key": "also", "missing": ["spec"]}, {"key": "later", "missing": ["tier", "spec"]}]));
    assert_eq!(show(&env, "pay")["plan"]["phase"], "plan");
    assert_eq!(job_status(&env, "later"), "draft");
    assert!(watch_calls(&env).is_empty());

    // Cancelled, a stub is no draft and blocks nothing; the other is completed.
    env.cpd(&["job", "cancel", "later", "--reason", "No.", "--plan", "pay"]).ok();
    env.cpd(&["job", "edit", "also", "--spec", &spec_file(&env, "also"), "--plan", "pay"]).ok();
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    let out = lock().ok();
    assert_eq!(out["jobs"], json!(["a", "b", "also"]), "in creation order");
    assert_eq!(job_status(&env, "also"), "queued");
    assert_eq!(job_status(&env, "later"), "cancelled");
}

#[test]
fn render_shows_what_a_stub_lacks_and_the_fingerprint_covers_it_as_it_is() {
    let env = TestEnv::new();
    let repo = setup(&env);
    seed_tiers(&env);
    new_plan(&env, "pay", &repo);
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    stub(&env, "bare", &[]);
    stub(&env, "tiered", &["--tier", "O3"]);
    add_job(&env, "pay", "full", &[]);
    // Another Plan's stub in the same repo does not break the overlap check.
    new_plan(&env, "oth", &repo);
    env.cpd(&["plan", "phase", "plan", "--plan", "oth"]).ok();
    env.cpd(&["job", "add", "elsewhere", "--draft", "--title", "Elsewhere", "--plan", "oth"]).ok();

    let out = env.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_eq!((&out["draft_jobs"], &out["overlaps"]), (&json!(3), &json!([])));
    let text = fs::read_to_string(home(&env).plan_md(&pid("pay"))).unwrap();
    // The table row and the section show a missing Tier as `—`; the Spec of a stub is not written yet.
    assert!(text.contains("| `bare` | Planned bare | — | draft | none | — |"), "{text}");
    assert!(text.contains("| `tiered` | Planned tiered | O3 | draft | none | — |"), "{text}");
    assert!(text.contains("| `full` | Job full | S2 | draft | none | `src/full.rs` |"), "{text}");
    assert!(text.contains("\n### `bare`: Planned bare\n\nStatus: `draft` · Tier: — · Needs: none\n\nSpec: not written yet\n"), "{text}");
    assert!(text.contains("\n### `tiered`: Planned tiered\n\nStatus: `draft` · Tier: `O3` · Needs: none\n\nSpec: not written yet\n"), "{text}");
    assert!(text.contains("\n### `full`: Job full\n\nStatus: `draft` · Tier: `S2` · Needs: none\n\n#### Goal\n\nGoal of full.\n"), "{text}");
    assert_eq!(text.matches("Spec: not written yet").count(), 2, "{text}");

    // The fingerprint hashes a stub as it is: its missing fields are null.
    let (pr_repo, pr_base): (String, String) =
        store(&env).read(|c| Ok(c.query_row("SELECT repo, base FROM prs WHERE plan = 'pay'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    let null_spec = |key: &str, tier: &str| {
        format!("{{\"acceptance\":null,\"context\":null,\"goal\":null,\"key\":\"{key}\",\"needs\":[],\"out_of_scope\":null,\"pr\":\"pay\",\"tier\":{tier},\"title\":\"Planned {key}\",\"touches\":null}}")
    };
    let full = "{\"acceptance\":[\"cargo test\"],\"context\":\"Context of full.\",\"goal\":\"Goal of full.\",\"key\":\"full\",\"needs\":[],\"out_of_scope\":[],\"pr\":\"pay\",\"tier\":\"S2\",\"title\":\"Job full\",\"touches\":[\"src/full.rs\"]}";
    let prs = format!("[{{\"base\":\"{pr_base}\",\"key\":\"pay\",\"needs\":[],\"repo\":{}}}]", serde_json::to_string(&pr_repo).unwrap());
    assert_eq!(out["fingerprint"], sha256(&format!("{{\"jobs\":[{},{full},{}],\"prs\":{prs}}}", null_spec("bare", "null"), null_spec("tiered", "\"O3\""))));

    // Completing a stub changes it, and the render shows the Tier and the Spec.
    env.cpd(&["job", "edit", "bare", "--tier", "S2", "--spec", &spec_file(&env, "bare"), "--plan", "pay"]).ok();
    let completed = env.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_ne!(completed["fingerprint"], out["fingerprint"]);
    let text = fs::read_to_string(home(&env).plan_md(&pid("pay"))).unwrap();
    assert!(text.contains("| `bare` | Planned bare | S2 | draft | none | `src/bare.rs` |"), "{text}");
    assert_eq!(text.matches("Spec: not written yet").count(), 1, "{text}");
}
