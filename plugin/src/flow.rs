//! The state machine: what each answer leads to, and which question comes next.
//! See Architecture.md, section 12.

use crate::ui::{self, HostMessage, PluginMessage};

/// The first question of a run.
pub fn start() -> PluginMessage {
    ui::menu()
}

/// Acts on Pocket Sync's reply and returns the next question. `log` writes a line to the run's log.
pub fn handle(message: HostMessage, mut log: impl FnMut(&str)) -> PluginMessage {
    match message {
        HostMessage::Kill => PluginMessage::Exit,
        HostMessage::Answer { name, value } => match (name.as_str(), value.as_str()) {
            (ui::MENU, ui::SYNC_NOW) => {
                log(ui::SYNC_NOT_READY);
                ui::menu()
            }
            (ui::MENU, ui::SETTINGS) => {
                log(ui::SETTINGS_NOT_READY);
                ui::menu()
            }
            (ui::MENU, ui::EXIT) => PluginMessage::Exit,
            // A stale or unexpected answer is ignored.
            _ => ui::menu(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(name: &str, value: &str) -> (PluginMessage, Vec<String>) {
        let mut lines = Vec::new();
        let message = HostMessage::Answer {
            name: name.into(),
            value: value.into(),
        };
        let reply = handle(message, |line| lines.push(line.to_string()));
        (reply, lines)
    }

    #[test]
    fn start_shows_the_menu() {
        assert_eq!(start(), ui::menu());
    }

    #[test]
    fn sync_now_is_not_available_yet() {
        let not_ready = vec![ui::SYNC_NOT_READY.to_string()];
        assert_eq!(answer(ui::MENU, ui::SYNC_NOW), (ui::menu(), not_ready));
    }

    #[test]
    fn settings_are_not_available_yet() {
        let not_ready = vec![ui::SETTINGS_NOT_READY.to_string()];
        assert_eq!(answer(ui::MENU, ui::SETTINGS), (ui::menu(), not_ready));
    }

    #[test]
    fn exit_ends_the_run() {
        assert_eq!(answer(ui::MENU, ui::EXIT), (PluginMessage::Exit, vec![]));
    }

    #[test]
    fn kill_ends_the_run() {
        assert_eq!(handle(HostMessage::Kill, |_| {}), PluginMessage::Exit);
    }

    #[test]
    fn unexpected_answers_show_the_menu_again() {
        assert_eq!(answer(ui::MENU, "Nope"), (ui::menu(), vec![]));
        assert_eq!(answer("confirm-plan", "Sync"), (ui::menu(), vec![]));
    }
}
