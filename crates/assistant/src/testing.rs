//! What the assistant's tests stand in for the network with, and the desktop's tests too: the crate's `testing` feature turns
//! this on for them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::net::{Net, Request};

/// A network that answers from a queue, one reply for each request, and remembers what it was sent.
#[derive(Default)]
pub struct Fake {
    replies: Mutex<VecDeque<Result<(u16, String), String>>>,
    seen: Mutex<Vec<Request>>,
}

impl Fake {
    /// The next request is answered with `status` and `body`.
    pub fn reply(self, status: u16, body: impl Into<String>) -> Fake {
        self.replies.lock().unwrap().push_back(Ok((status, body.into())));
        self
    }

    /// The next request fails as a dropped connection does.
    pub fn fail(self, error: &str) -> Fake {
        self.replies.lock().unwrap().push_back(Err(error.to_string()));
        self
    }

    /// The requests so far, in order.
    pub fn seen(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }
}

impl Net for Fake {
    fn post(&self, request: &Request) -> Result<(u16, String), String> {
        self.seen.lock().unwrap().push(request.clone());
        self.replies.lock().unwrap().pop_front().unwrap_or_else(|| Err("the fake network has no more replies".into()))
    }
}
