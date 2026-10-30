//! Threads that draw the sprites' frames. Each sprite starts its frame while the game updates
//! (`process`) and takes it when Godot asks it to draw, so many characters are drawn at once on
//! all cores instead of one after another.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel, sync_channel};
use std::sync::{Mutex, OnceLock};

type Job = Box<dyn FnOnce() + Send>;

fn queue() -> &'static Mutex<Sender<Job>> {
    static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let rx = std::sync::Arc::new(Mutex::new(rx));
        let n = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8);
        for i in 0..n {
            let rx = rx.clone();
            let _ = std::thread::Builder::new().name(format!("motif-draw-{i}")).spawn(move || {
                loop {
                    let job = match rx.lock() {
                        Ok(rx) => rx.recv(),
                        Err(_) => return,
                    };
                    match job {
                        // A frame that fails is a frame missed, not a thread lost.
                        Ok(job) => drop(std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))),
                        Err(_) => return,
                    }
                }
            });
        }
        Mutex::new(tx)
    })
}

/// Frames started and not yet taken.
static STARTED: AtomicUsize = AtomicUsize::new(0);

/// A frame being drawn: [`Pending::wait`] gives it.
pub struct Pending<T> {
    how: How<T>,
}

enum How<T> {
    /// Done by the thread that takes it: the main thread's share of the work.
    Here(Box<dyn FnOnce() -> T + Send>),
    There(Receiver<T>),
}

impl<T> Pending<T> {
    pub fn wait(self) -> Option<T> {
        // `self` is dropped only after the work, so the batch counts it until then.
        let mut this = self;
        match std::mem::replace(&mut this.how, How::There(sync_channel(0).1)) {
            How::Here(work) => Some(work()),
            How::There(done) => done.recv().ok(),
        }
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        STARTED.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Starts `work` (drawing a frame). The first of a batch is left for the thread that will take
/// it, which would only wait otherwise: a lone character is drawn just as without threads.
pub fn start<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    if STARTED.fetch_add(1, Ordering::Relaxed) == 0 {
        return Pending { how: How::Here(Box::new(work)) };
    }
    let (tx, rx) = sync_channel(1);
    let job: Job = Box::new(move || {
        let _ = tx.send(work());
    });
    let left = match queue().lock() {
        Ok(q) => q.send(job).err().map(|e| e.0),
        Err(_) => Some(job),
    };
    if let Some(job) = left {
        job();
    }
    Pending { how: How::There(rx) }
}
