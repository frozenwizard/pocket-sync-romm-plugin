//! The messages exchanged with Pocket Sync, and the questions in them.

mod strings;

use extism_pdk::{FromBytes, Json, ToBytes};
use serde::{Deserialize, Serialize};
pub use strings::{Key, Strings};

/// A question for Pocket Sync to show, or `Exit` to end the run.
#[derive(Debug, PartialEq, Serialize, ToBytes)]
#[encoding(Json)]
pub enum PluginMessage {
    Choice {
        name: String,
        query: String,
        choices: Vec<String>,
    },
    Exit,
}

/// Pocket Sync's reply to a question: the answer, or `Kill`. `Kill` only arrives while a question is
/// waiting; Pocket Sync's Stop sends nothing and cuts the plugin off, even mid-call.
#[derive(Debug, Deserialize, FromBytes)]
#[encoding(Json)]
pub enum HostMessage {
    Answer { name: String, value: String },
    Kill,
}

pub const MENU: &str = "menu";

/// A choice on the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuChoice {
    SyncNow,
    Settings,
    Exit,
}

impl MenuChoice {
    const ALL: [MenuChoice; 3] = [MenuChoice::SyncNow, MenuChoice::Settings, MenuChoice::Exit];

    fn key(self) -> Key {
        match self {
            MenuChoice::SyncNow => Key::SyncNow,
            MenuChoice::Settings => Key::Settings,
            MenuChoice::Exit => Key::Exit,
        }
    }
}

pub fn menu(strings: &Strings) -> PluginMessage {
    PluginMessage::Choice {
        name: MENU.into(),
        query: strings.get(Key::MenuQuery).into(),
        choices: MenuChoice::ALL
            .into_iter()
            .map(|choice| strings.get(choice.key()).into())
            .collect(),
    }
}

/// The menu choice labelled `value` in `strings`' language. Pocket Sync sends back the label, not the
/// choice.
pub fn menu_choice(strings: &Strings, value: &str) -> Option<MenuChoice> {
    MenuChoice::ALL
        .into_iter()
        .find(|choice| strings.get(choice.key()) == value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use extism_pdk::json::{from_str, json, to_value};
    use std::collections::BTreeSet;

    // Pocket Sync has its own copies of these enums, so the JSON is pinned here.

    #[test]
    fn messages_are_sent_as_pocket_sync_reads_them() {
        assert_eq!(
            to_value(menu(&Strings::new("en-US"))).unwrap(),
            json!({"Choice": {
                "name": "menu",
                "query": "What would you like to do?",
                "choices": ["Sync now", "Settings", "Exit"],
            }})
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
    fn menu_answers_are_matched_to_their_choice() {
        let english = Strings::new("en-US");
        for choice in MenuChoice::ALL {
            assert_eq!(
                menu_choice(&english, english.get(choice.key())),
                Some(choice)
            );
        }
        assert_eq!(menu_choice(&english, "Nope"), None);
    }

    // Otherwise pressing the second of two same-labelled buttons would run the first one's action.
    #[test]
    fn menu_labels_differ_in_every_language() {
        for (tag, _) in strings::LOCALES {
            let language = Strings::new(tag);
            let labels: BTreeSet<_> = MenuChoice::ALL
                .into_iter()
                .map(|choice| language.get(choice.key()))
                .collect();
            assert_eq!(labels.len(), MenuChoice::ALL.len(), "{tag}: {labels:?}");
        }
    }

    #[test]
    fn menu_answers_are_matched_in_the_language_shown() {
        let locales = [
            (
                "en",
                r#"{"menu": {"sync_now": "Sync now", "settings": "Settings", "exit": "Exit"}}"#,
            ),
            ("de", r#"{"menu": {"exit": "Beenden"}}"#),
        ];
        let german = Strings::with_locales("de-DE", &locales);
        assert_eq!(menu_choice(&german, "Beenden"), Some(MenuChoice::Exit));
        assert_eq!(menu_choice(&german, "Exit"), None);
        // Untranslated labels are shown, and matched, in English.
        assert_eq!(menu_choice(&german, "Settings"), Some(MenuChoice::Settings));
    }
}
