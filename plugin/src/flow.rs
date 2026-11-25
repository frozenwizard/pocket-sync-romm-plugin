//! The state machine: what each answer leads to, and which question comes next.
//! See Architecture.md, section 12.

use crate::card::{Card, PocketIdError};
use crate::pocket_sync::PocketSync;
use crate::romm::{self, ConnectError};
use crate::ui::{
    self, BadServerChoice, ConnectChoice, HostMessage, Key, MenuChoice, OptionsChoice,
    PluginMessage, Strings,
};

/// What a run remembers between calls. Later phases add the plan, pending conflicts and claimed
/// requests.
pub struct Run {
    strings: Strings,
    card: Card,
    /// The question waiting for its answer, asked again when an answer doesn't fit it.
    asked: PluginMessage,
}

impl Run {
    /// A new run on `card`, shown in `locale`.
    pub fn new(locale: &str, card: Card) -> Self {
        Run {
            strings: Strings::new(locale),
            card,
            asked: PluginMessage::Exit,
        }
    }
}

/// Checks the card is a Pocket's and gives the Pocket an ID if it has none, then returns the first
/// question. Returns `Exit` when the card can't be used.
pub fn start(run: &mut Run, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let question = first_question(run, pocket_sync);
    run.asked = question.clone();
    question
}

/// Acts on Pocket Sync's reply and returns the next question.
pub fn handle(
    message: HostMessage,
    run: &mut Run,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    let question = match message {
        HostMessage::Kill => PluginMessage::Exit,
        HostMessage::Answer { name, value } if run.asked.name() == Some(name.as_str()) => {
            answer(run, &name, &value, pocket_sync).unwrap_or_else(|| run.asked.clone())
        }
        // An answer to a question that isn't waiting is ignored.
        HostMessage::Answer { .. } => run.asked.clone(),
    };
    run.asked = question.clone();
    question
}

fn first_question(run: &Run, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let strings = &run.strings;
    if !run.card.is_pocket_card() {
        pocket_sync.log(strings.get(Key::NotAPocketCard));
        return PluginMessage::Exit;
    }
    run.card.remove_leftover_parts();
    if let Err(error) = run.card.pocket_id() {
        pocket_sync.log(strings.get(match &error {
            PocketIdError::Unreadable(_) => Key::PocketIdUnreadable,
            PocketIdError::Mismatch(_) => Key::PocketIdMismatch,
            PocketIdError::NotSaved(_) => Key::PocketIdNotSaved,
        }));
        pocket_sync.log(&error.to_string());
        return PluginMessage::Exit;
    }
    server_question(run, pocket_sync)
}

/// The next question after `value` answered the question named `name`, or `None` when `value` isn't
/// one of its choices.
fn answer(
    run: &Run,
    name: &str,
    value: &str,
    pocket_sync: &mut impl PocketSync,
) -> Option<PluginMessage> {
    let strings = &run.strings;
    let question = match name {
        ui::SERVER => use_host(run, value, pocket_sync),
        ui::BAD_SERVER => match ui::chosen(strings, value)? {
            BadServerChoice::TryAgain => match saved_host(run, pocket_sync) {
                Ok(host) => ui::server(strings, host.as_deref()),
                Err(end) => end,
            },
            // Back to Options when an address is being changed; with none yet, there's nothing to
            // go back to.
            BadServerChoice::Cancel => match saved_host(run, pocket_sync) {
                Ok(Some(_)) => ui::options(strings),
                Ok(None) => PluginMessage::Exit,
                Err(end) => end,
            },
        },
        ui::CONNECT => match ui::chosen(strings, value)? {
            ConnectChoice::Connect => match saved_host(run, pocket_sync) {
                Ok(Some(host)) => connect(run, &host, pocket_sync),
                Ok(None) => ui::server(strings, None),
                Err(end) => end,
            },
            ConnectChoice::Options => ui::options(strings),
            ConnectChoice::Exit => PluginMessage::Exit,
        },
        ui::OPTIONS => match ui::chosen(strings, value)? {
            OptionsChoice::ChangeServer => match saved_host(run, pocket_sync) {
                Ok(host) => ui::server(strings, host.as_deref()),
                Err(end) => end,
            },
            OptionsChoice::Back => server_question(run, pocket_sync),
        },
        ui::MENU => match ui::chosen(strings, value)? {
            MenuChoice::SyncNow => {
                pocket_sync.log(strings.get(Key::SyncNotReady));
                ui::menu(strings)
            }
            MenuChoice::Settings => {
                pocket_sync.log(strings.get(Key::SettingsNotReady));
                ui::menu(strings)
            }
            MenuChoice::Exit => PluginMessage::Exit,
        },
        _ => return None,
    };
    Some(question)
}

