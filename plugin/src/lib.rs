mod flow;
mod ui;

use extism_pdk::*;
use ui::{HostMessage, PluginMessage, Strings};

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

#[plugin_fn]
pub fn start() -> FnResult<PluginMessage> {
    Ok(flow::start(&Strings::new(LOCALE)))
}

#[plugin_fn]
pub fn handle_response(message: HostMessage) -> FnResult<PluginMessage> {
    Ok(flow::handle(message, &Strings::new(LOCALE), log))
}
