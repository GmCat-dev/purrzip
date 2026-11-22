//! A small worker pool that hands results back in the order the jobs were given,
//! with a cap on how many jobs are in flight (so memory stays bounded).

use std::collections::BTreeMap;
use std::sync::mpsc::{channel, sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{Scope, ScopedJoinHandle};
use std::time::Duration;

/// Receives results (in job order). `idle` runs on every wake-up while waiting, for progress and stop checks.
pub trait Deliver<R, E> {
    fn deliver(&mut self, result: R) -> Result<(), E>;
    fn idle(&mut self) -> Result<(), E> {
        Ok(())
    }
}

pub struct Pool<'scope, J: Send + 'scope, R: Send + 'scope> {
    tx: Option<SyncSender<(u64, J)>>,
    rx: Receiver<(u64, R)>,
    next_submit: u64,
    next_deliver: u64,
    ready: BTreeMap<u64, R>,
    window: u64,
    handles: Vec<ScopedJoinHandle<'scope, ()>>,
}

impl<'scope, J: Send + 'scope, R: Send + 'scope> Pool<'scope, J, R> {
    /// `threads` workers; at most `window` jobs submitted but not yet delivered.
    pub fn new<'env, F>(scope: &'scope Scope<'scope, 'env>, threads: usize, window: usize, work: F) -> Self
    where
        F: Fn(J) -> R + Send + Sync + 'scope,
    {
        let threads = threads.max(1);
        let (tx, jobs) = sync_channel::<(u64, J)>(threads);
        let jobs = Arc::new(Mutex::new(jobs));
        let (done_tx, rx) = channel::<(u64, R)>();
        let work = Arc::new(work);
        let mut handles = Vec::new();
        for _ in 0..threads {
            let (jobs, done_tx, work) = (jobs.clone(), done_tx.clone(), work.clone());
            handles.push(scope.spawn(move || loop {
                let next = { jobs.lock().unwrap().recv() };
                match next {
                    Ok((seq, job)) => {
                        if done_tx.send((seq, work(job))).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }));
        }
        Pool { tx: Some(tx), rx, next_submit: 0, next_deliver: 0, ready: BTreeMap::new(), window: window.max(1) as u64, handles }
    }

    pub fn in_flight(&self) -> u64 {
        self.next_submit - self.next_deliver
    }

    fn deliver_ready<E, D: Deliver<R, E> + ?Sized>(&mut self, d: &mut D) -> Result<(), E> {
        while let Some(r) = self.ready.remove(&self.next_deliver) {
            self.next_deliver += 1;
            d.deliver(r)?;
        }
        Ok(())
    }

    /// Wait up to 150 ms for one result.
    fn wait_one<E, D: Deliver<R, E> + ?Sized>(&mut self, d: &mut D) -> Result<(), E> {
        match self.rx.recv_timeout(Duration::from_millis(150)) {
            Ok((seq, r)) => {
                self.ready.insert(seq, r);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("a Purrzip worker stopped unexpectedly"),
        }
        d.idle()?;
        self.deliver_ready(d)
    }

    /// Queue a job. Blocks (delivering finished results in order) while the window is full.
    pub fn submit<E, D: Deliver<R, E> + ?Sized>(&mut self, job: J, d: &mut D) -> Result<(), E> {
        while self.in_flight() >= self.window {
            self.wait_one(d)?;
        }
        let mut item = (self.next_submit, job);
        loop {
            match self.tx.as_ref().unwrap().try_send(item) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) => {
                    item = back;
                    self.wait_one(d)?;
                }
                Err(TrySendError::Disconnected(_)) => panic!("Purrzip workers are gone"),
            }
        }
        self.next_submit += 1;
        // Take whatever is already finished, without waiting.
        while let Ok((seq, r)) = self.rx.try_recv() {
            self.ready.insert(seq, r);
        }
        self.deliver_ready(d)
    }

    /// Wait for every queued job and deliver the rest.
    pub fn drain<E, D: Deliver<R, E> + ?Sized>(&mut self, d: &mut D) -> Result<(), E> {
        while self.in_flight() > 0 {
            self.wait_one(d)?;
        }
        Ok(())
    }
}

impl<'scope, J: Send + 'scope, R: Send + 'scope> Drop for Pool<'scope, J, R> {
    fn drop(&mut self) {
        self.tx = None; // workers see the closed queue and stop
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Collect {
        got: Vec<u64>,
        fail_on: Option<u64>,
    }
    impl Deliver<u64, &'static str> for Collect {
        fn deliver(&mut self, r: u64) -> Result<(), &'static str> {
            if Some(r) == self.fail_on {
                return Err("stop");
            }
            self.got.push(r);
            Ok(())
        }
    }

    #[test]
    fn results_come_back_in_order() {
        std::thread::scope(|s| {
            let mut pool = Pool::new(s, 4, 6, |n: u64| {
                // later jobs finish sooner
                std::thread::sleep(Duration::from_millis(20u64.saturating_sub(n)));
                n * 2
            });
            let mut c = Collect { got: Vec::new(), fail_on: None };
            for n in 0..40u64 {
                pool.submit(n, &mut c).unwrap();
                assert!(pool.in_flight() <= 6);
            }
            pool.drain(&mut c).unwrap();
            assert_eq!(c.got, (0..40u64).map(|n| n * 2).collect::<Vec<_>>());
        });
    }

    #[test]
    fn errors_stop_cleanly() {
        std::thread::scope(|s| {
            let mut pool = Pool::new(s, 2, 3, |n: u64| n);
            let mut c = Collect { got: Vec::new(), fail_on: Some(5) };
            let mut r = Ok(());
            for n in 0..20u64 {
                r = pool.submit(n, &mut c);
                if r.is_err() {
                    break;
                }
            }
            if r.is_ok() {
                r = pool.drain(&mut c);
            }
            assert_eq!(r, Err("stop"));
        }); // dropping the pool with jobs still queued must not hang
    }
}
