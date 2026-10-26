use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    thread::{self, JoinHandle},
    time::Duration,
};

// What input code sleeps and checks through, so it runs blocking or as a cancellable `Worker` job.
pub trait Pace {
    // True once the job was cancelled (or the worker is shutting down): finish what is held, post nothing new.
    fn cancelled(&self) -> bool;

    // Sleeps for `duration`, or less if cancelled. Returns false when cancelled.
    fn sleep(&self, duration: Duration) -> bool;
}

pub struct Blocking;

impl Pace for Blocking {
    fn cancelled(&self) -> bool {
        false
    }

    fn sleep(&self, duration: Duration) -> bool {
        thread::sleep(duration);
        true
    }
}

struct State<J> {
    queue: VecDeque<J>,
    busy: bool,
    // Bumped by `cancel`; a running job is cancelled when this no longer matches what it started under.
    generation: u64,
    closed: bool,
    error: Option<String>,
}

struct Shared<J> {
    state: Mutex<State<J>>,
    changed: Condvar,
}

impl<J> Shared<J> {
    fn lock(&self) -> MutexGuard<'_, State<J>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Task<'a, J> {
    shared: &'a Shared<J>,
    generation: u64,
}

impl<J> Pace for Task<'_, J> {
    fn cancelled(&self) -> bool {
        let state = self.shared.lock();
        state.closed || state.generation != self.generation
    }

    fn sleep(&self, duration: Duration) -> bool {
        let state = self.shared.lock();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, duration, |s| !s.closed && s.generation == self.generation)
            .unwrap_or_else(PoisonError::into_inner);
        !state.closed && state.generation == self.generation
    }
}

// One thread running queued jobs in order; `cancel` drops the queue, stops the running job and returns once idle.
pub struct Worker<J> {
    shared: Arc<Shared<J>>,
    release: Box<dyn Fn() + Send + Sync>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl<J: Send + 'static> Worker<J> {
    // `release` lets go of what jobs left held, on the caller's thread once the worker is idle, at every `cancel` and `shutdown`.
    pub fn new(
        run: impl Fn(&J, &dyn Pace) -> Result<(), String> + Send + 'static,
        release: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { queue: VecDeque::new(), busy: false, generation: 0, closed: false, error: None }),
            changed: Condvar::new(),
        });
        let thread = thread::spawn({
            let shared = shared.clone();
            move || work(&shared, run)
        });
        Self { shared, release: Box::new(release), thread: Mutex::new(Some(thread)) }
    }
}

impl<J> Worker<J> {
    pub fn push(&self, job: J) -> Result<(), String> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err("input worker is stopped".into());
        }
        state.queue.push_back(job);
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn cancel(&self) {
        let mut state = self.shared.lock();
        state.generation += 1;
        state.queue.clear();
        self.shared.changed.notify_all();
        drop(self.shared.changed.wait_while(state, |s| s.busy).unwrap_or_else(PoisonError::into_inner));
        (self.release)();
    }

    pub fn busy(&self) -> bool {
        let state = self.shared.lock();
        state.busy || !state.queue.is_empty()
    }

    pub fn wait(&self) {
        let state = self.shared.lock();
        drop(self.shared.changed.wait_while(state, |s| s.busy || !s.queue.is_empty()).unwrap_or_else(PoisonError::into_inner));
    }

    // The error of the last job that failed, once.
    pub fn take_error(&self) -> Option<String> {
        self.shared.lock().error.take()
    }

    pub fn shutdown(&self) {
        {
            let mut state = self.shared.lock();
            state.closed = true;
            state.queue.clear();
            self.shared.changed.notify_all();
        }
        let thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(thread) = thread {
            let _ = thread.join();
            (self.release)();
        }
    }
}

impl<J> Drop for Worker<J> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn work<J>(shared: &Shared<J>, run: impl Fn(&J, &dyn Pace) -> Result<(), String>) {
    let mut state = shared.lock();
    loop {
        state = shared.changed.wait_while(state, |s| s.queue.is_empty() && !s.closed).unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return;
        }
        let Some(job) = state.queue.pop_front() else { continue };
        state.busy = true;
        let task = Task { shared, generation: state.generation };
        drop(state);

        let result = catch_unwind(AssertUnwindSafe(|| run(&job, &task))).unwrap_or_else(|_| Err("input job panicked".into()));

        state = shared.lock();
        state.busy = false;
        if let Err(error) = result {
            state.error = Some(error);
        }
        shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU32, Ordering},
        time::Instant,
    };

    use super::*;

    #[test]
    fn runs_jobs_in_order() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let worker = Worker::new({
            let seen = seen.clone();
            move |job: &u32, _| {
                seen.lock().unwrap().push(*job);
                Ok(())
            }
        }, || {});
        for i in 0..5 {
            worker.push(i).unwrap();
        }
        worker.wait();
        assert_eq!(*seen.lock().unwrap(), vec![0, 1, 2, 3, 4]);
        assert!(!worker.busy());
    }

    #[test]
    fn cancel_interrupts_a_sleep_and_drops_the_queue() {
        let ran = Arc::new(AtomicU32::new(0));
        let cleaned = Arc::new(AtomicU32::new(0));
        let worker = Worker::new({
            let (ran, cleaned) = (ran.clone(), cleaned.clone());
            move |_: &u32, pace| {
                ran.fetch_add(1, Ordering::SeqCst);
                // Like a click: the button goes down, the hold is cut short, the button still goes up.
                pace.sleep(Duration::from_secs(10));
                cleaned.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }, || {});
        worker.push(0).unwrap();
        worker.push(1).unwrap();
        while ran.load(Ordering::SeqCst) == 0 {
            thread::yield_now();
        }

        let start = Instant::now();
        worker.cancel();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!((ran.load(Ordering::SeqCst), cleaned.load(Ordering::SeqCst)), (1, 1));
        assert!(!worker.busy());

        // The worker takes new jobs after a cancel.
        worker.push(2).unwrap();
        while ran.load(Ordering::SeqCst) < 2 {
            thread::yield_now();
        }
        worker.cancel();
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn errors_are_kept_until_taken() {
        let worker = Worker::new(|_: &u32, _| Err("no window".into()), || {});
        worker.push(0).unwrap();
        worker.wait();
        assert_eq!(worker.take_error(), Some("no window".into()));
        assert_eq!(worker.take_error(), None);
    }

    #[test]
    fn a_panicking_job_does_not_wedge_cancel() {
        let worker = Worker::new(|_: &u32, _| panic!("boom"), || {});
        worker.push(0).unwrap();
        worker.wait();
        worker.cancel();
        assert_eq!(worker.take_error(), Some("input job panicked".into()));
    }

    #[test]
    fn shutdown_stops_a_running_job_and_refuses_more() {
        let worker = Worker::new(
            |_: &u32, pace| {
                pace.sleep(Duration::from_secs(10));
                Ok(())
            },
            || {},
        );
        worker.push(0).unwrap();
        thread::sleep(Duration::from_millis(20));
        let start = Instant::now();
        worker.shutdown();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(worker.push(1).is_err());
    }
}
