mod card;
mod flow;
mod pocket_sync;
mod romm;
mod ui;

use std::cell::RefCell;
use std::time::Duration;

use card::Card;
use extism_pdk::*;
use flow::Run;
use pocket_sync::{PocketSync, Request, Response};
use ui::{HostMessage, PluginMessage};

/// The card's root, as Pocket Sync gives it to the plugin.
const POCKET: &str = "pocket";

/// The language the plugin shows. Pocket Sync's `locale` config key is the system's language, even when
/// its "Always use English (US)" box is ticked, so this stays fixed until Pocket Sync passes the
/// language the user chose.
const LOCALE: &str = "en-US";

#[host_fn]
unsafe extern "ExtismHost" {
    fn open_url(url: &str) -> ();
    fn print_msg(message: &str) -> ();
}

/// Pocket Sync, reached through Extism.
struct Extism;

impl PocketSync for Extism {
    /// Ignores errors. That doesn't cover a failure inside Pocket Sync's own `print_msg`, which ends
    /// the call.
    fn log(&mut self, line: &str) {
        unsafe {
            let _ = print_msg(&format!("{line}\n"));
        }
    }

    fn send(&mut self, request: &Request) -> Result<Response, String> {
        let mut http_request = HttpRequest::new(&request.url).with_method(request.method);
        for (name, value) in &request.headers {
            http_request = http_request.with_header(*name, value);
        }
        // Extism doesn't free memory when it's dropped, so the request body and the response are
        // freed here. Otherwise each stays in memory until the call ends, which matters while
        // pairing polls for minutes. extism-pdk's own copy of the URL and headers isn't freed, but
        // that's a few hundred bytes a request.
        let body = (request.body.as_ref())
            .map(Memory::from_bytes)
            .transpose()
            .map_err(|error| error.to_string())?;
        let response = http::request(&http_request, body.as_ref());
        if let Some(body) = body {
            body.free();
        }
        let response = response.map_err(|error| error.to_string())?;
        let status = response.status_code();
        let memory = response.into_memory();
        let body = memory.to_vec();
        memory.free();
        Ok(Response { status, body })
    }

    /// Ignores errors, as `log` does. That doesn't cover Pocket Sync failing to open the browser,
    /// which ends the call.
    fn open_url(&mut self, url: &str) {
        unsafe {
            let _ = open_url(url);
        }
    }

    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

thread_local! {
    /// The current run, from `start` until the plugin returns `Exit`. Pocket Sync keeps one plugin
    /// instance per run, and Extism keeps its memory between calls. Wasm has one thread, so this is
    /// the plugin's only copy.
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
}

#[plugin_fn]
pub fn start() -> FnResult<PluginMessage> {
    let (run, question) = flow::start(LOCALE, Card::new(POCKET), &mut Extism);
    // Replaces any earlier run.
    RUN.set(run);
    Ok(question)
}

#[plugin_fn]
pub fn handle_response(message: HostMessage) -> FnResult<PluginMessage> {
    Ok(RUN.with_borrow_mut(|run| continue_run(run, message, &mut Extism))?)
}

/// Passes `message` to the current run and returns the next question, ending the run on `Exit`.
/// Pocket Sync calls `start` first, so an answer with no run means something went wrong, and is an
/// error rather than a new run.
fn continue_run(
    current: &mut Option<Run>,
    message: HostMessage,
    pocket_sync: &mut impl PocketSync,
) -> Result<PluginMessage, Error> {
    let Some(run) = current else {
        return Err(Error::msg(
            "an answer arrived with no run; Pocket Sync calls start first",
        ));
    };
    let question = flow::handle(message, run, pocket_sync);
    if matches!(question, PluginMessage::Exit) {
        *current = None;
    }
    Ok(question)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_sync::FakePocketSync;

    /// A run on a new Pocket card, waiting for the server's address.
    fn started(root: &tempfile::TempDir) -> Option<Run> {
        let card = Card::new(root.path());
        flow::start("en-US", card, &mut FakePocketSync::default()).0
    }

    #[test]
    fn the_run_is_kept_between_answers() {
        let root = card::test_card();
        let mut current = started(&root);
        let answer = HostMessage::Answer {
            name: ui::SERVER.into(),
            value: "not an address".into(),
        };
        continue_run(&mut current, answer, &mut FakePocketSync::default()).unwrap();
        assert!(current.is_some());
    }

    #[test]
    fn the_run_ends_when_the_plugin_exits() {
        let root = card::test_card();
        let mut current = started(&root);
        let kill = HostMessage::Kill;
        let question = continue_run(&mut current, kill, &mut FakePocketSync::default()).unwrap();
        assert_eq!(question, PluginMessage::Exit);
        assert!(current.is_none());
    }

    #[test]
    fn an_answer_with_no_run_is_an_error() {
        let kill = HostMessage::Kill;
        assert!(continue_run(&mut None, kill, &mut FakePocketSync::default()).is_err());
    }
}
