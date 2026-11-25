use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;

/// Waits for the thread to be through with the newest request, then takes its answer.
fn answer<I: Send + 'static, O: Send + 'static>(w: &mut Latest<I, O>) -> Option<Result<O, String>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while w.busy() {
        assert!(Instant::now() < deadline, "the worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    w.take()
}

#[test]
fn a_job_is_made_and_answered_once() {
    let mut w = Latest::new(|n: u32, _| Some(n * 2)).expect("a thread");
    assert!(!w.busy() && w.take().is_none(), "nothing asked, nothing answered");
    w.request(21).expect("asked");
    assert_eq!(answer(&mut w), Some(Ok(42)));
    assert!(w.take().is_none(), "an answer is taken once");
}

#[test]
fn a_newer_request_cancels_the_job_and_only_the_newest_is_answered() {
    let (entered_tx, entered) = mpsc::channel::<u32>();
    let (release_tx, release) = mpsc::channel::<()>();
    let mut w = Latest::new(move |n: u32, cancelled| {
        entered_tx.send(n).ok()?;
        if n == 1 {
            release.recv().ok()?;
            // The first job finds out it is late, and publishes nothing.
            return if cancelled() { None } else { Some(n) };
        }
        Some(n)
    })
    .expect("a thread");
    w.request(1).expect("asked");
    assert_eq!(entered.recv_timeout(Duration::from_secs(30)), Ok(1));
    w.request(2).expect("asked");
    w.request(3).expect("asked");
    release_tx.send(()).expect("released");
    assert_eq!(answer(&mut w), Some(Ok(3)), "the latest of what was asked");
    let mut rest = Vec::new();
    while let Ok(n) = entered.try_recv() {
        rest.push(n);
    }
    assert_eq!(rest, [3], "2 was replaced while it waited: it never started");
}

#[test]
fn a_job_that_panics_is_an_error_and_the_next_request_goes_on() {
    let mut w = Latest::new(|n: u32, _| {
        if n == 1 {
            panic!("a job that goes wrong, on purpose");
        }
        Some(n)
    })
    .expect("a thread");
    w.request(1).expect("asked");
    assert!(matches!(answer(&mut w), Some(Err(_))), "a failure is an answer");
    w.request(2).expect("asked");
    assert_eq!(answer(&mut w), Some(Ok(2)), "the thread lives on");
}

#[test]
fn a_job_with_nothing_to_show_is_an_error_and_a_dropped_worker_cancels_its_job() {
    let mut w = Latest::new(|_: u32, _| None::<u32>).expect("a thread");
    w.request(1).expect("asked");
    assert!(matches!(answer(&mut w), Some(Err(_))));

    let (entered_tx, entered) = mpsc::channel::<()>();
    let (told_tx, told) = mpsc::channel::<bool>();
    let mut w = Latest::new(move |_: u32, cancelled| {
        entered_tx.send(()).ok()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !cancelled() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        told_tx.send(cancelled()).ok()?;
        Some(0u32)
    })
    .expect("a thread");
    w.request(1).expect("asked");
    entered.recv_timeout(Duration::from_secs(30)).expect("the job began");
    drop(w);
    assert_eq!(told.recv_timeout(Duration::from_secs(30)), Ok(true), "dropping the worker cancels the job it was making");
}
