//! `cpd review` (hs-d496): the plannotator sessions a Session has open, kept in `plannotator_reviews`. The pi adapter's
//! tracker opens and closes the rows; E8's `review_waiting` turns an open row into a magenta `review-waiting` Inbox item.
//! Both writes are idempotent and append one Event each only when they change something.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;

const KINDS: [&str; 4] = ["annotate", "review", "last", "plan"];
const ENDED: [&str; 2] = ["decided", "lost"];
const SUBJECT_MAX: usize = 200;

/// The `review` commands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Record an open plannotator session
    Open(OpenArgs),
    /// Close one or every open plannotator session
    Close(CloseArgs),
    /// List the open plannotator sessions of a Session
    List(ListArgs),
}

#[derive(clap::Args, Debug)]
pub struct OpenArgs {
    #[arg(long)]
    pub session: String,
    #[arg(long)]
    pub pn: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub subject: String,
    #[arg(long)]
    pub port: u16,
}

#[derive(clap::Args, Debug)]
pub struct CloseArgs {
    #[arg(long)]
    pub session: String,
    #[arg(long, conflicts_with = "all")]
    pub pn: Option<String>,
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub ended: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    #[arg(long)]
    pub session: String,
}

fn usage(message: impl Into<String>, field: &str, value: &str) -> CpdError {
    CpdError::new(ErrorCode::Usage, message).with(field, value)
}

/// `pn-` and six lower-case hex digits.
fn check_pn(pn: &str) -> Result<(), CpdError> {
    let ok = pn.len() == 9 && pn.starts_with("pn-") && pn[3..].bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if ok { Ok(()) } else { Err(usage("A plannotator session id is `pn-` and six lower-case hex digits.", "pn", pn)) }
}

fn check_one_of(what: &str, value: &str, allowed: &[&str]) -> Result<(), CpdError> {
    if allowed.contains(&value) { Ok(()) } else { Err(usage(format!("{what} is one of {}.", allowed.join(", ")), what, value)) }
}

/// The scope of a Session: its Context and Plan.
struct Scope {
    session: SessionId,
    context: ContextId,
    plan: Option<PlanId>,
}

impl Scope {
    fn event(&self, event: NewEvent) -> NewEvent {
        let event = event.session(self.session.clone()).context(self.context.clone());
        match &self.plan {
            Some(plan) => event.plan(plan.clone()),
            None => event,
        }
    }
}

fn scope(conn: &Connection, session: &str) -> Result<Scope, CpdError> {
    let found: Option<(String, Option<String>)> = conn
        .query_row("SELECT context, plan FROM sessions WHERE id = ?1", [session], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let (context, plan) = found.ok_or_else(|| CpdError::new(ErrorCode::SessionNotFound, format!("No Session {session}.")).with("session", session))?;
    let corrupt = |what: &str, v: &str| CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid {what}: {v:?}."));
    Ok(Scope {
        session: SessionId::parse(session).map_err(|_| corrupt("Session id", session))?,
        context: ContextId::parse(&context).map_err(|_| corrupt("Context id", &context))?,
        plan: plan.map(|p| PlanId::parse(&p).map_err(|_| corrupt("Plan id", &p))).transpose()?,
    })
}

/// `review open`: records the session; a second open of the same `(session, pn)` changes nothing.
fn open(args: OpenArgs) -> Result<Output, CpdError> {
    check_pn(&args.pn)?;
    check_one_of("kind", &args.kind, &KINDS)?;
    let chars = args.subject.chars().count();
    if chars == 0 || chars > SUBJECT_MAX {
        return Err(usage(format!("A subject is 1 to {SUBJECT_MAX} characters."), "subject", &args.subject));
    }
    if args.port == 0 {
        return Err(usage("A port is 1 to 65535.", "port", "0"));
    }
    let mut env = Env::open()?;
    let already = env.store.write("review open", |tx| {
        let scope = scope(tx, &args.session)?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO plannotator_reviews (session, pn_id, kind, subject, port, opened_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![args.session, args.pn, args.kind, args.subject, args.port, clock::now()],
        )?;
        if inserted == 1 {
            let text = format!("Session {} opened plannotator session {} ({}): {}.", args.session, args.pn, args.kind, args.subject);
            append(tx, scope.event(NewEvent::review_opened(text, &args.pn, &args.kind, &args.subject, args.port)))?;
        }
        Ok(inserted == 0)
    })?;
    let mut out = json!({"session": args.session, "pn_id": args.pn});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `review close`: closes one open row or every open row of the Session; what is not open stays as it is.
fn close(args: CloseArgs) -> Result<Output, CpdError> {
    let which = match (&args.pn, args.all) {
        (Some(pn), false) => {
            check_pn(pn)?;
            Some(pn.clone())
        }
        (None, true) => None,
        _ => return Err(usage("Close one session with --pn, or every open one with --all.", "session", &args.session)),
    };
    let ended = args.ended.as_deref().unwrap_or("decided");
    check_one_of("ended", ended, &ENDED)?;
    let mut env = Env::open()?;
    let closed = env.store.write("review close", |tx| {
        let scope = scope(tx, &args.session)?;
        let mut stmt = tx.prepare(
            "SELECT pn_id FROM plannotator_reviews WHERE session = ?1 AND closed_at IS NULL AND (?2 IS NULL OR pn_id = ?2) ORDER BY opened_at, pn_id",
        )?;
        let open = stmt.query_map(rusqlite::params![args.session, which], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for pn in &open {
            tx.execute(
                "UPDATE plannotator_reviews SET closed_at = ?1, ended = ?2 WHERE session = ?3 AND pn_id = ?4",
                rusqlite::params![clock::now(), ended, args.session, pn],
            )?;
            let text = format!("Session {} closed plannotator session {pn} ({ended}).", args.session);
            append(tx, scope.event(NewEvent::review_closed(text, pn, ended)))?;
        }
        Ok(open)
    })?;
    Ok(Output::success(json!({"session": args.session, "closed": closed})))
}

/// `review list`: the open rows of the Session, oldest first.
fn list(args: ListArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let reviews = env.store.read(|conn| {
        scope(conn, &args.session)?;
        let mut stmt = conn.prepare(
            "SELECT pn_id, kind, subject, port, opened_at FROM plannotator_reviews WHERE session = ?1 AND closed_at IS NULL ORDER BY opened_at, pn_id",
        )?;
        let rows = stmt
            .query_map([&args.session], |r| {
                Ok(json!({"pn_id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "subject": r.get::<_, String>(2)?,
                          "port": r.get::<_, i64>(3)?, "opened_at": r.get::<_, String>(4)?}))
            })?
            .collect::<rusqlite::Result<Vec<Value>>>()?;
        Ok(rows)
    })?;
    Ok(Output::success(json!({"session": args.session, "reviews": reviews})))
}

/// Runs a `review` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Open(a) => open(a),
        Command::Close(a) => close(a),
        Command::List(a) => list(a),
    }
}
