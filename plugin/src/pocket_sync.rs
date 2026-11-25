//! What the plugin uses of Pocket Sync, behind a trait so tests can stand in for it.

#[cfg(test)]
use std::collections::HashMap;

/// An HTTP response.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Pocket Sync, the plugin's host: its log and the network.
pub trait PocketSync {
    /// Writes a line to the log Pocket Sync shows.
    fn log(&mut self, line: &str);

    /// Sends a GET request to `url`. When no response arrives (a refused connection, an unknown
    /// server, a bad certificate), Extism ends the plugin's call instead of returning an error.
    fn get(&mut self, url: &str) -> Result<Response, String>;
}

/// A stand-in for Pocket Sync that keeps the log and answers requests from a list.
#[cfg(test)]
#[derive(Default)]
pub struct FakePocketSync {
    pub lines: Vec<String>,
    responses: HashMap<String, Result<Response, String>>,
}

#[cfg(test)]
impl FakePocketSync {
    /// Answers GET `url` with `status` and `body`.
    pub fn respond(&mut self, url: &str, status: u16, body: &str) {
        let response = Response {
            status,
            body: body.into(),
        };
        self.responses.insert(url.into(), Ok(response));
    }

    /// Fails GET `url` without a response.
    pub fn fail(&mut self, url: &str, error: &str) {
        self.responses.insert(url.into(), Err(error.into()));
    }
}

#[cfg(test)]
impl PocketSync for FakePocketSync {
    fn log(&mut self, line: &str) {
        self.lines.push(line.into());
    }

    fn get(&mut self, url: &str) -> Result<Response, String> {
        self.responses
            .get(url)
            .cloned()
            .unwrap_or_else(|| panic!("unexpected request: GET {url}"))
    }
}
