//! Threads that draw the sprites' frames. Each sprite starts its frame while the game updates
//! (`process`) and takes it when Godot asks it to draw, so many characters are drawn at once on
//! all cores instead of one after another.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel, sync_channel};
use std::sync::{Mutex, OnceLock};
use tiny_skia::Pixmap;

use crate::player::Frame;

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

/// A frame being drawn: [`Pending::wait`] gives the picture.
pub struct Pending {
    how: How,
}

enum How {
    /// Drawn by the thread that takes it: the main thread's share of the work.
    Here(Frame, f64, bool),
    There(Receiver<Option<Pixmap>>),
}

impl Pending {
    pub fn wait(self) -> Option<Pixmap> {
        match &self.how {
            How::Here(frame, scale, background) => frame.render(*scale, *background),
            How::There(picture) => picture.recv().ok().flatten(),
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        STARTED.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Starts drawing `frame`. The first frame of a batch is left for the thread that will take
/// it, which would only wait otherwise: a lone character is drawn just as without threads.
pub fn draw(frame: Frame, scale: f64, background: bool) -> Pending {
    if STARTED.fetch_add(1, Ordering::Relaxed) == 0 {
        return Pending { how: How::Here(frame, scale, background) };
    }
    let (tx, rx) = sync_channel(1);
    let job: Job = Box::new(move || {
        let _ = tx.send(frame.render(scale, background));
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
