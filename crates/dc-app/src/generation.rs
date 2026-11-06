// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Generation bookkeeping (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app" and "Digest and staleness"): which operations need
//! regenerating for the current snapshot, which results are in, and what
//! `finish` should run on. The worker thread and the per-operation digest
//! are slice 9's (#538); here the digest is the snapshot id, so every
//! enabled operation is pending once per snapshot, and a result from an
//! older snapshot is dropped.

use std::collections::{BTreeMap, BTreeSet};

use dc_job::{EmitOp, Finding, Findings, Finished, Generated};
use dc_project::{OperationId, Project};

/// Identifies one project snapshot: every `run`, `undo`, `redo` and `open`
/// makes a new one, so a result that arrives late can be told from one
/// that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId(u64);

impl SnapshotId {
    /// The id after this one.
    const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// One operation to generate: for which snapshot, and the digest of what
/// its generator reads, which the result carries back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The snapshot the job was issued for.
    pub snapshot: SnapshotId,
    /// The operation.
    pub op: OperationId,
    /// Why it regenerates: a digest of everything its generator reads.
    pub digest: u64,
}

/// What the worker hands back.
#[derive(Debug, Clone, PartialEq)]
pub enum JobResult {
    /// One operation generated ([`dc_job::generate_operation`]).
    Operation {
        /// The job it answers.
        job: Job,
        /// Its runs and findings.
        generated: Generated,
    },
    /// The project-level pass ([`dc_job::finish`]) over every operation's
    /// runs.
    Finished {
        /// The snapshot it was computed for.
        snapshot: SnapshotId,
        /// Its findings and programs.
        finished: Finished,
    },
}

/// A stored result of one operation.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationResult {
    /// The snapshot it was generated for.
    pub snapshot: SnapshotId,
    /// The digest its job carried.
    pub digest: u64,
    /// Its runs, in cut order.
    pub runs: Vec<EmitOp>,
    /// What generating found.
    pub findings: Findings,
}

/// The state of generation for the current snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Generation {
    snapshot: SnapshotId,
    /// The enabled operations of the current snapshot, in project order.
    order: Vec<OperationId>,
    /// Whether validation passed, so generators may run at all.
    runnable: bool,
    results: BTreeMap<OperationId, OperationResult>,
    /// Jobs handed out and not yet answered.
    in_flight: BTreeSet<OperationId>,
    finished: Option<(SnapshotId, Finished)>,
}

impl Generation {
    /// Bookkeeping for a first snapshot of `project`; `runnable` is whether
    /// validation found no error.
    #[must_use]
    pub(crate) fn new(project: &Project, runnable: bool) -> Self {
        let mut generation = Self {
            snapshot: SnapshotId(0),
            order: Vec::new(),
            runnable,
            results: BTreeMap::new(),
            in_flight: BTreeSet::new(),
            finished: None,
        };
        generation.advance(project, runnable);
        generation
    }

    /// Moves to a new snapshot of `project`: a new id, the enabled
    /// operations re-read, every job in flight forgotten (its result will
    /// be dropped), and results kept for the operations that still exist,
    /// stale until replaced.
    pub(crate) fn advance(&mut self, project: &Project, runnable: bool) {
        self.snapshot = self.snapshot.next();
        self.order = project
            .operations
            .iter()
            .filter(|op| op.enabled())
            .map(|op| op.id().clone())
            .collect();
        self.runnable = runnable;
        self.in_flight.clear();
        self.results
            .retain(|id, _| project.operations.iter().any(|op| op.id() == id));
    }

    /// The current snapshot.
    #[must_use]
    pub fn snapshot(&self) -> SnapshotId {
        self.snapshot
    }

    /// The digest of what `op`'s generator reads in the current snapshot.
    // TODO(#538): hash the operation's inputs; until then every snapshot
    // changes every digest, so every enabled operation regenerates once.
    #[must_use]
    pub fn digest(&self, _op: &OperationId) -> u64 {
        self.snapshot.0
    }

    /// Whether `op` has a result for the current digest.
    #[must_use]
    pub fn is_current(&self, op: &OperationId) -> bool {
        self.results
            .get(op)
            .is_some_and(|r| r.digest == self.digest(op))
    }

    /// Whether `op` is enabled and its stored result, if any, is for an
    /// older digest: the Viewport draws it faded until the new one arrives.
    #[must_use]
    pub fn is_stale(&self, op: &OperationId) -> bool {
        self.order.contains(op) && !self.is_current(op)
    }

    /// The jobs the worker should run next: every enabled operation
    /// without a current result and not already in flight, in project
    /// order; none when validation failed, since generators never see a
    /// project it rejects.
    #[must_use]
    pub fn pending(&self) -> Vec<Job> {
        if !self.runnable {
            return Vec::new();
        }
        self.order
            .iter()
            .filter(|op| !self.is_current(op) && !self.in_flight.contains(*op))
            .map(|op| Job {
                snapshot: self.snapshot,
                op: op.clone(),
                digest: self.digest(op),
            })
            .collect()
    }

    /// [`Generation::pending`], marking each job in flight so it is handed
    /// out once per snapshot.
    pub fn take_pending(&mut self) -> Vec<Job> {
        let jobs = self.pending();
        self.in_flight.extend(jobs.iter().map(|j| j.op.clone()));
        jobs
    }

