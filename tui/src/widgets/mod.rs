//! The `widgets` view (commands.md §13.2, §17): the Dispatch column, the
//! machine-wide right column of the cockpit. It takes one snapshot of the read
//! model ([`read::open_prs`]: the one thing it draws), then follows the Events: it looks at the Events and at the pull
//! request rows every [`TICK`] and re-reads when one concerns it (a pull request row changes with no Event), at most
//! once per [`COALESCE`]; the load average is re-read every [`LOAD_EVERY`], since it is no Event. It draws when the
//! pull requests, the load, the note, the size, a click or the wheel changed, not on every tick. It makes no write of
//! its own, but for what a click on an owned PR row asks of `cpd session open`, and the merge popup a click on a merge
//! icon opens. It has no key; mouse capture is on, for the clicks and the wheel over the PRs section.
//!
//! - [`column`]: the drawing of the machine, the PRs, git and Jira;
//! - [`legend`]: the key legend, `cpd tui view legend`, which the tree's `?` and
//!   the StatusBar's button (`cpd tui action legend`) open in a tmux popup (the columns are narrower than the legend);
//! - the colours and icons are [`crate::palette`]'s.

mod column;
pub mod legend;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use ratatui::crossterm::event::{self, Event, MouseButton, MouseEventKind};
use ratatui::layout::Position;

use super::palette;
use super::read::{self, OpenPr};
use super::runner::{ViewError, kept};
use super::{Capture, RunCommand, run_view};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::Kind;
use cpd_core::home::CpdHome;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, View, chat_slot, view_pane};
use cpd_core::ids::{AttemptId, PlanId, SessionId};
use cpd_core::session::PROGRAM;
use cpd_core::output::Output;
use cpd_core::store::Store;

pub use column::{Drawn, Hit, Machine, MergeIcon, draw};
pub(crate) use column::{Window, dim, max_scroll, window};

/// How often the view looks at the Events and at keys.
const TICK: Duration = Duration::from_millis(200);
/// How often the view redraws while a shown PR's Lead is working: one frame of the spinner (`palette::frame`, 100 ms).
const FRAME: Duration = Duration::from_millis(100);
/// At most one snapshot per this long, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);
/// The load at least this often: the load average is no Event.
const LOAD_EVERY: Duration = Duration::from_secs(2);

pub(super) fn run(run_command: RunCommand) -> Result<Output, CpdError> {
    run_view(
        "widgets",
        Capture::Mouse,
        || {
            let home = CpdHome::from_env()?;
            let config = Config::load(&home)?;
            let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
            let machine = Machine { name: hostname(), load: load_average() };
            let (prs, cursor) = read::open_prs(&mut store)?;
            let follow = read::Follow::new(&mut store, KINDS, cursor)?;
            Ok((store, config, prs, machine, follow))
        },
        |terminal, (mut store, config, prs, machine, follow)| {
            let tmux = Tmux::new(config.tmux.socket.clone());
            let lead = Opener { run_command, tmux: &tmux, cockpit: &config.tmux.cockpit_session };
            show(terminal, &mut store, follow, prs, machine, &lead)
        },
    )
}

/// The Events that change what the column draws besides the pull request rows (whose changes have no Event): a pull
/// request gets its PR (`pr-opened`), a Plan its Lead (`plan-created`, `session-registered`), the chat slot moves.
const KINDS: &[Kind] = &[Kind::PrOpened, Kind::PlanCreated, Kind::SessionRegistered, Kind::SessionMoved];

/// What a click on an owned PR row runs: `session open <lead>` through `run_command`, then the focus on the chat slot.
struct Opener<'a> {
    run_command: RunCommand,
    tmux: &'a Tmux,
    cockpit: &'a str,
}

