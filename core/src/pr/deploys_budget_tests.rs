//! REST call budget and rate-limit backoff tests for deploy workflow polling with ETags (swr-github.md §8).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::deploys::read_into_store;
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{Call, FakeRunner, Handler, Reply};
use crate::store::Store;

#[test]
fn conditional_etag_avoids_budget_burn() {
    todo!()
}

#[test]
fn rate_limit_403_flags_rate_limited_error() {
    todo!()
}