    /// Stores `result` when it is for the current snapshot; `false` when
    /// it was dropped as stale.
    pub fn accept(&mut self, result: JobResult) -> bool {
        match result {
            JobResult::Operation { job, generated } => {
                if job.snapshot != self.snapshot {
                    return false;
                }
                self.in_flight.remove(&job.op);
                self.results.insert(
                    job.op,
                    OperationResult {
                        snapshot: job.snapshot,
                        digest: job.digest,
                        runs: generated.runs,
                        findings: generated.findings,
                    },
                );
                true
            }
            JobResult::Finished { snapshot, finished } => {
                if snapshot != self.snapshot {
                    return false;
                }
                self.finished = Some((snapshot, finished));
                true
            }
        }
    }

    /// Whether every enabled operation generated without error for the
    /// current snapshot: what `finish` needs.
    fn all_generated(&self) -> bool {
        self.runnable
            && !self.order.is_empty()
            && self.in_flight.is_empty()
            && self.order.iter().all(|op| {
                self.results
                    .get(op)
                    .is_some_and(|r| r.digest == self.digest(op) && !r.findings.has_errors())
            })
    }

    /// Whether [`dc_job::finish`] should run: every enabled operation has
    /// a current result without error and no `Finished` is in for this
    /// snapshot.
    #[must_use]
    pub fn needs_finish(&self) -> bool {
        self.all_generated()
            && self
                .finished
                .as_ref()
                .is_none_or(|(s, _)| *s != self.snapshot)
    }

    /// Every enabled operation's current runs, in project order, for
    /// `finish`; `None` until they are all in.
    #[must_use]
    pub fn finish_input(&self) -> Option<Vec<EmitOp>> {
        if !self.all_generated() {
            return None;
        }
        Some(
            self.order
                .iter()
                .filter_map(|op| self.results.get(op))
                .flat_map(|r| r.runs.iter().cloned())
                .collect(),
        )
    }

    /// The stored result of `op`, current or stale.
    #[must_use]
    pub fn result(&self, op: &OperationId) -> Option<&OperationResult> {
        self.results.get(op)
    }

    /// The `Finished` of the current snapshot, once it is in.
    #[must_use]
    pub fn finished(&self) -> Option<&Finished> {
        self.finished
            .as_ref()
            .filter(|(s, _)| *s == self.snapshot)
            .map(|(_, f)| f)
    }

    /// The findings of the current snapshot's results: each current
    /// operation's, in project order, then `finish`'s.
    #[must_use]
    pub(crate) fn findings(&self) -> Vec<Finding> {
        let mut out = Vec::new();
        for op in &self.order {
            if let Some(result) = self.results.get(op).filter(|r| r.digest == self.digest(op)) {
                out.extend(result.findings.errors.iter().cloned());
                out.extend(result.findings.warnings.iter().cloned());
            }
        }
        if let Some(finished) = self.finished() {
            out.extend(finished.findings.errors.iter().cloned());
            out.extend(finished.findings.warnings.iter().cloned());
        }
        out
    }
}

// TODO(#538): the stubs below are replaced by the slice's implementation.

/// The digest of what `op`'s generator reads in `project`.
#[must_use]
pub fn digest(_project: &Project, _op: &OperationId) -> u64 {
    0
}

/// Work for the worker.
#[derive(Debug, Clone, PartialEq)]
pub enum Work {
    /// Generate one operation.
    Operation(Job),
    /// Run the project-level pass.
    Finish(FinishJob),
}

/// The project-level pass's input.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishJob {
    /// The snapshot it is for.
    pub snapshot: SnapshotId,
    /// Every enabled operation's runs.
    pub runs: Vec<EmitOp>,
    /// Whether to emit.
    pub emit: bool,
}

/// Runs `work` on `project`.
#[must_use]
pub fn execute(
    project: &Project,
    work: Work,
    _date: &str,
    _estimate: &dc_job::EstimateConfig,
) -> JobResult {
    match work {
        Work::Operation(job) => {
            let generated = dc_job::generate_operation(project, &job.op);
            JobResult::Operation { job, generated }
        }
        Work::Finish(job) => JobResult::Finished {
            snapshot: job.snapshot,
            finished: Finished {
                findings: Findings::default(),
                programs: Vec::new(),
                total_line: None,
            },
        },
    }
}

impl Generation {
    /// Whether `op` waits for a result.
    #[must_use]
    pub fn is_regenerating(&self, _op: &OperationId) -> bool {
        false
    }

    /// Progress.
    #[must_use]
    pub fn progress(&self) -> Option<(usize, usize)> {
        None
    }

    /// Whether anything is left to do.
    #[must_use]
    pub fn busy(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    #[test]
    fn every_enabled_operation_is_pending_once_per_snapshot() {
        let project = sample_project();
        let mut generation = Generation::new(&project, true);
        let jobs = generation.take_pending();
        assert_eq!(jobs.len(), project.operations.len());
        assert!(generation.take_pending().is_empty(), "handed out twice");
        for job in jobs {
            assert!(generation.accept(JobResult::Operation {
                job,
                generated: Generated::default(),
            }));
        }
        assert!(generation.needs_finish());
        generation.advance(&project, true);
        assert_eq!(generation.pending().len(), project.operations.len());
    }

    #[test]
    fn results_from_an_old_snapshot_are_dropped() {
        let project = sample_project();
        let mut generation = Generation::new(&project, true);
        let jobs = generation.take_pending();
        generation.advance(&project, true);
        for job in jobs {
            assert!(!generation.accept(JobResult::Operation {
                job,
                generated: Generated::default(),
            }));
        }
        assert!(!generation.needs_finish());
        assert_eq!(generation.pending().len(), project.operations.len());
    }

    #[test]
    fn nothing_is_pending_when_validation_failed() {
        let generation = Generation::new(&sample_project(), false);
        assert!(generation.pending().is_empty());
        assert!(!generation.needs_finish());
    }
}