/// Draws and follows, for as long as the process runs: no key ends it. A failed read keeps what is on screen and shows its error in the last row, until a read
/// succeeds ([`kept`]). The wheel over the PRs section scrolls it by one pull request per notch. A left click on an owned
/// PR row (not on its `󰏌` or its merge icon) opens its Lead in the chat slot; a left click on a merge icon opens the merge
/// popup; a refusal or a failure is the `note` in the column's last row until the
/// next click.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    mut follow: read::Follow,
    mut prs: Vec<OpenPr>,
    mut machine: Machine,
    opener: &Opener,
) -> Result<(), ViewError> {
    let (mut read_at, mut load_at) = (Instant::now(), Instant::now());
    let mut last = None::<Drawn>;
    let mut note = String::new();
    let mut error = None::<String>;
    let mut redraw = true;
    let mut drawn_frame = None::<usize>;
    // The PRs section's offset, in pull requests: in memory only, kept by a refresh and clamped by the drawing.
    let mut scroll = 0usize;
    loop {
        // the spinner of a working Lead turns: a new frame redraws (commands.md §13.2)
        let frame = palette::frame(SystemTime::now());
        if redraw || (spins(&prs) && Some(frame) != drawn_frame) {
            let shown = error.as_deref().unwrap_or(&note);
            terminal.draw(|f| last = Some(draw(f, f.area(), &prs, &machine, shown, scroll, frame)))?;
            (redraw, drawn_frame) = (false, Some(frame));
        }
        let drawn = last.as_ref().expect("the terminal drew");
        scroll = scroll.min(drawn.max_scroll);
        if event::poll(tick(&prs))? {
            redraw = true; // a click, the wheel, a key or a resize
            match event::read()? {
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                    note.clear();
                    let at = Position::new(m.column, m.row);
                    let result = if let Some(icon) = drawn.icons.iter().find(|i| i.area.contains(at)) {
                        Some(crate::merge::open(opener.tmux, &icon.pull))
                    } else {
                        drawn.hits.iter().find(|h| h.contains(at)).map(|hit| open_lead(opener.run_command, opener.tmux, opener.cockpit, &hit.lead))
                    };
                    if let Some(Err(e)) = result {
                        note = e.message().to_owned();
                    }
                }
                Event::Mouse(m) if drawn.prs.contains(Position::new(m.column, m.row)) => match m.kind {
                    MouseEventKind::ScrollDown => scroll = (scroll + 1).min(drawn.max_scroll),
                    MouseEventKind::ScrollUp => scroll = scroll.saturating_sub(1),
                    _ => {}
                },
                _ => {}
            }
        }
        if read_at.elapsed() >= COALESCE {
            let (retry, shown) = (error.is_some(), error.clone());
            let read = (|| {
                let moved = follow.poll_changes(store)?.any();
                let prs = if moved || retry { Some(read::open_prs(store)?.0) } else { None };
                let load = if load_at.elapsed() >= LOAD_EVERY || retry { Some(load_average()) } else { None };
                Ok((prs, load))
            })();
            match kept(read, &mut error) {
                Some((next, load)) => {
                    if let Some(next) = next.filter(|next| *next != prs) {
                        prs = next;
                        redraw = true;
                    }
                    if let Some(load) = load {
                        load_at = Instant::now();
                        if load != machine.load {
                            machine.load = load;
                            redraw = true;
                        }
                    }
                    read_at = Instant::now();
                }
                None => read_at = Instant::now(), // retried after the next COALESCE
            }
            redraw |= error != shown;
        }
    }
}

/// How long the view waits for input before it draws again: one spinner frame while a PR's Lead works, else [`TICK`].
fn tick(prs: &[OpenPr]) -> Duration {
    if spins(prs) { FRAME } else { TICK }
}

/// A shown PR's Lead is working: its spinner turns.
fn spins(prs: &[OpenPr]) -> bool {
    prs.iter().any(|p| p.lead_working)
}

/// A click on a tab or on an owned PR row: `session open <id>`, which moves that Lead into the chat slot, then `select-pane`
/// on the chat. The plan view's tabs and this column's rows share it.
pub(crate) fn open_lead(run_command: RunCommand, tmux: &Tmux, cockpit: &str, session: &SessionId) -> Result<(), CpdError> {
    let argv = [PROGRAM, "session", "open", session.as_str()].map(std::ffi::OsString::from).into();
    run_command(argv)?;
    if let Some(chat) = chat_slot(&main_panes(tmux, cockpit)?, cockpit) {
        tmux.run(&["select-pane", "-t", chat.pane.as_str()].map(str::to_owned))?;
    }
    Ok(())
}

/// The panes of the cockpit's `main` window.
pub(crate) fn main_panes(tmux: &Tmux, cockpit: &str) -> Result<Vec<Pane>, CpdError> {
    let all = tmux.list_panes()?;
    Ok(all.into_iter().filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW).collect())
}

/// The pane option holding the Job a Worker pane shows (its border title shows it), so a second click on the same Job
/// closes it.
const OPT_JOB: &str = "@cpd_job";

/// The pane option holding the Attempt a Worker pane shows.
const OPT_ATTEMPT: &str = "@cpd_attempt";