/// Offers to connect to the saved server, or asks for its address when there isn't one yet.
fn server_question(run: &Run, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    match saved_host(run, pocket_sync) {
        Ok(Some(host)) => ui::connect(&run.strings, &host),
        Ok(None) => ui::server(&run.strings, None),
        Err(end) => end,
    }
}

/// The server's address in `romm-sync/server.json`, read each time so edits made while the plugin
/// waits are used. `None` when there's no file or no address in it. A file that can't be read is
/// logged and ends the run (the error is `Exit`): asking for the address instead would lead to
/// replacing a file that may hold the sign-in.
fn saved_host(
    run: &Run,
    pocket_sync: &mut impl PocketSync,
) -> Result<Option<String>, PluginMessage> {
    run.card.server_host().map_err(|error| {
        pocket_sync.log(run.strings.get(Key::ServerFileUnreadable));
        pocket_sync.log(&error.to_string());
        PluginMessage::Exit
    })
}

/// Saves the address the user typed and connects to it. An answer that isn't an address, an empty
/// one included, offers Try again and Cancel.
fn use_host(run: &Run, typed: &str, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let strings = &run.strings;
    let host = typed.trim();
    if romm::base_url(host).is_none() {
        return ui::bad_server(strings);
    }
    if let Err(error) = run.card.save_server_host(host) {
        pocket_sync.log(strings.get(Key::ServerFileNotSaved));
        pocket_sync.log(&error);
        return PluginMessage::Exit;
    }
    connect(run, host, pocket_sync)
}

