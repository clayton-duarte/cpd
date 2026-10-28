//! The Inbox (contract §2.2 "Inbox item", §3.7 E8, §4.4; commands.md §21): the
//! stored items of what needs the User, and the reconcile that opens and closes them.

pub mod cmd;
pub mod model;
pub mod models;
pub mod pr;
pub mod rules;
pub mod team;

#[cfg(test)]
mod blue_tests;
#[cfg(test)]
mod approve_tests;
#[cfg(test)]
mod deploy_tests;
#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod pr_tests;
#[cfg(test)]
mod follow_tests;
#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod rows_tests;
#[cfg(test)]
mod rows_review_tests;