/// The Job a Worker pane shows.
fn worker_shows(tmux: &Tmux, pane: &PaneId) -> Result<String, CpdError> {
    let shown = tmux.run(&["display", "-p", "-t", pane.as_str(), &format!("#{{{OPT_JOB}}}")].map(str::to_owned))?;
    Ok(shown.trim_end_matches('\n').to_owned())
}

/// `set -p` of the Worker pane's options: the Job and its current Attempt.
fn mark_worker(tmux: &Tmux, pane: &PaneId, job: &str, attempt: &str) -> Result<(), CpdError> {
    for (option, value) in [(OPT_JOB, job), (OPT_ATTEMPT, attempt)] {
        tmux.run(&["set", "-p", "-t", pane.as_str(), option, value].map(str::to_owned))?;
    }
    Ok(())
}

/// The Worker pane's program: `tui view worker <attempt> --plan <plan>`, in cpd's home.
fn worker_spawn(home: &CpdHome, plan: &PlanId, attempt: &str) -> Spawn {
    Spawn {
        cwd: Some(home.root().to_owned()),
        env: BTreeMap::from([("CPD_HOME".to_owned(), home.root().display().to_string())]),
        argv: [PROGRAM, "tui", "view", "worker", attempt, "--plan", plan.as_str()].map(str::to_owned).into(),
    }
}

/// Opens the read-only Worker pane beside the chat (`split-window -h -d -l 50%`, so the focus stays) for `job`'s
/// `attempt`, and retargets it (`respawn-pane -k`) when it already shows a Worker. The plan view's pills and the tree's
/// `job-stuck` click share it.
pub(crate) fn open_worker(
    home: &CpdHome,
    tmux: &Tmux,
    cockpit: &str,
    plan: &PlanId,
    job: &str,
    attempt: &AttemptId,
) -> Result<(), CpdError> {
    let panes = main_panes(tmux, cockpit)?;
    let attempt = attempt.to_string();
    if let Some(pane) = view_pane(&panes, cockpit, View::Worker) {
        let spawn = worker_spawn(home, plan, &attempt);
        let mut args = ["respawn-pane", "-k", "-t", pane.pane.as_str()].map(str::to_owned).to_vec();
        if let Some(cwd) = &spawn.cwd {
            args.extend(["-c".to_owned(), cwd.display().to_string()]);
        }
        for (key, value) in &spawn.env {
            args.extend(["-e".to_owned(), format!("{key}={value}")]);
        }
        args.push("--".to_owned());
        args.extend(spawn.argv);
        tmux.run(&args)?;
        return mark_worker(tmux, &pane.pane, job, &attempt);
    }
    let Some(chat) = chat_slot(&panes, cockpit) else {
        return Err(CpdError::new(ErrorCode::TmuxFailed, "There is no chat slot to open the Worker beside."));
    };
    let pane = tmux.split_window(&chat.pane, Split::Right, Some("50%"), &worker_spawn(home, plan, &attempt))?;
    tmux.set_pane_view(&pane, View::Worker)?;
    mark_worker(tmux, &pane, job, &attempt)
}

/// A click on a pill: [`open_worker`], or closes the Worker pane when it already shows `job`. Returns whether it is open now.
pub(crate) fn toggle_worker(
    home: &CpdHome,
    tmux: &Tmux,
    cockpit: &str,
    plan: &PlanId,
    job: &str,
    attempt: &AttemptId,
) -> Result<bool, CpdError> {
    let panes = main_panes(tmux, cockpit)?;
    if let Some(pane) = view_pane(&panes, cockpit, View::Worker) {
        if worker_shows(tmux, &pane.pane)? == job {
            tmux.kill_pane(&pane.pane)?;
            return Ok(false);
        }
    }
    open_worker(home, tmux, cockpit, plan, job, attempt)?;
    Ok(true)
}

/// Closes the Worker pane when it shows `job`; any other Worker pane stays.
pub(crate) fn close_worker(tmux: &Tmux, cockpit: &str, job: &str) -> Result<(), CpdError> {
    let panes = main_panes(tmux, cockpit)?;
    if let Some(pane) = view_pane(&panes, cockpit, View::Worker) {
        if worker_shows(tmux, &pane.pane)? == job {
            tmux.kill_pane(&pane.pane)?;
        }
    }
    Ok(())
}

/// What the column draws for a load it could not read.
const UNKNOWN: &str = "?";

/// The machine's name: the kernel's (`/proc/sys/kernel/hostname`), else `$HOSTNAME`, else what the `hostname` program
/// prints (macOS has no `/proc`), else `?`: the name is one decorative line, so no failure ends the view.
fn hostname() -> String {
    read_hostname(Path::new("/proc/sys/kernel/hostname"))
        .or_else(|| std::env::var("HOSTNAME").ok().map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()))
        .or_else(|| {
            let out = Command::new("hostname").output().ok().filter(|o| o.status.success())?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| UNKNOWN.to_owned())
}

