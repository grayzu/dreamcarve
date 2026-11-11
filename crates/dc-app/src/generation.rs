// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Generation bookkeeping (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app" and "Digest and staleness"; functional spec, capability 71):
//! which operations need regenerating for the current snapshot, which
//! results are in, what `finish` should run on, and what Stop and
//! Regenerate all do.
//!
//! Each enabled operation has a [`digest`] of everything its generator
//! reads. An operation is current when its stored result carries the
//! current digest, and stale otherwise: the Viewport draws its old runs
//! faded until the new result lands. After an edit only the operations
//! whose digest changed are pending, so editing one operation regenerates
//! it and the operations that read it. `finish` reruns on every snapshot,
//! since the order check and emission read the whole project.
//!
//! A result is kept only when it answers a job handed out for the current
//! snapshot: a late answer to an older snapshot, or to a job Stop
//! abandoned, is dropped. Stop holds every stale operation at its digest,
//! so nothing is pending until an edit changes a digest or Regenerate all
//! forces every operation (design open question 4: the generators have
//! no cancellation point, so the job the worker is on runs to its end and
//! its result is dropped).
//!
//! Nothing here runs a generator: [`execute`] does, on whatever thread
//! calls it. The shell's worker thread is the only caller in the app; the
//! test harness calls it inline.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use dc_job::preview::Marks;
use dc_job::{EmitOp, EstimateConfig, Findings, Finished, Generated, ProgramSplit};
use dc_project::{InstanceId, Operation, OperationId, Project, ToolId};
use serde::Serialize;

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

/// The project-level pass once every enabled operation has a current
/// result: its input.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishJob {
    /// The snapshot it was issued for.
    pub snapshot: SnapshotId,
    /// Every enabled operation's runs, in project order.
    pub runs: Vec<EmitOp>,
    /// Whether every operation generated: then [`dc_job::finish`] runs
    /// (the order and tab checks, then emission); else only the checks
    /// ([`dc_job::project_checks`]), as `check` reports them when an
    /// operation failed.
    pub emit: bool,
}

/// What the shell hands the worker.
#[derive(Debug, Clone, PartialEq)]
pub enum Work {
    /// Generate one operation ([`dc_job::generate_operation`]).
    Operation(Job),
    /// The project-level pass.
    Finish(FinishJob),
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
        /// Each run's marks (`dc_job::preview::marks`), found on the
        /// worker so the window does not walk the toolpaths again.
        marks: Vec<Marks>,
        /// `check`'s carve summary of its runs as facts
        /// ([`dc_job::carve_facts`]), found on the worker: empty but for
        /// a V-Carve or an Inlay (slice 13, #542; slice 28, #620).
        carves: Vec<dc_job::CarveFacts>,
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

/// Runs `work` on `project`, the snapshot it was issued for: the one
/// place the app calls the engine's generators. `date` heads the
/// programs `finish` emits, `estimate` is the user's `[estimate]`
/// settings. Pure; the shell calls it on its worker thread.
#[must_use]
pub fn execute(project: &Project, work: Work, date: &str, estimate: &EstimateConfig) -> JobResult {
    match work {
        Work::Operation(job) => {
            let generated = dc_job::generate_operation(project, &job.op);
            let marks = generated
                .runs
                .iter()
                .map(|run| dc_job::preview::marks(&dc_job::preview::Run::from(run)))
                .collect();
            // Only this operation has runs here, so only its lines are
            // described: `check`'s, from the same runs.
            let carves = dc_job::carve_facts(project, &generated.runs);
            JobResult::Operation {
                job,
                generated,
                marks,
                carves,
            }
        }
        Work::Finish(FinishJob {
            snapshot,
            runs,
            emit,
        }) => {
            let finished = if emit {
                dc_job::finish(project, &runs, ProgramSplit::ProjectDefault, date, estimate)
            } else {
                Finished {
                    findings: dc_job::project_checks(project, &runs),
                    programs: Vec::new(),
                    total_line: None,
                }
            };
            JobResult::Finished { snapshot, finished }
        }
    }
}

/// A 64-bit FNV-1a hash, fed through [`Write`] so values serialize
/// straight into it. Its output is fixed by its definition, so a digest
/// means the same on every build and Rust version (unlike `std`'s
/// `DefaultHasher`, whose algorithm may change).
struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    /// `value`'s JSON under `tag`: the tag keeps two inputs that
    /// serialize alike (a missing tool and a missing definition) apart.
    fn put<T: Serialize + ?Sized>(&mut self, tag: &str, value: &T) {
        self.bytes(tag.as_bytes());
        self.bytes(b"=");
        if serde_json::to_writer(&mut *self, value).is_err() {
            // The model always serializes (it is what a save writes); a
            // value that did not is marked rather than skipped.
            self.bytes(b"!unserializable");
        }
        self.bytes(b";");
    }
}

