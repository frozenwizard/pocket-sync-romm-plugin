//! What the plugin uses of Pocket Sync, behind a trait so tests can stand in for it.

#[cfg(test)]
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use extism_pdk::json;
use serde::Serialize;

/// An HTTP request.
#[derive(Clone)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<Vec<u8>>,
}

impl Request {
    /// A GET request for `url`.
    pub fn get(url: String) -> Self {
        Request {
            method: "GET",
            url,
            headers: Vec::new(),
            body: None,
        }
    }

    /// A DELETE request for `url`.
    pub fn delete(url: String) -> Self {
        Request {
            method: "DELETE",
            url,
            headers: Vec::new(),
            body: None,
        }
    }

    /// A POST request sending `body` to `url` as JSON.
    pub fn post_json(url: String, body: &impl Serialize) -> Self {
        Request {
            method: "POST",
            url,
            headers: vec![("Content-Type", "application/json".into())],
            body: Some(json::to_vec(body).expect("request bodies serialize")),
        }
    }

    /// This request, sending `token` as `Authorization: Bearer …`.
    pub fn with_bearer(mut self, token: &str) -> Self {
        self.headers
            .push(("Authorization", format!("Bearer {token}")));
        self
    }
}

#[cfg(test)]
impl Request {
    /// The JSON body sent.
    pub fn json(&self) -> json::Value {
        json::from_slice(self.body.as_ref().expect("a body was sent")).unwrap()
    }
}

/// An HTTP response.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Pocket Sync, the plugin's host: its log, the network and the browser.
pub trait PocketSync {
    /// Writes a line to the log Pocket Sync shows.
    fn log(&mut self, line: &str);

    /// Sends `request`. When no response arrives (a refused connection, an unknown server, a bad
    /// certificate), Extism ends the plugin's call instead of returning an error.
    fn send(&mut self, request: &Request) -> Result<Response, String>;

    /// Opens `url` in the user's browser. When the browser can't be opened, Pocket Sync ends the
    /// plugin's call.
    fn open_url(&mut self, url: &str);

    /// Waits for `duration`.
    fn wait(&mut self, duration: Duration);
}

/// A stand-in for Pocket Sync that keeps the log, answers requests from a list, and doesn't wait.
#[cfg(test)]
#[derive(Default)]
pub struct FakePocketSync {
    pub lines: Vec<String>,
    /// Every request sent, in order.
    pub sent: Vec<Request>,
    /// Every URL opened in the browser, in order.
    pub opened: Vec<String>,
    /// How long the plugin has waited, in all.
    pub waited: Duration,
    /// The answers still to give, by method and URL.
    responses: HashMap<(&'static str, String), VecDeque<Result<Response, String>>>,
}

#[cfg(test)]
impl FakePocketSync {
    /// Answers `method` requests to `url` with `status` and `body`. Called again for the same
    /// request, it queues an answer for the next one; the last answer queued is given from then on.
    pub fn respond(&mut self, method: &'static str, url: &str, status: u16, body: &str) {
        let response = Response {
            status,
            body: body.into(),
        };
        self.queue(method, url, Ok(response));
    }

    /// Fails `method` requests to `url` without a response.
    pub fn fail(&mut self, method: &'static str, url: &str, error: &str) {
        self.queue(method, url, Err(error.into()));
    }

    fn queue(&mut self, method: &'static str, url: &str, answer: Result<Response, String>) {
        self.responses
            .entry((method, url.into()))
            .or_default()
            .push_back(answer);
    }
}

#[cfg(test)]
impl PocketSync for FakePocketSync {
    fn log(&mut self, line: &str) {
        self.lines.push(line.into());
    }

    fn send(&mut self, request: &Request) -> Result<Response, String> {
        self.sent.push(request.clone());
        let answers = self
            .responses
            .get_mut(&(request.method, request.url.clone()))
            .unwrap_or_else(|| panic!("unexpected request: {} {}", request.method, request.url));
        if answers.len() > 1 {
            answers.pop_front().unwrap()
        } else {
            answers[0].clone()
        }
    }

    fn open_url(&mut self, url: &str) {
        self.opened.push(url.into());
    }

    fn wait(&mut self, duration: Duration) {
        self.waited += duration;
    }
}
