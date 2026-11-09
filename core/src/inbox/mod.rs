//! The Inbox (contract §2.2 "Inbox item", §3.7 E8, §4.4; commands.md §21): the
//! stored items of what needs the User, and the reconcile that opens and closes them.

pub mod asks;
pub mod cmd;
pub mod model;
pub mod models;
pub mod pr;
pub mod refusals;
pub mod review_plan;
pub mod rules;
pub mod team;

#[cfg(test)]
mod approve_refusal_tests;
#[cfg(test)]
mod approve_tests;
#[cfg(test)]
mod blue_tests;
#[cfg(test)]
mod claim_refusal_tests;
#[cfg(test)]
mod claim_reuse_tests;
#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod closed_tests;
#[cfg(test)]
mod deploy_tests;
#[cfg(test)]
mod dialog_link_tests;
#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod dismiss_tests;
#[cfg(test)]
mod follow_tests;
#[cfg(test)]
mod followup_tests;
#[cfg(test)]
mod overdue_tests;
#[cfg(test)]
mod pr_swap_tests;
#[cfg(test)]
mod pr_tests;
#[cfg(test)]
mod pr_updating_tests;
#[cfg(test)]
mod reopen_tests;
#[cfg(test)]
mod review_plan_close_tests;
#[cfg(test)]
mod rows_review_tests;
#[cfg(test)]
mod rows_tests;
#[cfg(test)]
mod snooze_refusal_tests;
#[cfg(test)]
pub(crate) mod swr_fixture;
#[cfg(test)]
mod team_tests;
#[cfg(test)]
mod tier_tests;
#[cfg(test)]
mod unread_rows_tests;
