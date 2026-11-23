//! The messages exchanged with Pocket Sync, and the English strings in them, kept in one place for
//! later translation.

use extism_pdk::{FromBytes, Json, ToBytes};
use serde::{Deserialize, Serialize};

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
pub const SYNC_NOW: &str = "Sync now";
pub const SETTINGS: &str = "Settings";
pub const EXIT: &str = "Exit";

pub const SYNC_NOT_READY: &str = "Syncing isn't available yet.";
pub const SETTINGS_NOT_READY: &str = "Settings aren't available yet.";

pub fn menu() -> PluginMessage {
    PluginMessage::Choice {
        name: MENU.into(),
        query: "What would you like to do?".into(),
        choices: vec![SYNC_NOW.into(), SETTINGS.into(), EXIT.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use extism_pdk::json::{from_str, json, to_value};

    // Pocket Sync has its own copies of these enums, so the JSON is pinned here.

    #[test]
    fn messages_are_sent_as_pocket_sync_reads_them() {
        assert_eq!(
            to_value(menu()).unwrap(),
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
}
