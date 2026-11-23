mod flow;
mod ui;

use std::cell::RefCell;

use extism_pdk::*;
use flow::Run;
use ui::{HostMessage, PluginMessage};

/// The language the plugin shows. Pocket Sync's `locale` config key is the system's language, even when
/// its "Always use English (US)" box is ticked, so this stays fixed until Pocket Sync passes the
/// language the user chose.
const LOCALE: &str = "en-US";

#[host_fn]
unsafe extern "ExtismHost" {
    fn print_msg(message: &str) -> ();
}

/// Writes a line to the log Pocket Sync shows, ignoring errors. That doesn't cover a failure inside
/// Pocket Sync's own `print_msg`, which ends the call.
fn log(line: &str) {
    unsafe {
        let _ = print_msg(&format!("{line}\n"));
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
    let run = Run::new(LOCALE);
    let question = flow::start(&run);
    RUN.set(Some(run));
    Ok(question)
}

#[plugin_fn]
pub fn handle_response(message: HostMessage) -> FnResult<PluginMessage> {
    Ok(RUN.with_borrow_mut(|run| continue_run(run, message, log))?)
}

/// Passes `message` to the current run and returns the next question, ending the run on `Exit`.
/// Pocket Sync calls `start` first, so an answer with no run means something went wrong, and is an
/// error rather than a new run.
fn continue_run(
    current: &mut Option<Run>,
    message: HostMessage,
    log: impl FnMut(&str),
) -> Result<PluginMessage, Error> {
    let Some(run) = current else {
        return Err(Error::msg(
            "an answer arrived with no run; Pocket Sync calls start first",
        ));
    };
    let question = flow::handle(message, run, log);
    if matches!(question, PluginMessage::Exit) {
        *current = None;
    }
    Ok(question)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_is_kept_between_answers() {
        let mut current = Some(Run::new("en-US"));
        let answer = HostMessage::Answer {
            name: ui::MENU.into(),
            value: "Nope".into(),
        };
        continue_run(&mut current, answer, |_| {}).unwrap();
        assert!(current.is_some());
    }

    #[test]
    fn the_run_ends_when_the_plugin_exits() {
        let mut current = Some(Run::new("en-US"));
        let question = continue_run(&mut current, HostMessage::Kill, |_| {}).unwrap();
        assert_eq!(question, PluginMessage::Exit);
        assert!(current.is_none());
    }

    #[test]
    fn an_answer_with_no_run_is_an_error() {
        assert!(continue_run(&mut None, HostMessage::Kill, |_| {}).is_err());
    }
}
