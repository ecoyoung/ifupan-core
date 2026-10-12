use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

/// Run `items` on a bounded thread pool with optional stagger between launches.
pub fn run_bounded<T, F>(
    items: Vec<T>,
    max_conc: usize,
    stagger_ms: u64,
    should_stop: impl Fn() -> bool,
    work: F,
) where
    T: Send + 'static,
    F: Fn(T) + Send + Sync + Clone + 'static,
{
    let max_conc = max_conc.max(1);
    let gate = Arc::new(Gate {
        n: Mutex::new(0),
        cap: max_conc,
        cvar: Condvar::new(),
    });
    let mut handles = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        if should_stop() {
            break;
        }
        if i > 0 && stagger_ms > 0 {
            thread::sleep(Duration::from_millis(stagger_ms));
        }
        gate.acquire();
        let gate = gate.clone();
        let work = work.clone();
        handles.push(thread::spawn(move || {
            work(item);
            gate.release();
        }));
    }
    for h in handles {
        let _ = h.join();
    }
}

struct Gate {
    n: Mutex<usize>,
    cap: usize,
    cvar: Condvar,
}

impl Gate {
    fn acquire(&self) {
        let mut n = self.n.lock().unwrap();
        while *n >= self.cap {
            n = self.cvar.wait(n).unwrap();
        }
        *n += 1;
    }

    fn release(&self) {
        let mut n = self.n.lock().unwrap();
        *n = n.saturating_sub(1);
        self.cvar.notify_one();
    }
}

/// Test helper: peak in-flight workers.
pub fn peak_inflight(tasks: usize, max_conc: usize, hold_ms: u64) -> usize {
    let peak = Arc::new(AtomicUsize::new(0));
    let cur = Arc::new(AtomicUsize::new(0));
    let items: Vec<usize> = (0..tasks).collect();
    let peak2 = peak.clone();
    let cur2 = cur.clone();
    run_bounded(items, max_conc, 0, || false, move |_| {
        let n = cur2.fetch_add(1, Ordering::SeqCst) + 1;
        peak2.fetch_max(n, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(hold_ms));
        cur2.fetch_sub(1, Ordering::SeqCst);
    });
    peak.load(Ordering::SeqCst)
}