impl Write for Fnv {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Whether `op`'s generator reads earlier operations: a rest Pocket its
/// `rest_of` (and that Pocket's tool), a V-Carve and an Inlay the floors
/// before them (`dc_project::cleared_start_plane`, their cone checks), a
/// countersink the floors before it. Each of those reads the earlier
/// operation's targets and tool too.
fn reads_earlier(op: &Operation) -> bool {
    match op {
        Operation::Pocket(pocket) => pocket.rest_of.is_some(),
        Operation::Drill(drill) => drill.cycle.is_countersink(),
        // A Photo V-Carve reads the floors before it too (format 27).
        Operation::VCarve(_) | Operation::Inlay(_) | Operation::PhotoVCarve(_) => true,
        Operation::Profile(_) | Operation::Engrave(_) | Operation::Bore(_) => false,
    }
}

/// Pushes `item` unless `list` holds it.
fn push_once<'a, T: PartialEq + ?Sized>(list: &mut Vec<&'a T>, item: &'a T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// The digest of everything operation `op`'s generator reads in
/// `project` (design, "Digest and staleness"), in a fixed order: the
/// operation's record and, when it reads earlier operations
/// (a rest Pocket, a countersink, a V-Carve or an Inlay), the record of every operation before it; every
/// tool those records name (their own, their clearing passes', their
/// cut-outs'); every instance they target and the definition each
/// names; `stock`, `origin`, `heights`, `display_units`, the two
/// tolerances and the post-processor's decimal places (every generator
/// snaps to the grid the program is written on); and the placed tabs on
/// the targets. An operation the project does not have digests its id
/// alone.
///
/// Over-invalidation is accepted: two projects whose digests are equal
/// generate the same for `op`, and two that differ may or may not.
/// `tests/generation.rs`, `digest_covers_every_generator_input`, holds
/// the first half to account.
#[must_use]
pub fn digest(project: &Project, op: &OperationId) -> u64 {
    let mut hash = Fnv::new();
    let Some(index) = project.operations.iter().position(|o| o.id() == op) else {
        hash.put("missing", op);
        return hash.0;
    };
    let mut records: Vec<&Operation> = Vec::new();
    if let Some(operation) = project.operations.get(index) {
        records.push(operation);
        if reads_earlier(operation) {
            records.extend(project.operations.iter().take(index));
        }
    }
    hash.put("operations", &records);

    let mut tools: Vec<&ToolId> = Vec::new();
    let mut targets: Vec<&InstanceId> = Vec::new();
    for record in &records {
        push_once(&mut tools, record.tool());
        for tool in record.run_tools() {
            push_once(&mut tools, tool);
        }
        for target in record.targets() {
            push_once(&mut targets, target);
        }
    }
    for id in tools {
        hash.put("tool", id);
        hash.put("is", &project.tools.iter().find(|t| &t.id == id));
    }
    for id in &targets {
        let instance = project.instances.iter().find(|i| &i.id == *id);
        hash.put("instance", *id);
        hash.put("is", &instance);
        if let Some(instance) = instance {
            hash.put("definition", &project.definitions.get(&instance.definition));
        }
    }
    hash.put("stock", &project.stock);
    hash.put("origin", &project.origin);
    hash.put("heights", &project.heights);
    hash.put("display_units", &project.display_units);
    hash.put("flatten_tolerance_mm", &project.flatten_tolerance_mm);
    hash.put("arc_fit_tolerance_mm", &project.arc_fit_tolerance_mm);
    hash.put("decimal_places", &project.post_processor.decimal_places);
    let tabs: Vec<_> = project
        .tabs
        .iter()
        .filter(|t| targets.contains(&&t.instance))
        .collect();
    hash.put("tabs", &tabs);
    hash.0
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
    /// Each run's marks, in `runs`' order (the worker's; empty when the
    /// result was made without them, and the Scene finds them then).
    pub marks: Vec<Marks>,
    /// `check`'s carve summary of its runs, as facts: the Warnings
    /// drawer's carve note and the status bar's message
    /// ([`crate::warnings::carve_note`]).
    pub carves: Vec<dc_job::CarveFacts>,
}

/// How far the current round of generation has got, for the status bar's
/// *Generating k of n…*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The operation being generated, counting from 1.
    pub at: usize,
    /// The operations this round regenerates.
    pub of: usize,
}

