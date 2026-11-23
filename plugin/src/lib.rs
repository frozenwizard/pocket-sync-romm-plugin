mod flow;
mod ui;

use extism_pdk::*;
use ui::{HostMessage, PluginMessage};

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
    Ok(flow::start())
}

#[plugin_fn]
pub fn handle_response(message: HostMessage) -> FnResult<PluginMessage> {
    Ok(flow::handle(message, log))
}
