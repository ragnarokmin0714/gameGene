//! Running a scan on a background thread.
//!
//! A first/next scan over a multi-GB game takes seconds. Doing it inline froze
//! the whole window — no progress, no cancel, and frozen table entries stopped
//! being re-written. [`ScanJob`] moves the work to a worker thread: the UI keeps
//! painting (progress bar + cancel), and freezing keeps ticking, because the
//! engine only borrows the process through a shared [`Arc`] the worker owns too.

use gamegene_core::scan::{Compare, ScanControl, ScanSession};
use gamegene_core::value::ValueType;
use gamegene_core::{MemorySource, ScanError};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Which flavor of scan a job is running (picks the progress label).
#[derive(Clone, Copy, PartialEq)]
pub enum JobKind {
    First,
    Next,
}

/// The finished work handed back from the worker thread.
pub enum JobDone {
    First(Result<ScanSession, ScanError>),
    /// The session is returned (mutated in place) alongside the narrow result,
    /// so the app can put it back whether or not narrowing errored.
    Next {
        session: Box<ScanSession>,
        result: Result<(), ScanError>,
    },
}

/// A scan in flight on a background thread.
pub struct ScanJob {
    pub kind: JobKind,
    control: Arc<ScanControl>,
    rx: Receiver<JobDone>,
    cancelling: bool,
    // Kept so the thread is joined on drop rather than detached and leaked.
    handle: Option<JoinHandle<()>>,
}

impl ScanJob {
    /// Spawn a first scan.
    pub fn first(
        source: Arc<dyn MemorySource>,
        value_type: ValueType,
        compare: Compare,
    ) -> ScanJob {
        let control = Arc::new(ScanControl::new());
        let (tx, rx) = channel();
        let c = control.clone();
        let handle = std::thread::spawn(move || {
            let done = JobDone::First(ScanSession::first_scan_with(
                &*source, value_type, compare, &c,
            ));
            let _ = tx.send(done); // receiver may be gone if the app closed
        });
        ScanJob {
            kind: JobKind::First,
            control,
            rx,
            cancelling: false,
            handle: Some(handle),
        }
    }

    /// Spawn a narrowing (next) scan over an existing session.
    pub fn next(
        source: Arc<dyn MemorySource>,
        mut session: Box<ScanSession>,
        compare: Compare,
    ) -> ScanJob {
        let control = Arc::new(ScanControl::new());
        let (tx, rx) = channel();
        let c = control.clone();
        let handle = std::thread::spawn(move || {
            let result = session.next_scan_with(&*source, compare, &c);
            let _ = tx.send(JobDone::Next { session, result });
        });
        ScanJob {
            kind: JobKind::Next,
            control,
            rx,
            cancelling: false,
            handle: Some(handle),
        }
    }

    /// `(bytes_scanned, bytes_total)`; total is 0 until the engine sets it.
    pub fn progress(&self) -> (u64, u64) {
        self.control.progress()
    }

    /// Fraction scanned in `0.0..=1.0`, or `None` before the total is known.
    pub fn fraction(&self) -> Option<f32> {
        let (done, total) = self.progress();
        (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0))
    }

    /// Ask the scan to stop; the job still needs to be polled until it returns.
    pub fn request_cancel(&mut self) {
        self.control.request_cancel();
        self.cancelling = true;
    }

    pub fn is_cancelling(&self) -> bool {
        self.cancelling
    }

    /// Take the result if the worker has finished, else `None` (still running).
    pub fn poll(&mut self) -> Option<JobDone> {
        match self.rx.try_recv() {
            Ok(done) => {
                if let Some(h) = self.handle.take() {
                    let _ = h.join();
                }
                Some(done)
            }
            Err(_) => None,
        }
    }
}