/// The state of generation for the current snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Generation {
    snapshot: SnapshotId,
    /// The enabled operations of the current snapshot, in project order.
    order: Vec<OperationId>,
    /// Each enabled operation's digest in the current snapshot.
    digests: BTreeMap<OperationId, u64>,
    /// Whether validation passed, so generators may run at all.
    runnable: bool,
    results: BTreeMap<OperationId, OperationResult>,
    /// Operations Regenerate all forces, whatever their result's digest.
    forced: BTreeSet<OperationId>,
    /// Operations Stop held, at the digest they had: not pending until
    /// their digest changes or Regenerate all.
    held: BTreeMap<OperationId, u64>,
    /// Jobs handed out for this snapshot and not yet answered.
    in_flight: BTreeSet<OperationId>,
    /// Whether the `finish` of this snapshot is handed out.
    finishing: bool,
    /// Whether Stop held the project-level pass of this snapshot.
    finish_held: bool,
    finished: Option<(SnapshotId, Finished)>,
    /// The operations the current round regenerates, for [`Progress`].
    round: BTreeSet<OperationId>,
}

impl Generation {
    /// Bookkeeping for a first snapshot of `project`; `runnable` is whether
    /// validation found no error.
    #[must_use]
    pub(crate) fn new(project: &Project, runnable: bool) -> Self {
        Self::after(SnapshotId(0), project, runnable)
    }

    /// [`Generation::new`] for a project that replaces one whose last
    /// snapshot was `previous` (*Open*, *New*): its ids carry on from
    /// there, so a late result for the old project never matches a
    /// snapshot of the new one.
    #[must_use]
    pub(crate) fn after(previous: SnapshotId, project: &Project, runnable: bool) -> Self {
        let mut generation = Self {
            snapshot: previous,
            order: Vec::new(),
            digests: BTreeMap::new(),
            runnable,
            results: BTreeMap::new(),
            forced: BTreeSet::new(),
            held: BTreeMap::new(),
            in_flight: BTreeSet::new(),
            finishing: false,
            finish_held: false,
            finished: None,
            round: BTreeSet::new(),
        };
        generation.advance(project, runnable);
        generation
    }

