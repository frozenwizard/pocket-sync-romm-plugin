//! Every string the user sees, looked up in the bundled `locales/<lang>/index.json` files. They follow
//! Pocket Sync's layout, so its translators and tools (inlang, Fink) work here too.

use std::collections::HashMap;

use extism_pdk::json::{self, Value};

/// The bundled translations, by language tag. English has every string; the others may leave some out
/// or empty, and those show in English.
pub(super) const LOCALES: &[(&str, &str)] = &[(
    "en",
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/locales/en/index.json"
    )),
)];

/// A string the user sees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    MenuQuery,
    SyncNow,
    Settings,
    Exit,
    SyncNotReady,
    SettingsNotReady,
}

impl Key {
    #[cfg(test)]
    const ALL: [Key; 6] = [
        Key::MenuQuery,
        Key::SyncNow,
        Key::Settings,
        Key::Exit,
        Key::SyncNotReady,
        Key::SettingsNotReady,
    ];

    /// Its path in the locale files.
    fn path(self) -> &'static str {
        match self {
            Key::MenuQuery => "menu.query",
            Key::SyncNow => "menu.sync_now",
            Key::Settings => "menu.settings",
            Key::Exit => "menu.exit",
            Key::SyncNotReady => "log.sync_not_ready",
            Key::SettingsNotReady => "log.settings_not_ready",
        }
    }
}

/// The strings for one language.
pub struct Strings(HashMap<String, String>);

impl Strings {
    /// The strings for `locale`, such as `de-AT`. Each comes from the most specific bundled file that
    /// has it, so `de-AT` falls back to `de`, then English.
    pub fn new(locale: &str) -> Self {
        Self::with_locales(locale, LOCALES)
    }

    pub(super) fn with_locales(locale: &str, locales: &[(&str, &str)]) -> Self {
        let mut strings = HashMap::new();
        for tag in fallbacks(locale).iter().rev() {
            if let Some((_, file)) = locales.iter().find(|(t, _)| t == tag) {
                flatten(
                    "",
                    json::from_str(file).expect("locale files are JSON"),
                    &mut strings,
                );
            }
        }
        Strings(strings)
    }

    /// The string for `key`. Panics if English doesn't have it, which the tests rule out.
    pub fn get(&self, key: Key) -> &str {
        self.0
            .get(key.path())
            .unwrap_or_else(|| panic!("English has no {}", key.path()))
    }
}

/// The tags to look in, most specific first: `zh-Hans-CN` gives `zh-Hans-CN`, `zh-Hans`, `zh`, `en`.
/// Takes `_` as well as `-`.
fn fallbacks(locale: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut tag = locale.replace('_', "-");
    while !tag.is_empty() {
        tags.push(tag.clone());
        tag.truncate(tag.rfind('-').unwrap_or(0));
    }
    if !tags.iter().any(|tag| tag == "en") {
        tags.push("en".into());
    }
    tags
}

/// Adds the strings in `value` to `strings` under dotted paths, such as `menu.query`.
fn flatten(path: &str, value: Value, strings: &mut HashMap<String, String>) {
    match value {
        // Translation tools write untranslated strings as "", so it falls back like a missing one.
        Value::String(string) if string.is_empty() => {}
        Value::String(string) => {
            strings.insert(path.into(), string);
        }
        Value::Object(object) => {
            for (name, value) in object {
                let path = if path.is_empty() {
                    name
                } else {
                    format!("{path}.{name}")
                };
                flatten(&path, value, strings);
            }
        }
        // Locale files hold only objects and strings.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_fall_back_to_shorter_tags_then_english() {
        assert_eq!(
            fallbacks("zh-Hans-CN"),
            ["zh-Hans-CN", "zh-Hans", "zh", "en"]
        );
        assert_eq!(fallbacks("en_US"), ["en-US", "en"]);
        assert_eq!(fallbacks("en"), ["en"]);
        assert_eq!(fallbacks(""), ["en"]);
    }

    #[test]
    fn each_string_comes_from_the_most_specific_file_that_has_it() {
        let locales = [
            ("en", r#"{"a": {"b": "color", "c": "gray"}}"#),
            ("en-GB", r#"{"a": {"b": "colour"}}"#),
        ];
        let british = Strings::with_locales("en-GB", &locales);
        assert_eq!((&*british.0["a.b"], &*british.0["a.c"]), ("colour", "gray"));
        let french = Strings::with_locales("fr-FR", &locales);
        assert_eq!(french.0["a.b"], "color");
    }

    #[test]
    fn empty_strings_fall_back_to_english() {
        let locales = [("en", r#"{"a": "Exit"}"#), ("de", r#"{"a": ""}"#)];
        assert_eq!(Strings::with_locales("de", &locales).0["a"], "Exit");
    }

    // Every language falls back to English, so this is what keeps `get` from panicking.
    #[test]
    fn english_has_every_string() {
        let english = Strings::new("en");
        for key in Key::ALL {
            assert!(
                english.0.contains_key(key.path()),
                "{key:?} is missing or empty in English"
            );
        }
    }

    #[test]
    fn translations_only_have_strings_english_has() {
        let english = Strings::new("en");
        for (tag, file) in LOCALES {
            let mut strings = HashMap::new();
            flatten("", json::from_str(file).unwrap(), &mut strings);
            for path in strings.keys() {
                assert!(english.0.contains_key(path), "{tag}: {path}");
            }
        }
    }
}
