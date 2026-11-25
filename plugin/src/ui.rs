//! The messages exchanged with Pocket Sync, and the questions in them.

mod strings;

use extism_pdk::{FromBytes, Json, ToBytes};
use serde::{Deserialize, Serialize};
pub use strings::{Key, Strings};

/// A question for Pocket Sync to show, or `Exit` to end the run.
#[derive(Clone, Debug, PartialEq, Serialize, ToBytes)]
#[encoding(Json)]
pub enum PluginMessage {
    /// Answered with one of the buttons in `choices`.
    Choice {
        name: String,
        query: String,
        choices: Vec<String>,
    },
    /// Answered by typing.
    Text {
        name: String,
        query: String,
    },
    Exit,
}

impl PluginMessage {
    /// The question's name, which comes back with its answer. `Exit` asks nothing.
    pub fn name(&self) -> Option<&str> {
        match self {
            PluginMessage::Choice { name, .. } | PluginMessage::Text { name, .. } => Some(name),
            PluginMessage::Exit => None,
        }
    }
}

/// Pocket Sync's reply to a question: the answer, or `Kill`. `Kill` only arrives while a question is
/// waiting; Pocket Sync's Stop sends nothing and cuts the plugin off, even mid-call.
#[derive(Debug, Deserialize, FromBytes)]
#[encoding(Json)]
pub enum HostMessage {
    Answer { name: String, value: String },
    Kill,
}

// The questions' names.
pub const SERVER: &str = "server";
pub const BAD_SERVER: &str = "bad-server";
pub const CONNECT: &str = "connect";
pub const OPTIONS: &str = "options";
pub const MENU: &str = "menu";

/// The buttons of a `Choice` question.
pub trait Choices: Copy + 'static {
    /// Every button, in the order shown.
    const ALL: &'static [Self];

    /// The button's label.
    fn label(self) -> Key;
}

/// The button labelled `value` in `strings`' language. Pocket Sync sends back the label, not the
/// button.
pub fn chosen<C: Choices>(strings: &Strings, value: &str) -> Option<C> {
    C::ALL
        .iter()
        .copied()
        .find(|choice| strings.get(choice.label()) == value)
}

/// A `Choice` question named `name`, with a button for each of `C`.
fn choice<C: Choices>(strings: &Strings, name: &str, query: String) -> PluginMessage {
    PluginMessage::Choice {
        name: name.into(),
        query,
        choices: C::ALL
            .iter()
            .map(|choice| strings.get(choice.label()).into())
            .collect(),
    }
}

/// Asks for the RomM server's address. `current` is the one it replaces, if there is one.
pub fn server(strings: &Strings, current: Option<&str>) -> PluginMessage {
    let query = match current {
        Some(current) => strings.fill(Key::ChangeServerQuery, &[("host", current)]),
        None => strings.get(Key::ServerQuery).into(),
    };
    PluginMessage::Text {
        name: SERVER.into(),
        query,
    }
}

/// A button on the question after an answer that isn't a server address. Pocket Sync's `Text`
/// question has no buttons, so this is how the address question is left without an answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BadServerChoice {
    TryAgain,
    Cancel,
}

impl Choices for BadServerChoice {
    const ALL: &'static [Self] = &[BadServerChoice::TryAgain, BadServerChoice::Cancel];

    fn label(self) -> Key {
        match self {
            BadServerChoice::TryAgain => Key::TryAgain,
            BadServerChoice::Cancel => Key::Cancel,
        }
    }
}

/// Says the answer to the address question isn't a server address, and offers to try again.
pub fn bad_server(strings: &Strings) -> PluginMessage {
    choice::<BadServerChoice>(strings, BAD_SERVER, strings.get(Key::BadHost).into())
}

/// A button on the first question once the server's address is known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnectChoice {
    Connect,
    Options,
    Exit,
}

impl Choices for ConnectChoice {
    const ALL: &'static [Self] = &[
        ConnectChoice::Connect,
        ConnectChoice::Options,
        ConnectChoice::Exit,
    ];

    fn label(self) -> Key {
        match self {
            ConnectChoice::Connect => Key::Connect,
            ConnectChoice::Options => Key::Options,
            ConnectChoice::Exit => Key::Exit,
        }
    }
}

/// Offers to connect to the server at `host`.
pub fn connect(strings: &Strings, host: &str) -> PluginMessage {
    let query = strings.fill(Key::ConnectQuery, &[("host", host)]);
    choice::<ConnectChoice>(strings, CONNECT, query)
}

/// A button in Options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OptionsChoice {
    ChangeServer,
    Back,
}

impl Choices for OptionsChoice {
    const ALL: &'static [Self] = &[OptionsChoice::ChangeServer, OptionsChoice::Back];

