use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use crate::{
    Input, MouseButton,
    async_input::{Blocking, Pace, Worker},
};

// What the async input worker runs: any input, or a pause between inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Input(Input),
    Wait(u32),
}

// What async jobs pressed and haven't released yet.
#[derive(Default)]
struct Held {
    buttons: Vec<MouseButton>,
    keys: Vec<u8>,
}

type SendInput = dyn Fn(&Input, &dyn Pace) -> Result<(), String> + Send + Sync;

struct Jobs {
    send: Box<SendInput>,
    held: Mutex<Held>,
}

impl Jobs {
    fn run(&self, job: &Job, pace: &dyn Pace) -> Result<(), String> {
        match job {
            Job::Wait(ms) => {
                pace.sleep(Duration::from_millis(*ms as u64));
                Ok(())
            }
            Job::Input(input) => {
                if pace.cancelled() {
                    return Ok(());
                }
                (self.send)(input, pace)?;
                let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
                match *input {
                    Input::Button { button, down: true } if !held.buttons.contains(&button) => held.buttons.push(button),
                    Input::Button { button, down: false } => held.buttons.retain(|b| *b != button),
                    Input::Key { key, down: true } if !held.keys.contains(&key) => held.keys.push(key),
                    Input::Key { key, down: false } => held.keys.retain(|k| *k != key),
                    _ => {}
                }
                Ok(())
            }
        }
    }

    // Lets go of the buttons and keys jobs left pressed, the last one pressed first.
    fn release(&self) {
        let held = std::mem::take(&mut *self.held.lock().unwrap_or_else(PoisonError::into_inner));
        for key in held.keys.into_iter().rev() {
            let _ = (self.send)(&Input::Key { key, down: false }, &Blocking);
        }
        for button in held.buttons.into_iter().rev() {
            let _ = (self.send)(&Input::Button { button, down: false }, &Blocking);
        }
    }
}

// A worker for `send`. When it is cancelled or shut down it releases every button and key its jobs left held.
pub fn worker(send: impl Fn(&Input, &dyn Pace) -> Result<(), String> + Send + Sync + 'static) -> Worker<Job> {
    let jobs = Arc::new(Jobs { send: Box::new(send), held: Mutex::default() });
    let release = jobs.clone();
    Worker::new(move |job, pace| jobs.run(job, pace), move || release.release())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn recorded() -> (Arc<Mutex<Vec<Input>>>, Worker<Job>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker = worker({
            let log = log.clone();
            move |input, _| {
                log.lock().unwrap().push(input.clone());
                Ok(())
            }
        });
        (log, worker)
    }

    fn key(key: u8, down: bool) -> Job {
        Job::Input(Input::Key { key, down })
    }

    #[test]
    fn cancelling_releases_what_is_held_last_first() {
        let (log, worker) = recorded();
        worker.push(key(16, true)).unwrap();
        worker.push(key(37, true)).unwrap();
        worker.push(Job::Input(Input::Button { button: MouseButton::Middle, down: true })).unwrap();
        worker.push(Job::Wait(10_000)).unwrap();
        while log.lock().unwrap().len() < 3 {
            std::thread::yield_now();
        }

        let start = Instant::now();
        worker.cancel();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(
            log.lock().unwrap()[3..],
            [
                Input::Key { key: 37, down: false },
                Input::Key { key: 16, down: false },
                Input::Button { button: MouseButton::Middle, down: false },
            ]
        );

        // Nothing is held any more.
        worker.cancel();
        assert_eq!(log.lock().unwrap().len(), 6);
    }

    #[test]
    fn released_input_is_not_released_again() {
        let (log, worker) = recorded();
        worker.push(key(37, true)).unwrap();
        worker.push(key(37, false)).unwrap();
        worker.wait();
        worker.cancel();
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[test]
    fn shutdown_releases_too() {
        let (log, worker) = recorded();
        worker.push(key(40, true)).unwrap();
        worker.wait();
        worker.shutdown();
        assert_eq!(log.lock().unwrap().last(), Some(&Input::Key { key: 40, down: false }));
    }

    #[test]
    fn wait_jobs_pause_the_queue() {
        let (log, worker) = recorded();
        let start = Instant::now();
        worker.push(key(37, true)).unwrap();
        worker.push(Job::Wait(80)).unwrap();
        worker.push(key(37, false)).unwrap();
        worker.wait();
        assert!(start.elapsed() >= Duration::from_millis(80));
        assert_eq!(log.lock().unwrap().len(), 2);
    }
}
