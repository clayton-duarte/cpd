//! `GitRoute`: the `Handler` of the program `git`, the argv of git over a [`Universe`].
//!
//! STUB: every body is `todo!()`.

use std::sync::{Arc, Mutex};

use super::universe::Universe;
use crate::host::runner::{Call, Handler, Reply};

pub struct GitRoute {
    _universe: Arc<Mutex<Universe>>,
}

impl GitRoute {
    pub fn new(_universe: Arc<Mutex<Universe>>) -> GitRoute {
        todo!("GitRoute::new")
    }
    /// Every call the route refused as `fake-git: unsupported: ...`, as the text after that prefix
    /// (`status --bogus`, `nonsense`), in order.
    pub fn unsupported(&self) -> Vec<String> {
        todo!("GitRoute::unsupported")
    }
}

impl Handler for GitRoute {
    fn handle(&self, _call: &Call) -> Reply {
        todo!("GitRoute::handle")
    }
}
