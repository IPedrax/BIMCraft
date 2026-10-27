//! Bounded background view jobs. A view owns one running input, one latest pending
//! input and one completed result. Superseded generations never reach the caller.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak, mpsc};

type Task = Box<dyn FnOnce() + Send>;

/// Cooperative cancellation; identity also names a generation across counter wrap/undo.
#[derive(Clone, Default)]
pub(crate) struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub(crate) fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

struct Pool {
    send: mpsc::SyncSender<Task>,
}

impl Pool {
    fn shared() -> Result<&'static Self, String> {
        static POOL: OnceLock<Result<Pool, String>> = OnceLock::new();
        POOL.get_or_init(|| {
            let (send, receive) = mpsc::sync_channel::<Task>(32);
            let receive = Arc::new(Mutex::new(receive));
            let mut started = 0;
            for n in 0..2 {
                let receive = Arc::clone(&receive);
                if std::thread::Builder::new()
                    .name(format!("bimcraft-view-{n}"))
                    .spawn(move || {
                        loop {
                            let task = match receive.lock() {
                                Ok(r) => r.recv(),
                                Err(_) => return,
                            };
                            match task {
                                Ok(task) => task(),
                                Err(_) => return,
                            }
                        }
                    })
                    .is_ok()
                {
                    started += 1;
                }
            }
            if started == 0 { Err("could not start a view worker".into()) } else { Ok(Pool { send }) }
        })
        .as_ref()
        .map_err(Clone::clone)
    }
}

struct Request<I> {
    input: I,
    token: Cancel,
}
type Render<I, O> = Box<dyn FnMut(I, &Cancel) -> Option<O> + Send>;

struct State<I, O> {
    pending: Option<Request<I>>,
    current: Cancel,
    ready: Option<O>,
    error: Option<String>,
    running: bool,
    render: Option<Render<I, O>>,
}

pub(crate) struct Latest<I: Send + 'static, O: Send + 'static> {
    state: Arc<Mutex<State<I, O>>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl<I: Send + 'static, O: Send + 'static> Latest<I, O> {
    pub(crate) fn new(render: impl FnMut(I, &Cancel) -> Option<O> + Send + 'static, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                pending: None,
                current: Cancel::default(),
                ready: None,
                error: None,
                running: false,
                render: Some(Box::new(render)),
            })),
            wake: Arc::new(wake),
        }
    }

    /// Replaces pending work immediately. A full shared queue retains only the latest
    /// input here; `start` retries on a later UI frame without blocking input handling.
    pub(crate) fn request(&self, input: I) {
        if let Ok(mut s) = self.state.lock() {
            s.current.cancel();
            s.current = Cancel::default();
            s.pending = Some(Request { input, token: s.current.clone() });
            s.ready = None;
            s.error = None;
        }
    }

    pub(crate) fn take_error(&self) -> Option<String> {
        self.state.lock().ok()?.error.take()
    }

    pub(crate) fn busy(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.running || s.pending.is_some())
    }

    pub(crate) fn take(&self) -> Option<O> {
        self.state.lock().ok()?.ready.take()
    }

    pub(crate) fn start(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "view worker state is unavailable")?;
        if state.running || state.pending.is_none() {
            return Ok(());
        }
        let pool = Pool::shared()?;
        state.running = true;
        let weak = Arc::downgrade(&self.state);
        let wake = Arc::clone(&self.wake);
        match pool.send.try_send(Box::new(move || Self::run(weak, wake))) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                state.running = false;
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                state.running = false;
                Err("view workers have stopped".into())
            }
        }
    }

    fn run(weak: Weak<Mutex<State<I, O>>>, wake: Arc<dyn Fn() + Send + Sync>) {
        let Some(state) = weak.upgrade() else { return };
        let Some(mut render) = (match state.lock() {
            Ok(mut s) => s.render.take(),
            Err(_) => return,
        }) else {
            return;
        };
        loop {
            let job = match state.lock() {
                Ok(mut s) => match s.pending.take() {
                    Some(job) => job,
                    None => {
                        s.render = Some(render);
                        s.running = false;
                        drop(s);
                        wake();
                        return;
                    }
                },
                Err(_) => return,
            };
            // Catch a bad renderer outside the mailbox lock so later requests remain usable.
            let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(job.input, &job.token)));
            if let Ok(mut s) = state.lock()
                && !job.token.cancelled()
                && job.token.same(&s.current)
            {
                match output {
                    Ok(Some(output)) => s.ready = Some(output),
                    Ok(None) => s.error = Some("view rendering did not finish".into()),
                    Err(_) => s.error = Some("view rendering failed".into()),
                }
            }
            wake();
        }
    }
}

impl<I: Send + 'static, O: Send + 'static> Drop for Latest<I, O> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.current.cancel();
            s.pending = None;
            s.ready = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use web_time::Duration;

    #[test]
    fn superseded_work_is_cancelled_and_only_the_latest_input_is_rendered_next() {
        let (entered, enter) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (done, completion) = mpsc::channel();
        let worker = Latest::new(
            move |input, cancel| {
                entered.send(input).unwrap();
                if input == 1 {
                    wait.recv().unwrap();
                    assert!(cancel.cancelled());
                }
                Some(input)
            },
            move || {
                let _ = done.send(());
            },
        );
        worker.request(1);
        worker.start().unwrap();
        assert_eq!(enter.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        worker.request(2);
        worker.request(3);
        release.send(()).unwrap();
        assert_eq!(enter.recv_timeout(Duration::from_secs(5)).unwrap(), 3);
        for _ in 0..3 {
            completion.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some(result) = worker.take() {
                assert_eq!(result, 3);
                return;
            }
        }
        panic!("latest result was not published");
    }

    #[test]
    fn a_failed_renderer_reports_an_error_and_can_process_the_next_request() {
        let (done, completion) = mpsc::channel();
        let worker = Latest::new(
            |input, _| {
                if input == 1 {
                    panic!("test renderer failure");
                }
                Some(input)
            },
            move || {
                let _ = done.send(());
            },
        );
        worker.request(1);
        worker.start().unwrap();
        completion.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(worker.take_error().is_some());
        assert!(worker.take().is_none());
        worker.request(2);
        worker.start().unwrap();
        for _ in 0..5 {
            completion.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some(result) = worker.take() {
                assert_eq!(result, 2);
                return;
            }
        }
        panic!("renderer did not recover");
    }

    #[test]
    fn dropping_a_view_cancels_its_running_job_and_releases_its_pending_input() {
        let (entered, enter) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (done, completion) = mpsc::channel();
        let pending = Arc::new(());
        let worker = Latest::new(
            move |_: Arc<()>, cancel| {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                done.send(cancel.cancelled()).unwrap();
                Some(())
            },
            || {},
        );
        worker.request(Arc::new(()));
        worker.start().unwrap();
        enter.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.request(Arc::clone(&pending));
        drop(worker);
        assert_eq!(Arc::strong_count(&pending), 1);
        release.send(()).unwrap();
        assert!(completion.recv_timeout(Duration::from_secs(5)).unwrap());
    }
}
