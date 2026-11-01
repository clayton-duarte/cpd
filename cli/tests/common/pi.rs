//! `FakePi`: the distiller's `pi`, as an in-process route of the `FakeRunner` (docs/implementing.md, Tests). It answers
//! as pi does in `--mode json`: a stream of JSON lines whose final assistant message holds the answer (shapes recorded
//! from pi 1.3.0). What the answer is can depend on the files pi is run over (`batch.md`, ...), which it reads from the
//! call's working directory, as the shell fake it replaces did; every call's copy of those files is kept ([`FakePi::seen`]).
//! What pi would print on stderr is appended to the call's `stderr_to` file, as the real runner does.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

use cpd_core::host::runner::{Call, Handler, Reply};
use serde_json::json;

use super::fx::Fx;

/// The input files pi is run over, of the directory it runs in, by name.
pub type Inputs = BTreeMap<String, String>;

type Behave = Box<dyn Fn(&Inputs) -> Reply + Send + Sync>;

/// The files of the distiller's directory that [`FakePi::seen`] keeps.
const INPUT_FILES: [&str; 4] = ["prompt.md", "batch.md", "known.md", "docs.md"];

pub struct FakePi {
    behave: Mutex<Behave>,
    seen: Mutex<Vec<Inputs>>,
}

impl FakePi {
    /// A `pi` that, until [`FakePi::set`], curates every record of the batch.
    pub fn new() -> FakePi {
        FakePi {
            behave: Mutex::new(Box::new(|inputs| says(&curates_all(inputs)))),
            seen: Mutex::default(),
        }
    }

    /// Installs a [`FakePi::new`] as the route for `pi` on `fx`'s `FakeRunner`.
    pub fn install(fx: &Fx) -> Arc<FakePi> {
        let pi = Arc::new(FakePi::new());
        let _ = (*fx.fakes.runner).clone().route("pi", pi.clone());
        pi
    }

    /// From the next call on, pi answers `behave(inputs)`.
    pub fn set(&self, behave: impl Fn(&Inputs) -> Reply + Send + Sync + 'static) {
        *self.behave.lock().unwrap_or_else(PoisonError::into_inner) = Box::new(behave);
    }

    /// From the next call on, pi's final message is `text`, stopped with `stop`.
    pub fn answers(&self, text: &str) {
        let reply = says(text);
        self.set(move |_| reply.clone());
    }

    /// From the next call on, pi's final message is `text`, cut at the output limit (`stopReason` `length`).
    pub fn cut_after(&self, text: &str) {
        let reply = says_with(text, "length");
        self.set(move |_| reply.clone());
    }

    /// From the next call on, pi curates every record of the batch ([`curates_all`]).
    pub fn curates_all(&self) {
        self.set(|inputs| says(&curates_all(inputs)));
    }

    /// The file `name` as the last call saw it; panics when pi was never called or the file was missing.
    pub fn seen(&self, name: &str) -> String {
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let last = seen
            .last()
            .unwrap_or_else(|| panic!("pi was never called, it saw no {name}"));
        last.get(name)
            .unwrap_or_else(|| panic!("the distiller saw no {name}"))
            .clone()
    }
}

impl Handler for FakePi {
    fn handle(&self, call: &Call) -> Reply {
        let mut inputs = Inputs::new();
        for name in INPUT_FILES {
            if let Some(text) = call
                .cwd
                .as_ref()
                .and_then(|dir| std::fs::read_to_string(dir.join(name)).ok())
            {
                inputs.insert(name.to_owned(), text);
            }
        }
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(inputs.clone());
        let reply = (self.behave.lock().unwrap_or_else(PoisonError::into_inner))(&inputs);
        match (reply, &call.stderr_to) {
            (
                Reply::Exit {
                    code,
                    stdout,
                    stderr,
                    takes,
                },
                Some(log),
            ) => {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log)
                    .expect("open the log pi's stderr goes to");
                file.write_all(&stderr)
                    .expect("write pi's stderr to the log");
                Reply::Exit {
                    code,
                    stdout,
                    stderr: Vec::new(),
                    takes,
                }
            }
            (reply, _) => reply,
        }
    }
}

/// One valid curation line for every block of `batch.md`: topic `t`, short `s <id>`, no retirement, no doc candidate.
pub fn curates_all(inputs: &Inputs) -> String {
    let batch = inputs
        .get("batch.md")
        .map(String::as_str)
        .unwrap_or_default();
    batch
        .lines()
        .filter_map(|line| line.strip_prefix('[')?.split_once(']').map(|(id, _)| id))
        .map(|id| format!("{{\"id\":\"{id}\",\"topic\":\"t\",\"short\":\"s {id}\",\"retire\":null,\"doc\":false}}\n"))
        .collect()
}

/// pi exits 0 after a stream whose final assistant message is `text` (stopped with `stop`).
pub fn says(text: &str) -> Reply {
    says_with(text, "stop")
}

fn says_with(text: &str, stop: &str) -> Reply {
    let lines = [
        json!({"type": "session", "version": 3, "id": "x", "timestamp": "2026-10-19T01:59:25.851Z", "cwd": "/"}),
        json!({"type": "agent_start"}),
        json!({"type": "message_end", "message": {
            "role": "assistant",
            "content": [{"type": "thinking", "thinking": "hm"}, {"type": "text", "text": text}],
            "usage": {"input": 2, "output": 4, "cacheRead": 100, "cacheWrite": 631, "reasoning": 1, "totalTokens": 737,
                      "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0.25}},
            "stopReason": stop,
        }}),
        json!({"type": "agent_settled", "aborted": false}),
    ];
    let stream: String = lines.iter().map(|line| format!("{line}\n")).collect();
    Reply::ok(&stream)
}