    /// Moves to a new snapshot of `project`: a new id, the enabled
    /// operations and their digests re-read, every job in flight
    /// forgotten (its result will be dropped), and results kept for the
    /// operations that still exist, current where their digest is.
    pub(crate) fn advance(&mut self, project: &Project, runnable: bool) {
        self.snapshot = self.snapshot.next();
        self.order = project
            .operations
            .iter()
            .filter(|op| op.enabled())
            .map(|op| op.id().clone())
            .collect();
        self.digests = self
            .order
            .iter()
            .map(|op| (op.clone(), digest(project, op)))
            .collect();
        self.runnable = runnable;
        self.in_flight.clear();
        self.finishing = false;
        self.finish_held = false;
        self.results
            .retain(|id, _| project.operations.iter().any(|op| op.id() == id));
        let digests = &self.digests;
        self.forced.retain(|op| digests.contains_key(op));
        self.held
            .retain(|op, held| digests.get(op).is_some_and(|d| d == held));
        if runnable {
            let waiting: Vec<OperationId> = self
                .order
                .iter()
                .filter(|op| self.waits(op))
                .cloned()
                .collect();
            self.round.retain(|op| digests.contains_key(op));
            self.round.extend(waiting);
        } else {
            self.round.clear();
        }
    }

    /// The current snapshot.
    #[must_use]
    pub fn snapshot(&self) -> SnapshotId {
        self.snapshot
    }

    /// The digest of what `op`'s generator reads in the current snapshot;
    /// `None` when it is not an enabled operation.
    #[must_use]
    pub fn digest(&self, op: &OperationId) -> Option<u64> {
        self.digests.get(op).copied()
    }

    /// Whether `op`'s stored result carries its current digest.
    fn result_matches(&self, op: &OperationId) -> bool {
        self.results
            .get(op)
            .is_some_and(|r| Some(r.digest) == self.digest(op))
    }

    /// Whether `op` has a result for its current digest and Regenerate all
    /// does not force it.
    #[must_use]
    pub fn is_current(&self, op: &OperationId) -> bool {
        self.result_matches(op) && !self.forced.contains(op)
    }

    /// Whether `op` is enabled and has no current result, so the Viewport
    /// draws its stored runs, if any, faded until the new ones arrive.
    #[must_use]
    pub fn is_stale(&self, op: &OperationId) -> bool {
        self.digests.contains_key(op) && !self.is_current(op)
    }

    /// Whether `op` is stale and Stop holds it.
    fn is_held(&self, op: &OperationId) -> bool {
        self.held
            .get(op)
            .is_some_and(|held| Some(*held) == self.digest(op))
    }

    /// Whether `op` is stale and will be generated: not held by Stop.
    fn waits(&self, op: &OperationId) -> bool {
        self.is_stale(op) && !self.is_held(op)
    }

    /// Whether `op` waits for a result: pending or being generated. Its
    /// row shows the spinner (capability 71).
    #[must_use]
    pub fn is_regenerating(&self, op: &OperationId) -> bool {
        self.runnable && self.waits(op)
    }

    /// Whether `op` is stale because Stop held it.
    #[must_use]
    pub fn is_stopped(&self, op: &OperationId) -> bool {
        self.runnable && self.is_stale(op) && self.is_held(op)
    }

    /// Whether Stop holds anything back: an operation, or the
    /// project-level pass of this snapshot.
    #[must_use]
    pub fn stopped(&self) -> bool {
        self.order.iter().any(|op| self.is_stopped(op))
            || (self.finish_held && self.all_current() && self.finished().is_none())
    }

