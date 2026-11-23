//! The state machine: what each answer leads to, and which question comes next.
//! See Architecture.md, section 12.

use crate::ui::{self, HostMessage, Key, MenuChoice, PluginMessage, Strings};

/// What a run remembers between calls. Later phases add the plan, pending conflicts and claimed
/// requests.
pub struct Run {
    strings: Strings,
}

impl Run {
    /// A new run, shown in `locale`.
    pub fn new(locale: &str) -> Self {
        Run {
            strings: Strings::new(locale),
        }
    }
}

/// The first question of a run.
pub fn start(run: &Run) -> PluginMessage {
    ui::menu(&run.strings)
}

/// Acts on Pocket Sync's reply and returns the next question. `log` writes a line to the run's log.
pub fn handle(message: HostMessage, run: &mut Run, mut log: impl FnMut(&str)) -> PluginMessage {
    let strings = &run.strings;
    match message {
        HostMessage::Kill => PluginMessage::Exit,
        HostMessage::Answer { name, value } => {
            match (name.as_str(), ui::menu_choice(strings, &value)) {
                (ui::MENU, Some(MenuChoice::SyncNow)) => {
                    log(strings.get(Key::SyncNotReady));
                    ui::menu(strings)
                }
                (ui::MENU, Some(MenuChoice::Settings)) => {
                    log(strings.get(Key::SettingsNotReady));
                    ui::menu(strings)
                }
                (ui::MENU, Some(MenuChoice::Exit)) => PluginMessage::Exit,
                // A stale or unexpected answer is ignored.
                _ => ui::menu(strings),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english() -> Strings {
        Strings::new("en-US")
    }

    fn answer(name: &str, value: &str) -> (PluginMessage, Vec<String>) {
        let mut lines = Vec::new();
        let message = HostMessage::Answer {
            name: name.into(),
            value: value.into(),
        };
        let reply = handle(message, &mut Run::new("en-US"), |line| {
            lines.push(line.to_string())
        });
        (reply, lines)
    }

    #[test]
    fn start_shows_the_menu() {
        assert_eq!(start(&Run::new("en-US")), ui::menu(&english()));
    }

    #[test]
    fn sync_now_is_not_available_yet() {
        let english = english();
        let not_ready = vec![english.get(Key::SyncNotReady).to_string()];
        assert_eq!(
            answer(ui::MENU, english.get(Key::SyncNow)),
            (ui::menu(&english), not_ready)
        );
    }

    #[test]
    fn settings_are_not_available_yet() {
        let english = english();
        let not_ready = vec![english.get(Key::SettingsNotReady).to_string()];
        assert_eq!(
            answer(ui::MENU, english.get(Key::Settings)),
            (ui::menu(&english), not_ready)
        );
    }

    #[test]
    fn exit_ends_the_run() {
        let exit = english().get(Key::Exit).to_string();
        assert_eq!(answer(ui::MENU, &exit), (PluginMessage::Exit, vec![]));
    }

    #[test]
    fn kill_ends_the_run() {
        assert_eq!(
            handle(HostMessage::Kill, &mut Run::new("en-US"), |_| {}),
            PluginMessage::Exit
        );
    }

    #[test]
    fn unexpected_answers_show_the_menu_again() {
        assert_eq!(answer(ui::MENU, "Nope"), (ui::menu(&english()), vec![]));
        // A menu label sent to another question is still ignored.
        let exit = english().get(Key::Exit).to_string();
        assert_eq!(
            answer("confirm-plan", &exit),
            (ui::menu(&english()), vec![])
        );
    }
}
