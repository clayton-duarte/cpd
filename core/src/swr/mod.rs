//! Stale-while-revalidate (docs/architecture/swr.md): the cache and the endpoint health.

pub mod badge;
pub mod cache;
pub mod cmd;
pub mod endpoint;
pub mod gc;
pub mod outbox;
pub mod overlay;
pub mod source;

#[cfg(test)]
mod badge_tests;
#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod endpoint_tests;
#[cfg(test)]
mod gc_tests;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod overlay_tests;
