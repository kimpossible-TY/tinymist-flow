//! Revision and lifetime guards for queued compilation.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tinymist_world::{CompilerFeat, ProjectInsId};

use super::{CompileReport, CompiledArtifact};

/// Identifies one compilation within a project lifetime.
///
/// Reusing a project ID after a restart creates a new generation source, so an
/// old worker cannot clear or publish work belonging to the replacement.
#[derive(Clone, Debug)]
pub struct CompileTicket {
    /// The project being compiled.
    pub id: ProjectInsId,
    /// The input world revision.
    pub revision: usize,
    generation: usize,
    current: Arc<AtomicUsize>,
}

impl CompileTicket {
    pub(super) fn new(id: ProjectInsId, revision: usize, current: Arc<AtomicUsize>) -> Self {
        Self {
            id,
            revision,
            generation: current.load(Ordering::Acquire),
            current,
        }
    }

    /// Whether the task still corresponds to the requested inputs and demand.
    pub fn is_current(&self) -> bool {
        self.generation != 0 && self.current.load(Ordering::Acquire) == self.generation
    }

    /// Whether two tickets identify the same task in the same project lifetime.
    pub fn same_task(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.generation == other.generation
            && Arc::ptr_eq(&self.current, &other.current)
    }

    pub(super) fn belongs_to(&self, current: &Arc<AtomicUsize>) -> bool {
        Arc::ptr_eq(&self.current, current)
    }
}

/// Completion acknowledged by the actor, including work skipped before execution.
pub struct CompileOutcome<F: CompilerFeat> {
    /// The task whose admission slot must be released.
    pub ticket: CompileTicket,
    /// Whether synchronous compilation ran, even if its result became stale.
    pub executed: bool,
    /// The computed artifact, absent when work was superseded at a safe boundary.
    pub artifact: Option<CompiledArtifact<F>>,
    /// The completed status, published only if the actor still accepts the task.
    pub report: Option<CompileReport>,
}

impl<F: CompilerFeat> CompileOutcome<F> {
    pub(super) fn discarded(ticket: CompileTicket, executed: bool) -> Self {
        Self {
            ticket,
            executed,
            artifact: None,
            report: None,
        }
    }

    /// Whether the result may be shown to consumers after actor admission.
    pub fn is_publishable(&self) -> bool {
        self.ticket.is_current()
            && self.artifact.as_ref().is_some_and(|artifact| {
                !artifact
                    .diagnostics()
                    .any(|d| d.message == super::FILE_MISSING_ERROR_MSG)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_lifetime_cannot_complete_restarted_project() {
        let old = CompileTicket::new(ProjectInsId::PRIMARY, 3, Arc::new(AtomicUsize::new(1)));
        let new = CompileTicket::new(ProjectInsId::PRIMARY, 3, Arc::new(AtomicUsize::new(1)));
        assert!(!old.same_task(&new));
    }

    #[test]
    fn pause_then_resume_does_not_revalidate_queued_work() {
        let current = Arc::new(AtomicUsize::new(1));
        let old = CompileTicket::new(ProjectInsId::PRIMARY, 3, current.clone());
        current.fetch_add(1, Ordering::AcqRel);
        current.fetch_add(1, Ordering::AcqRel);
        let new = CompileTicket::new(ProjectInsId::PRIMARY, 3, current);
        assert!(!old.is_current());
        assert!(new.is_current());
        assert!(!old.same_task(&new));
    }
}
