//! Stale-while-revalidate (docs/architecture/swr.md): the cache and the endpoint health.

pub mod cache;
pub mod endpoint;

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod endpoint_tests;