    /// The jobs the worker should run: every enabled operation without a
    /// current result, not held by Stop and not already in flight, in
    /// project order; none when validation failed, since generators never
    /// see a project it rejects.
    #[must_use]
    pub fn pending(&self) -> Vec<Job> {
        if !self.runnable {
            return Vec::new();
        }
        self.order
            .iter()
            .filter(|op| self.waits(op) && !self.in_flight.contains(*op))
            .filter_map(|op| {
                Some(Job {
                    snapshot: self.snapshot,
                    op: op.clone(),
                    digest: self.digest(op)?,
                })
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

    /// The next work for the worker, marked handed out: the first pending
    /// operation, else the project-level pass when it is due, else
    /// nothing.
    pub fn next_work(&mut self) -> Option<Work> {
        if let Some(job) = self.pending().into_iter().next() {
            self.in_flight.insert(job.op.clone());
            return Some(Work::Operation(job));
        }
        let job = self.finish_job()?;
        self.finishing = true;
        Some(Work::Finish(job))
    }

    /// Stores `result` when it answers work handed out for the current
    /// snapshot; `false` when it was dropped.
    pub fn accept(&mut self, result: JobResult) -> bool {
        match result {
            JobResult::Operation {
                job,
                generated,
                marks,
                carves,
            } => {
                if job.snapshot != self.snapshot || !self.in_flight.remove(&job.op) {
                    return false;
                }
                self.forced.remove(&job.op);
                self.results.insert(
                    job.op,
                    OperationResult {
                        snapshot: job.snapshot,
                        digest: job.digest,
                        runs: generated.runs,
                        findings: generated.findings,
                        marks,
                        carves,
                    },
                );
                if self.in_flight.is_empty() && self.pending().is_empty() {
                    self.round.clear();
                }
                true
            }
            JobResult::Finished { snapshot, finished } => {
                if snapshot != self.snapshot || !self.finishing {
                    return false;
                }
                self.finishing = false;
                self.finished = Some((snapshot, finished));
                true
            }
        }
    }

    /// Stop: every stale operation held at its digest, so nothing is
    /// pending; the jobs in flight forgotten, so their results are
    /// dropped when they land.
    pub fn stop(&mut self) {
        let stale: Vec<(OperationId, u64)> = self
            .order
            .iter()
            .filter(|op| self.is_stale(op))
            .filter_map(|op| Some((op.clone(), self.digest(op)?)))
            .collect();
        self.held.extend(stale);
        self.in_flight.clear();
        self.finishing = false;
        self.finish_held = true;
        self.round.clear();
    }

    /// Regenerate all: nothing held, every enabled operation forced, the
    /// project-level pass due again.
    pub fn regenerate_all(&mut self) {
        self.held.clear();
        self.forced = self.order.iter().cloned().collect();
        self.finished = None;
        self.finishing = false;
        self.finish_held = false;
        self.round = self.forced.clone();
    }

    /// Whether every enabled operation has a current result and nothing is
    /// in flight: what the project-level pass needs.
    fn all_current(&self) -> bool {
        self.runnable
            && self.in_flight.is_empty()
            && self.order.iter().all(|op| self.is_current(op))
    }

    /// The project-level pass for this snapshot, when it is due: every
    /// enabled operation current, and no `Finished` handed out or in.
    fn finish_job(&self) -> Option<FinishJob> {
        if !self.needs_finish() {
            return None;
        }
        let results: Vec<&OperationResult> = self
            .order
            .iter()
            .filter_map(|op| self.results.get(op))
            .collect();
        Some(FinishJob {
            snapshot: self.snapshot,
            emit: results.iter().all(|r| !r.findings.has_errors()),
            runs: results
                .iter()
                .flat_map(|r| r.runs.iter().cloned())
                .collect(),
        })
    }

    /// Whether the project-level pass should run: every enabled operation
    /// has a current result and no `Finished` for this snapshot is handed
    /// out or in.
    #[must_use]
    pub fn needs_finish(&self) -> bool {
        self.all_current()
            && !self.finishing
            && !self.finish_held
            && self
                .finished
                .as_ref()
                .is_none_or(|(s, _)| *s != self.snapshot)
    }

    /// Every enabled operation's current runs, in project order, once they
    /// are all in without error: what `finish` emits.
    #[must_use]
    pub fn finish_input(&self) -> Option<Vec<EmitOp>> {
        self.finish_job().filter(|j| j.emit).map(|j| j.runs)
    }

    /// Whether anything is left to do for this snapshot: an operation
    /// waiting or in flight, or the project-level pass due or running.
    #[must_use]
    pub fn busy(&self) -> bool {
        !self.in_flight.is_empty()
            || self.finishing
            || !self.pending().is_empty()
            || self.needs_finish()
    }

    /// The current round's progress while an operation is pending or
    /// being generated; `None` otherwise.
    #[must_use]
    pub fn progress(&self) -> Option<Progress> {
        if self.in_flight.is_empty() && self.pending().is_empty() {
            return None;
        }
        let of = self.round.len().max(1);
        let done = self.round.iter().filter(|op| self.is_current(op)).count();
        Some(Progress {
            at: (done + 1).min(of),
            of,
        })
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

    /// The findings of the current snapshot's results, in the order
    /// `build` finds them: each enabled operation's whose result carries
    /// its current digest (forced or not: what it found still holds), in
    /// project order, then `finish`'s. None when validation failed, since
    /// `check` then generates nothing.
    #[must_use]
    pub(crate) fn findings(&self) -> Findings {
        let mut out = Findings::default();
        if !self.runnable {
            return out;
        }
        for op in &self.order {
            if let Some(result) = self.results.get(op).filter(|_| self.result_matches(op)) {
                out.extend(result.findings.clone());
            }
        }
        if let Some(finished) = self.finished() {
            out.extend(finished.findings.clone());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    /// Answers every pending job with an empty result.
    fn answer_all(generation: &mut Generation) {
        while let Some(Work::Operation(job)) = generation.next_work() {
            assert!(generation.accept(JobResult::Operation {
                job,
                generated: Generated::default(),
                marks: Vec::new(),
                carves: Vec::new(),
            }));
        }
    }

    #[test]
    fn each_operation_is_handed_out_once_and_finish_follows() {
        let project = sample_project();
        let mut generation = Generation::new(&project, true);
        let jobs = generation.take_pending();
        assert_eq!(jobs.len(), project.operations.len());
        assert!(generation.take_pending().is_empty(), "handed out twice");
        for job in jobs {
            assert!(generation.accept(JobResult::Operation {
                job,
                generated: Generated::default(),
                marks: Vec::new(),
                carves: Vec::new(),
            }));
        }
        assert!(generation.needs_finish());
        assert!(matches!(generation.next_work(), Some(Work::Finish(_))));
        assert!(!generation.needs_finish(), "finish is handed out once");
    }

    #[test]
    fn an_unchanged_snapshot_keeps_its_results_and_reruns_finish() {
        let project = sample_project();
        let mut generation = Generation::new(&project, true);
        answer_all(&mut generation);
        generation.advance(&project, true);
        assert!(generation.pending().is_empty());
        assert!(generation.needs_finish());
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
                marks: Vec::new(),
                carves: Vec::new(),
            }));
        }
        assert!(!generation.needs_finish());
        assert_eq!(generation.pending().len(), project.operations.len());
    }

    #[test]
    fn a_replaced_project_drops_the_old_projects_results() {
        let project = sample_project();
        let mut old = Generation::new(&project, true);
        let late = old.take_pending();
        let mut opened = Generation::after(old.snapshot(), &project, true);
        assert_eq!(opened.take_pending().len(), late.len());
        for job in late {
            assert!(!opened.accept(JobResult::Operation {
                job,
                generated: Generated::default(),
                marks: Vec::new(),
                carves: Vec::new(),
            }));
        }
    }

    #[test]
    fn nothing_is_pending_when_validation_failed() {
        let generation = Generation::new(&sample_project(), false);
        assert!(generation.pending().is_empty());
        assert!(!generation.needs_finish());
        assert!(!generation.busy());
    }

    #[test]
    fn the_digest_is_stable_and_follows_the_record() {
        let project = sample_project();
        let op = project.operations[0].id().clone();
        assert_eq!(digest(&project, &op), digest(&project.clone(), &op));
        let mut renamed = project.clone();
        renamed.name = "Other".into();
        assert_eq!(
            digest(&project, &op),
            digest(&renamed, &op),
            "the project's name is no generator input"
        );
        let mut deeper = project.clone();
        dc_project::test_support::profile_mut(&mut deeper).depth_mm = Some(5.0);
        assert_ne!(digest(&project, &op), digest(&deeper, &op));
        assert_ne!(
            digest(&project, &op),
            digest(&project, &OperationId::new("nope"))
        );
    }
}