    fn label(self) -> Key {
        match self {
            OptionsChoice::ChangeServer => Key::ChangeServer,
            OptionsChoice::Back => Key::Back,
        }
    }
}

pub fn options(strings: &Strings) -> PluginMessage {
    choice::<OptionsChoice>(strings, OPTIONS, strings.get(Key::OptionsQuery).into())
}

/// A button on the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuChoice {
    SyncNow,
    Settings,
    Exit,
}

impl Choices for MenuChoice {
    const ALL: &'static [Self] = &[MenuChoice::SyncNow, MenuChoice::Settings, MenuChoice::Exit];

    fn label(self) -> Key {
        match self {
            MenuChoice::SyncNow => Key::SyncNow,
            MenuChoice::Settings => Key::Settings,
            MenuChoice::Exit => Key::Exit,
        }
    }
}

pub fn menu(strings: &Strings) -> PluginMessage {
    choice::<MenuChoice>(strings, MENU, strings.get(Key::MenuQuery).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use extism_pdk::json::{from_str, json, to_value};
    use std::collections::BTreeSet;
    use std::fmt::Debug;

    // Pocket Sync has its own copies of these enums, so the JSON is pinned here.

    #[test]
    fn messages_are_sent_as_pocket_sync_reads_them() {
        let english = Strings::new("en-US");
        assert_eq!(
            to_value(menu(&english)).unwrap(),
            json!({"Choice": {
                "name": "menu",
                "query": "What would you like to do?",
                "choices": ["Sync now", "Settings", "Exit"],
            }})
        );
        assert_eq!(
            to_value(server(&english, None)).unwrap(),
            json!({"Text": {"name": "server", "query": english.get(Key::ServerQuery)}})
        );
        assert_eq!(to_value(PluginMessage::Exit).unwrap(), json!("Exit"));
    }

    #[test]
    fn pocket_sync_replies_parse() {
        let answer = from_str(r#"{"Answer":{"name":"menu","value":"Exit"}}"#).unwrap();
        assert!(matches!(
            answer,
            HostMessage::Answer { name, value } if name == "menu" && value == "Exit"
        ));
        let kill = from_str(r#""Kill""#).unwrap();
        assert!(matches!(kill, HostMessage::Kill));
    }

    #[test]
    fn the_current_address_is_in_the_change_question() {
        let english = Strings::new("en-US");
        let question = server(&english, Some("https://romm.example.com"));
        let PluginMessage::Text { query, .. } = question else {
            panic!("{question:?}");
        };
        assert!(query.contains("https://romm.example.com"), "{query}");
    }

    /// Checks each of `C`'s labels is matched back to its button.
    fn labels_find_their_choice<C: Choices + PartialEq + Debug>() {
        let english = Strings::new("en-US");
        for &choice in C::ALL {
            assert_eq!(
                chosen::<C>(&english, english.get(choice.label())),
                Some(choice)
            );
        }
        assert_eq!(chosen::<C>(&english, "Nope"), None);
    }

    #[test]
    fn answers_are_matched_to_their_choice() {
        labels_find_their_choice::<BadServerChoice>();
        labels_find_their_choice::<ConnectChoice>();
        labels_find_their_choice::<OptionsChoice>();
        labels_find_their_choice::<MenuChoice>();
    }

    fn labels<C: Choices>() -> Vec<Key> {
        C::ALL.iter().map(|choice| choice.label()).collect()
    }

    // Otherwise pressing the second of two same-labelled buttons would run the first one's action.
    #[test]
    fn labels_differ_within_a_question_in_every_language() {
        let questions = [
            labels::<BadServerChoice>(),
            labels::<ConnectChoice>(),
            labels::<OptionsChoice>(),
            labels::<MenuChoice>(),
        ];
        for (tag, _) in strings::LOCALES {
            let language = Strings::new(tag);
            for keys in &questions {
                let labels: BTreeSet<_> = keys.iter().map(|&key| language.get(key)).collect();
                assert_eq!(labels.len(), keys.len(), "{tag}: {labels:?}");
            }
        }
    }

    #[test]
    fn answers_are_matched_in_the_language_shown() {
        let locales = [
            (
                "en",
                r#"{"menu": {"sync_now": "Sync now", "settings": "Settings", "exit": "Exit"}}"#,
            ),
            ("de", r#"{"menu": {"exit": "Beenden"}}"#),
        ];
        let german = Strings::with_locales("de-DE", &locales);
        assert_eq!(chosen(&german, "Beenden"), Some(MenuChoice::Exit));
        assert_eq!(chosen::<MenuChoice>(&german, "Exit"), None);
        // Untranslated labels are shown, and matched, in English.
        assert_eq!(chosen(&german, "Settings"), Some(MenuChoice::Settings));
    }
}
