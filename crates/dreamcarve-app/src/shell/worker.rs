// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The generation worker (`docs/specs/phase-4/design.md`, "Architecture →
//! The shell", threads; functional spec, capability 71): one thread that
//! owns a receiver of work and a sender of results, and runs each piece
//! with `dc_app::generation::execute`, which calls
//! `dc_job::generate_operation` for an operation and `dc_job::finish` once
//! every operation is current. The UI thread never calls them.
//!
//! The shell posts one piece of work at a time, after the snapshot has
//! settled for [`SETTLE`] (`experience.md`, section 8, "Stale and
//! regenerating"), so a drag or a run of typing does not queue a job per
//! frame, and Stop has nothing queued to take back: the job the worker is
//! on runs to its end (design open question 4) and `dc_app` drops its
//! result. The worker never wakes the window itself: the shell polls the
//! channel each frame and asks for a frame [`POLL`] later while work is
//! out, so a result lands within that, and the window idles when nothing
//! is running.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use dc_app::generation::{self, Work};
use dc_app::{App, JobResult, SnapshotId};
use dc_job::EstimateConfig;
use dc_project::Project;

/// How long the snapshot must stay unchanged before its work is posted:
/// `experience.md`'s 250 ms after the last edit.
pub(crate) const SETTLE: Duration = Duration::from_millis(250);
/// How soon the window looks at the channel again while work is out.
pub(crate) const POLL: Duration = Duration::from_millis(50);
/// The date the worker's `finish` heads its programs with, as `check`
/// does: those programs are counted and summarised, never written.
/// Export builds its own (slice 18).
const CHECK_DATE: &str = "check";

/// One piece of work and the snapshot it is for.
struct Request {
    project: Arc<Project>,
    work: Work,
}

/// The worker thread and its two channels.
pub(crate) struct Worker {
    /// `None` when the thread could not start or has stopped.
    jobs: Option<Sender<Request>>,
    results: Receiver<JobResult>,
    /// Whether a piece of work is out and unanswered.
    busy: bool,
    /// The snapshot the last post shared with the thread.
    shared: Option<(SnapshotId, Arc<Project>)>,
    /// The snapshot last seen, to tell an edit.
    seen: Option<SnapshotId>,
    /// When the last change settles.
    settles_at: Option<Instant>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("running", &self.jobs.is_some())
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

impl Worker {
    /// Starts the thread; `estimate` is the user's `[estimate]` settings,
    /// which `finish` prints its estimates with. A thread that cannot
    /// start leaves a worker that runs nothing, and the error says why.
    pub(crate) fn spawn(estimate: EstimateConfig) -> (Self, Option<std::io::Error>) {
        let (jobs, inbox) = mpsc::channel::<Request>();
        let (outbox, results) = mpsc::channel::<JobResult>();
        let started = thread::Builder::new()
            .name("generation".into())
            .spawn(move || {
                for Request { project, work } in inbox {
                    let result = generation::execute(&project, work, CHECK_DATE, &estimate);
                    if outbox.send(result).is_err() {
                        break;
                    }
                }
            });
        let error = started.err();
        let worker = Self {
            jobs: error.is_none().then_some(jobs),
            results,
            busy: false,
            shared: None,
            seen: None,
            settles_at: None,
        };
        (worker, error)
    }

    /// Whether a piece of work is out: the thread is running it.
    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    /// One frame's turn at `now`: every result that came back handed to
    /// `app` (which keeps those for its current snapshot), then, once the
    /// snapshot has settled and the thread is free, the next piece of
    /// work posted. Whether the window should look again in [`POLL`].
    pub(crate) fn tick(&mut self, app: &mut App, now: Instant) -> bool {
        loop {
            match self.results.try_recv() {
                Ok(result) => {
                    self.busy = false;
                    app.accept(result);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.jobs = None;
                    self.busy = false;
                    break;
                }
            }
        }
        let snapshot = app.snapshot();
        if self.seen != Some(snapshot) {
            self.seen = Some(snapshot);
            self.settles_at = Some(now + SETTLE);
        }
        let settled = self.settles_at.is_none_or(|at| now >= at);
        if settled {
            self.settles_at = None;
        }
        if settled && !self.busy && self.jobs.is_some() {
            self.post_next(app, snapshot);
        }
        self.busy || self.settles_at.is_some() || app.generation().busy()
    }

    /// Posts `app`'s next piece of work, with the snapshot it is for.
    fn post_next(&mut self, app: &mut App, snapshot: SnapshotId) {
        let Some(jobs) = &self.jobs else {
            return;
        };
        let Some(work) = app.next_work() else {
            return;
        };
        let project = match &self.shared {
            Some((at, project)) if *at == snapshot => Arc::clone(project),
            _ => {
                let project = Arc::new(app.project().clone());
                self.shared = Some((snapshot, Arc::clone(&project)));
                project
            }
        };
        if jobs.send(Request { project, work }).is_ok() {
            self.busy = true;
        } else {
            // The thread is gone: nothing more can run.
            self.jobs = None;
        }
    }
}