/// The first line of the file at `path`, when it has one.
fn read_hostname(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().next()?.trim().to_owned()).filter(|n| !n.is_empty())
}

/// The first three fields of the load average: `/proc/loadavg` on Linux, `sysctl -n vm.loadavg` on macOS; `?` when
/// it cannot be read (the view keeps running and reads it again).
fn load_average() -> Vec<String> {
    let text = if cfg!(target_os = "macos") {
        Command::new("sysctl").args(["-n", "vm.loadavg"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        std::fs::read_to_string("/proc/loadavg").ok()
    };
    text.and_then(|text| parse_load(&text)).unwrap_or_else(|| vec![UNKNOWN.to_owned()])
}

/// The three numbers that open `text` (`16.89 24.21 25.00 6/3484 1221694`, or sysctl's `{ 0.52 0.58 0.59 }`).
fn parse_load(text: &str) -> Option<Vec<String>> {
    let fields: Vec<String> = text.split_whitespace().filter(|f| *f != "{").take(3).map(str::to_owned).collect();
    (fields.len() == 3 && fields.iter().all(|f| f.parse::<f64>().is_ok())).then_some(fields)
}

/// The legend in a tmux popup over the cockpit, sized to it and at most the client's size.
pub(crate) fn open_legend(tmux: &Tmux) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&legend::popup_args(client)).map(drop)
}

/// The size `(columns, rows)` of the most recently active client in `list-clients`'
/// `#{client_activity} #{client_width} #{client_height}` lines.
pub(crate) fn newest_client(listing: &str) -> Option<(u16, u16)> {
    listing
        .lines()
        .filter_map(|line| {
            let n: Vec<u64> = line.split(' ').filter_map(|x| x.parse().ok()).collect();
            Some((*n.first()?, u16::try_from(*n.get(1)?).ok()?, u16::try_from(*n.get(2)?).ok()?))
        })
        .max()
        .map(|(_, w, h)| (w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(working: bool) -> read::OpenPr {
        read::OpenPr {
            repo: "acme/repo".to_owned(),
            number: 1,
            url: String::new(),
            title: String::new(),
            draft: false,
            checks: read::Checks::Passed,
            review_decision: read::ReviewDecision::None,
            conflicting: false,
            clean: false,
            active: false,
            lead: Some(SessionId::parse("l-a").unwrap()),
            lead_working: working,
        }
    }

    #[test]
    fn the_view_redraws_every_spinner_frame_while_a_shown_leads_works_and_every_tick_otherwise() {
        assert_eq!(tick(&[]), TICK);
        assert_eq!(tick(&[pr(false), pr(false)]), TICK);
        assert_eq!(tick(&[pr(false), pr(true)]), FRAME);
        assert_eq!(FRAME, Duration::from_millis(100), "one frame of `palette::frame`");
    }

    #[test]
    fn the_legend_popup_is_sized_by_the_most_recently_active_client() {
        assert_eq!(newest_client("100 80 24\n300 215 62\n200 120 30\n"), Some((215, 62)));
        assert_eq!(newest_client(""), None);
        assert_eq!(newest_client("junk\n"), None);
    }

    /// tui-W10: a name or a load that cannot be read is no error of the view.
    #[test]
    fn a_missing_hostname_file_or_load_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hostname");
        assert_eq!(read_hostname(&file), None, "no file");
        std::fs::write(&file, "\n").unwrap();
        assert_eq!(read_hostname(&file), None, "an empty one");
        std::fs::write(&file, "homeserver\n").unwrap();
        assert_eq!(read_hostname(&file).as_deref(), Some("homeserver"));
        assert!(!hostname().is_empty());
        assert!(!load_average().is_empty(), "the numbers, or the `?`");
    }

    #[test]
    fn the_load_is_the_first_three_numbers_of_proc_loadavg_or_sysctl() {
        let load = |text| parse_load(text).map(|l| l.join(" "));
        assert_eq!(load("16.89 24.21 25.00 6/3484 1221694\n").as_deref(), Some("16.89 24.21 25.00"));
        assert_eq!(load("{ 0.52 0.58 0.59 }\n").as_deref(), Some("0.52 0.58 0.59"));
        assert_eq!(load("1.0 2.0"), None);
        assert_eq!(load("a b c"), None);
        assert_eq!(load(""), None);
    }
}
