use extism_pdk::*;
use serde::Serialize;

#[derive(Serialize, ToBytes)]
#[encoding(Json)]
enum PluginMessage {
    Exit,
}

#[host_fn]
unsafe extern "ExtismHost" {
    fn print_msg(message: &str) -> ();
}

#[plugin_fn]
pub fn start() -> FnResult<PluginMessage> {
    unsafe { print_msg("Hello from the RomM plugin!\n")? };
    Ok(PluginMessage::Exit)
}
