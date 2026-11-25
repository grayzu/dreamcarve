// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The simulation worker (`docs/specs/phase-6/design.md`, "The worker";
//! functional spec, capability 123): a second thread, beside the
//! generation worker's, that runs `dc_app::sim::execute`, so a long
//! simulation never holds up a toolpath and neither holds up the window.
//!
//! The shell posts one job at a time, and only while the Simulate step
//! is open and generation has nothing left to do: `App::next_sim_job`
//! says when. A job can be told to stop, which the generation worker's
//! cannot: the engine asks between chunks of work, and the answer is an
//! atomic flag this side sets when the job is no longer the one wanted
//! (the project changed, or Quality did) or the user pressed Stop. The
//! thread sends its progress as it goes; like the generation worker it
//! never wakes the window, which polls while a job is out.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use dc_app::App;
use dc_app::sim::{self, SimJob, SimResult};
use dc_job::sim::Progress;
use dc_job::{EmitOp, EstimateConfig};
use dc_project::Project;

/// One simulation to make, with what it is made of.
struct Request {
    project: Arc<Project>,
    generated: Vec<EmitOp>,
    job: SimJob,
}

/// What the thread sends back.
enum Reply {
    /// How far the job has got.
    Progress(Progress),
    /// What it came to.
    Done(SimResult),
}

/// The simulation thread and its channels.
pub(crate) struct SimWorker {
    /// `None` when the thread could not start or has stopped.
    jobs: Option<Sender<Request>>,
    replies: Receiver<Reply>,
    /// Set to stop the job that is out; cleared as the next is posted.
    stop: Arc<AtomicBool>,
    /// Whether a job is out and unanswered.
    busy: bool,
}

impl std::fmt::Debug for SimWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimWorker")
            .field("running", &self.jobs.is_some())
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

impl SimWorker {
    /// Starts the thread; `estimate` is the user's `[estimate]`
    /// settings, which give the moves their times. A thread that cannot
    /// start leaves a worker that runs nothing, and the error says why.
    pub(crate) fn spawn(estimate: EstimateConfig) -> (Self, Option<std::io::Error>) {
        let (jobs, inbox) = mpsc::channel::<Request>();
        let (outbox, replies) = mpsc::channel::<Reply>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let started = thread::Builder::new()
            .name("simulation".into())
            .spawn(move || {
                for request in inbox {
                    let Request {
                        project,
                        generated,
                        job,
                    } = request;
                    let result =
                        sim::execute(&project, &generated, &job, &estimate, &mut |progress| {
                            // A window that has gone is a reason to stop.
                            outbox.send(Reply::Progress(progress)).is_ok()
                                && !stopped.load(Ordering::Relaxed)
                        });
                    if outbox.send(Reply::Done(result)).is_err() {
                        break;
                    }
                }
            });
        let error = started.err();
        let worker = Self {
            jobs: error.is_none().then_some(jobs),
            replies,
            stop,
            busy: false,
        };
        (worker, error)
    }

    /// Whether a job is out: the thread is making a simulation.
    pub(crate) const fn busy(&self) -> bool {
        self.busy
    }

    /// One frame's turn: the progress and the result that came back
    /// handed to `app`; the job that is out told to stop when `app` no
    /// longer wants it; and, with the Simulate step `open` and the
    /// thread free, the next job posted. Whether the window should look
    /// again soon.
    pub(crate) fn tick(&mut self, app: &mut App, open: bool) -> bool {
        loop {
            match self.replies.try_recv() {
                Ok(Reply::Progress(progress)) => app.sim_progressed(progress),
                Ok(Reply::Done(result)) => {
                    self.busy = false;
                    app.accept_sim(result);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.jobs = None;
                    self.busy = false;
                    break;
                }
            }
        }
        if self.busy && app.sim_wants_stop() {
            self.stop.store(true, Ordering::Relaxed);
        }
        if open
            && !self.busy
            && let Some(jobs) = &self.jobs
            && let Some((job, generated)) = app.next_sim_job()
        {
            self.stop.store(false, Ordering::Relaxed);
            let request = Request {
                project: Arc::new(app.project().clone()),
                generated,
                job,
            };
            if jobs.send(request).is_ok() {
                self.busy = true;
            } else {
                // The thread is gone: nothing more can run.
                self.jobs = None;
            }
        }
        self.busy
    }
}
