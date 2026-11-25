mod card;
mod flow;
mod pocket_sync;
mod romm;
mod ui;

use std::cell::RefCell;

use card::Card;
use extism_pdk::*;
use flow::Run;
use pocket_sync::{PocketSync, Response};
use ui::{HostMessage, PluginMessage};

/// The card's root, as Pocket Sync gives it to the plugin.
const POCKET: &str = "pocket";

/// The language the plugin shows. Pocket Sync's `locale` config key is the system's language, even when
/// its "Always use English (US)" box is ticked, so this stays fixed until Pocket Sync passes the
/// language the user chose.
const LOCALE: &str = "en-US";

#[host_fn]
unsafe extern "ExtismHost" {
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

    fn get(&mut self, url: &str) -> Result<Response, String> {
        let response =
            http::request::<()>(&HttpRequest::new(url), None).map_err(|error| error.to_string())?;
        let status = response.status_code();
        // Extism doesn't free the body's memory when the response is dropped, so it's copied out
        // and freed here. Otherwise every response stays in memory until the call ends.
        let memory = response.into_memory();
        let body = memory.to_vec();
        memory.free();
        Ok(Response { status, body })
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
    let mut run = Run::new(LOCALE, Card::new(POCKET));
    let question = flow::start(&mut run, &mut Extism);
    // As in `continue_run`, a run that has ended isn't kept, and neither is an earlier one.
    let ended = matches!(question, PluginMessage::Exit);
    RUN.set(if ended { None } else { Some(run) });
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
        let mut run = Run::new("en-US", Card::new(root.path()));
        flow::start(&mut run, &mut FakePocketSync::default());
        Some(run)
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