/// Checks the server at `host` is RomM 5.4 or newer, then shows the menu. Otherwise says why and
/// offers to connect again.
fn connect(run: &Run, host: &str, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let strings = &run.strings;
    // Logged first: when the server can't be reached, Extism ends the call before anything else is.
    pocket_sync.log(&strings.fill(Key::Connecting, &[("host", host)]));
    match romm::connect(pocket_sync, host) {
        Ok(version) => {
            pocket_sync.log(&strings.fill(Key::Connected, &[("version", &version)]));
            ui::menu(strings)
        }
        Err(error) => {
            pocket_sync.log(&match error {
                ConnectError::BadHost => strings.fill(Key::BadSavedHost, &[("host", host)]),
                ConnectError::NoResponse(error) => {
                    strings.fill(Key::NoResponse, &[("host", host), ("error", &error)])
                }
                ConnectError::NotRomm(status) => strings.fill(
                    Key::NotRomm,
                    &[("host", host), ("status", &status.to_string())],
                ),
                ConnectError::TooOld(version) => {
                    strings.fill(Key::RommTooOld, &[("host", host), ("version", &version)])
                }
            });
            ui::connect(strings, host)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card;
    use crate::pocket_sync::FakePocketSync;
    use std::fs;
    use tempfile::TempDir;

    const HOST: &str = "https://romm.example.com";
    const HEARTBEAT: &str = "https://romm.example.com/api/heartbeat";

    fn english() -> Strings {
        Strings::new("en-US")
    }

    fn heartbeat(version: &str) -> String {
        format!(r#"{{"SYSTEM": {{"VERSION": "{version}"}}}}"#)
    }

    /// A Pocket card whose `server.json` holds `HOST`.
    fn card_with_server() -> TempDir {
        let root = card::test_card();
        Card::new(root.path()).save_server_host(HOST).unwrap();
        root
    }

    /// Starts a run on the card at `root`, then gives each answer in turn to the question waiting.
    /// Returns the last question.
    fn play(root: &TempDir, pocket_sync: &mut FakePocketSync, answers: &[&str]) -> PluginMessage {
        let mut run = Run::new("en-US", Card::new(root.path()));
        let mut question = start(&mut run, pocket_sync);
        for value in answers {
            let name = question.name().expect("a question is waiting").to_string();
            let answer = HostMessage::Answer {
                name,
                value: value.to_string(),
            };
            question = handle(answer, &mut run, pocket_sync);
        }
        question
    }

    #[test]
    fn a_new_pocket_gets_an_id_and_is_asked_for_the_server() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&root, &mut pocket_sync, &[]);
        assert_eq!(question, ui::server(&english(), None));
        // Checked before anything that would write it.
        assert!(root.path().join("romm-sync/device.json").is_file());
        assert_eq!(pocket_sync.lines, Vec::<String>::new());
    }

    #[test]
    fn a_run_starts_by_removing_leftover_part_files() {
        let root = card_with_server();
        let part = root.path().join("romm-sync/server.json.part");
        fs::write(&part, "{").unwrap();
        play(&root, &mut FakePocketSync::default(), &[]);
        assert!(!part.exists());
    }

    #[test]
    fn a_pocket_with_a_server_is_offered_connect_and_options() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&card_with_server(), &mut pocket_sync, &[]);
        assert_eq!(question, ui::connect(&english(), HOST));
    }

    #[test]
    fn a_card_that_isnt_a_pockets_ends_the_run() {
        let root = tempfile::tempdir().unwrap();
        let mut pocket_sync = FakePocketSync::default();
        assert_eq!(play(&root, &mut pocket_sync, &[]), PluginMessage::Exit);
        assert_eq!(pocket_sync.lines, [english().get(Key::NotAPocketCard)]);
        assert!(!root.path().join("romm-sync").exists());
    }

    #[test]
    fn an_unreadable_pocket_id_ends_the_run() {
        let root = card::test_card();
        fs::create_dir(root.path().join("romm-sync")).unwrap();
        fs::write(root.path().join("romm-sync/device.json"), "{").unwrap();
        let mut pocket_sync = FakePocketSync::default();
        assert_eq!(play(&root, &mut pocket_sync, &[]), PluginMessage::Exit);
        assert_eq!(pocket_sync.lines[0], english().get(Key::PocketIdMismatch));
        assert_eq!(pocket_sync.lines.len(), 2, "{:?}", pocket_sync.lines);
    }

    #[test]
    fn an_unreadable_server_json_ends_the_run_and_is_kept() {
        let root = card::test_card();
        fs::create_dir(root.path().join("romm-sync")).unwrap();
        let server_file = root.path().join("romm-sync/server.json");
        fs::write(&server_file, "{").unwrap();
        let mut pocket_sync = FakePocketSync::default();
        assert_eq!(play(&root, &mut pocket_sync, &[]), PluginMessage::Exit);
        assert_eq!(
            pocket_sync.lines,
            [
                english().get(Key::ServerFileUnreadable),
                "the file ends early, at line 1, column 1"
            ]
        );
        assert_eq!(fs::read_to_string(&server_file).unwrap(), "{");
    }

    #[test]
    fn server_json_broken_while_the_plugin_waits_ends_the_run() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let mut run = Run::new("en-US", Card::new(root.path()));
        start(&mut run, &mut pocket_sync);
        fs::write(root.path().join("romm-sync/server.json"), "{").unwrap();
        let connect = HostMessage::Answer {
            name: ui::CONNECT.into(),
            value: "Connect".into(),
        };
        assert_eq!(
            handle(connect, &mut run, &mut pocket_sync),
            PluginMessage::Exit
        );
        assert_eq!(
            pocket_sync.lines[0],
            english().get(Key::ServerFileUnreadable)
        );
    }

    #[test]
    fn a_saved_address_that_cant_be_used_is_logged() {
        let root = card::test_card();
        fs::create_dir(root.path().join("romm-sync")).unwrap();
        let server_file = root.path().join("romm-sync/server.json");
        fs::write(&server_file, r#"{"host": "romm.example.com"}"#).unwrap();
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::connect(&english(), "romm.example.com"));
        let bad = english().fill(Key::BadSavedHost, &[("host", "romm.example.com")]);
        assert_eq!(pocket_sync.lines[1], bad);
    }

    #[test]
    fn a_typed_address_is_saved_and_connected_to() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond(HEARTBEAT, 200, &heartbeat("5.4.0"));
        let question = play(&root, &mut pocket_sync, &[" https://romm.example.com "]);
        assert_eq!(question, ui::menu(&english()));
        assert_eq!(Card::new(root.path()).server_host(), Ok(Some(HOST.into())));
        assert_eq!(
            pocket_sync.lines,
            [
                "Connecting to https://romm.example.com…",
                "Connected to RomM 5.4.0."
            ]
        );
    }

    #[test]
    fn an_answer_that_isnt_an_address_offers_try_again() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        for typed in ["romm.example.com", "", "https://:8080"] {
            let question = play(&root, &mut pocket_sync, &[typed]);
            assert_eq!(question, ui::bad_server(&english()), "{typed:?}");
        }
        assert_eq!(Card::new(root.path()).server_host(), Ok(None));
        let question = play(&root, &mut pocket_sync, &["romm.example.com", "Try again"]);
        assert_eq!(question, ui::server(&english(), None));
    }

    #[test]
    fn cancel_on_first_setup_ends_the_run() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&root, &mut pocket_sync, &["", "Cancel"]);
        assert_eq!(question, PluginMessage::Exit);
    }

    #[test]
    fn cancel_keeps_the_address_when_changing_it() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let change = ["Options", "Change server address", "", "Cancel"];
        assert_eq!(
            play(&root, &mut pocket_sync, &change),
            ui::options(&english())
        );
        assert_eq!(Card::new(root.path()).server_host(), Ok(Some(HOST.into())));
        let again = ["Options", "Change server address", "", "Try again"];
        assert_eq!(
            play(&root, &mut pocket_sync, &again),
            ui::server(&english(), Some(HOST))
        );
    }

    #[test]
    fn connect_checks_the_server() {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond(HEARTBEAT, 200, &heartbeat("5.4.1"));
        let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::menu(&english()));
    }

    #[test]
    fn a_server_that_cant_be_used_is_offered_again() {
        let too_old = english().fill(Key::RommTooOld, &[("host", HOST), ("version", "5.3.2")]);
        let not_romm = english().fill(Key::NotRomm, &[("host", HOST), ("status", "404")]);
        for (status, body, reason) in [
            (200, heartbeat("5.3.2"), too_old),
            (404, "Not Found".to_string(), not_romm),
        ] {
            let mut pocket_sync = FakePocketSync::default();
            pocket_sync.respond(HEARTBEAT, status, &body);
            let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
            assert_eq!(question, ui::connect(&english(), HOST));
            assert_eq!(pocket_sync.lines[1], reason);
        }
    }

    #[test]
    fn edits_to_server_json_are_used_on_connect() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let mut run = Run::new("en-US", Card::new(root.path()));
        start(&mut run, &mut pocket_sync);
        let other = "http://192.168.1.5:8080";
        Card::new(root.path()).save_server_host(other).unwrap();
        pocket_sync.respond(&format!("{other}/api/heartbeat"), 200, &heartbeat("5.4.0"));
        let connect = HostMessage::Answer {
            name: ui::CONNECT.into(),
            value: "Connect".into(),
        };
        assert_eq!(
            handle(connect, &mut run, &mut pocket_sync),
            ui::menu(&english())
        );
    }

    #[test]
    fn options_change_the_server_address() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let asked = play(
            &root,
            &mut pocket_sync,
            &["Options", "Change server address"],
        );
        assert_eq!(asked, ui::server(&english(), Some(HOST)));

        let other = "http://192.168.1.5:8080";
        pocket_sync.respond(&format!("{other}/api/heartbeat"), 200, &heartbeat("5.4.0"));
        let answers = ["Options", "Change server address", other];
        assert_eq!(
            play(&root, &mut pocket_sync, &answers),
            ui::menu(&english())
        );
        assert_eq!(Card::new(root.path()).server_host(), Ok(Some(other.into())));
    }

    #[test]
    fn back_leaves_options() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&card_with_server(), &mut pocket_sync, &["Options", "Back"]);
        assert_eq!(question, ui::connect(&english(), HOST));
    }

    #[test]
    fn exit_ends_the_run() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&card_with_server(), &mut pocket_sync, &["Exit"]);
        assert_eq!(question, PluginMessage::Exit);
    }

    #[test]
    fn kill_ends_the_run() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let mut run = Run::new("en-US", Card::new(root.path()));
        start(&mut run, &mut pocket_sync);
        assert_eq!(
            handle(HostMessage::Kill, &mut run, &mut pocket_sync),
            PluginMessage::Exit
        );
    }

    /// Gives `value` as the answer to question `name` while the menu is waiting. Returns the next
    /// question and the log.
    fn at_menu(name: &str, value: &str) -> (PluginMessage, Vec<String>) {
        let mut run = Run::new("en-US", Card::new("unused"));
        run.asked = ui::menu(&run.strings);
        let mut pocket_sync = FakePocketSync::default();
        let answer = HostMessage::Answer {
            name: name.into(),
            value: value.into(),
        };
        let question = handle(answer, &mut run, &mut pocket_sync);
        (question, pocket_sync.lines)
    }

    #[test]
    fn sync_now_is_not_available_yet() {
        let english = english();
        let not_ready = vec![english.get(Key::SyncNotReady).to_string()];
        assert_eq!(
            at_menu(ui::MENU, english.get(Key::SyncNow)),
            (ui::menu(&english), not_ready)
        );
    }

    #[test]
    fn settings_are_not_available_yet() {
        let english = english();
        let not_ready = vec![english.get(Key::SettingsNotReady).to_string()];
        assert_eq!(
            at_menu(ui::MENU, english.get(Key::Settings)),
            (ui::menu(&english), not_ready)
        );
    }

    #[test]
    fn exit_on_the_menu_ends_the_run() {
        let exit = english().get(Key::Exit).to_string();
        assert_eq!(at_menu(ui::MENU, &exit), (PluginMessage::Exit, vec![]));
    }

    #[test]
    fn unexpected_answers_ask_the_question_again() {
        assert_eq!(at_menu(ui::MENU, "Nope"), (ui::menu(&english()), vec![]));
        // A menu label sent to a question that isn't waiting is ignored too.
        let exit = english().get(Key::Exit).to_string();
        assert_eq!(at_menu(ui::CONNECT, &exit), (ui::menu(&english()), vec![]));
    }
}
