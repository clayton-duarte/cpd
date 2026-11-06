//! Stale-while-revalidate (docs/architecture/swr.md): the cache and the endpoint health.

pub mod cache;
pub mod endpoint;
pub mod gc;
pub mod outbox;
pub mod overlay;

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
