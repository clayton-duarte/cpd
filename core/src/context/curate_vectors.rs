//! The vector cache `$CPD_HOME/vectors.db` of write-time curation (contract §4.6): a derived cache, not the
//! source of truth. Stub: the implementation Job fills it.
#![allow(dead_code, unused_variables)]

use std::collections::HashSet;
use std::path::PathBuf;

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};

/// The vector cache file.
pub struct VectorStore {
    path: PathBuf,
}

impl VectorStore {
    pub fn at(home: &CpdHome) -> Self {
        VectorStore {
            path: home.vectors_db(),
        }
    }

    pub fn put(
        &self,
        context: &ContextId,
        model: &str,
        record: &RecordId,
        vector: &[f32],
        at: &str,
    ) -> Result<(), CpdError> {
        todo!()
    }

    pub fn get(
        &self,
        context: &ContextId,
        model: &str,
        ids: &[RecordId],
    ) -> Result<Vec<(RecordId, Vec<f32>)>, CpdError> {
        todo!()
    }

    pub fn embedded(
        &self,
        context: &ContextId,
        model: &str,
    ) -> Result<HashSet<RecordId>, CpdError> {
        todo!()
    }

    pub fn mark_checked(
        &self,
        context: &ContextId,
        record: &RecordId,
        at: &str,
    ) -> Result<(), CpdError> {
        todo!()
    }

    pub fn checked(&self, context: &ContextId) -> Result<HashSet<RecordId>, CpdError> {
        todo!()
    }
}
