//! User-facing text as data (D-062).
//!
//! The core never builds English sentences for the user. It returns a [`Text`]: a key
//! into the catalogs (`locales/<language>.json` at the repository root, under `core`)
//! plus its arguments. The desktop UI translates it like its own messages; the core
//! renders it itself only where it draws the UI (native menus, notifications) or for
//! the CLI, in the language set with [`set_language`].
//!
//! Messages are created through the generated [`msg`] functions, one per catalog
//! entry, so keys and arguments are checked at compile time.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{Arc, OnceLock, RwLock},
};

use intl_pluralrules::{PluralCategory, PluralRuleType, PluralRules};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unic_langid::LanguageIdentifier;

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

/// Typed constructors for every message the core produces, generated from
/// `locales/en.json` (`core.*`).
#[allow(missing_docs, clippy::pedantic)]
pub mod msg {
    #[allow(unused_imports)]
    use super::Text;
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));
}

/// A message argument: a number (formatted for the language) or text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(untagged)]
pub enum Arg {
    /// A number, e.g. a count (a JavaScript number in the UI).
    Num(#[cfg_attr(feature = "specta", specta(type = f64))] i64),
    /// Anything else, already as text (hostnames, names, versions).
    Str(String),
}

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(n) => n.fmt(f),
            Self::Str(s) => f.write_str(s),
        }
    }
}

/// A message for the user: a catalog key and its arguments.
///
/// Also deserializes from a plain string, shown verbatim: that's how text stored
/// before messages had keys (activity history) still loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Text {
    /// The catalog key, e.g. `core.doctor.dnsMissing.title`.
    pub key: String,
    /// Values for the message's `{placeholders}`; `count` picks a plural form.
    pub args: BTreeMap<String, Arg>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TextRepr {
    Keyed {
        key: String,
        #[serde(default)]
        args: BTreeMap<String, Arg>,
    },
    Plain(String),
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match TextRepr::deserialize(deserializer)? {
            TextRepr::Keyed { key, args } => Self { key, args },
            TextRepr::Plain(text) => msg::raw(text),
        })
    }
}

impl Text {
    /// A message without arguments. Prefer the generated [`msg`] functions.
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            args: BTreeMap::new(),
        }
    }

    /// Adds an argument.
    #[must_use]
    pub fn arg(mut self, name: &str, value: impl fmt::Display) -> Self {
        self.args
            .insert(name.to_owned(), Arg::Str(value.to_string()));
        self
    }

    /// Adds the count that picks the plural form.
    #[must_use]
    pub fn count(mut self, count: u64) -> Self {
        self.args.insert(
            "count".to_owned(),
            Arg::Num(i64::try_from(count).unwrap_or(i64::MAX)),
        );
        self
    }

    /// The text in English, e.g. for logs, whatever the user's language.
    pub fn english(&self) -> String {
        render(&english(), self)
    }
}

/// Renders in the language set with [`set_language`] (English until then).
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render(&current(), self))
    }
}

/// Something with a message for the user, e.g. an error.
pub trait UserText {
    /// The message.
    fn text(&self) -> Text;
}

/// `Display` for types with a [`UserText`]: the message in English, so logs and bug
/// reports read the same whatever the user's language. User-facing surfaces use
/// [`UserText::text`] instead.
macro_rules! english_display {
    ($($ty:ty),+ $(,)?) => {$(
        impl ::std::fmt::Display for $ty {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&$crate::text::UserText::text(self).english())
            }
        }
    )+};
}
pub(crate) use english_display;

/// One language's messages.
struct Catalog {
    language: String,
    messages: HashMap<String, String>,
    plurals: Option<PluralRules>,
}

impl Catalog {
    fn load(language: &str, json: &str) -> Self {
        let mut messages = HashMap::new();
        if let Ok(value) = serde_json::from_str::<Value>(json) {
            flatten(&value, "", &mut messages);
        }
        let plurals = language
            .parse::<LanguageIdentifier>()
            .ok()
            .and_then(|id| PluralRules::create(id, PluralRuleType::CARDINAL).ok());
        Self {
            language: language.to_owned(),
            messages,
            plurals,
        }
    }

    fn category(&self, count: i64) -> &'static str {
        let category = self
            .plurals
            .as_ref()
            .and_then(|rules| rules.select(count).ok())
            .unwrap_or(if count == 1 {
                PluralCategory::ONE
            } else {
                PluralCategory::OTHER
            });
        match category {
            PluralCategory::ZERO => "zero",
            PluralCategory::ONE => "one",
            PluralCategory::TWO => "two",
            PluralCategory::FEW => "few",
            PluralCategory::MANY => "many",
            PluralCategory::OTHER => "other",
        }
    }
}

fn flatten(value: &Value, prefix: &str, out: &mut HashMap<String, String>) {
    match value {
        Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(child, &path, out);
            }
        }
        _ => {}
    }
}

fn english() -> Arc<Catalog> {
    static ENGLISH: OnceLock<Arc<Catalog>> = OnceLock::new();
    ENGLISH
        .get_or_init(|| {
            let json = CATALOGS
                .iter()
                .find(|(lang, _)| *lang == "en")
                .map_or("{}", |(_, json)| json);
            Arc::new(Catalog::load("en", json))
        })
        .clone()
}

fn active() -> &'static RwLock<Option<Arc<Catalog>>> {
    static ACTIVE: RwLock<Option<Arc<Catalog>>> = RwLock::new(None);
    &ACTIVE
}

fn current() -> Arc<Catalog> {
    active()
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_else(english)
}

/// Languages with a catalog, e.g. `["de", "en"]`.
pub fn languages() -> Vec<&'static str> {
    CATALOGS.iter().map(|(lang, _)| *lang).collect()
}

/// The best catalog for `wanted` (BCP 47 tags or POSIX locales like `de_CH.UTF-8`,
/// most preferred first): an exact match, then the tag with subtags removed; `en`
/// when none matches.
pub fn pick_language(wanted: &[String], available: &[&str]) -> String {
    for tag in wanted {
        let mut candidate = tag
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('_', "-");
        while !candidate.is_empty() {
            if let Some(found) = available
                .iter()
                .find(|a| a.eq_ignore_ascii_case(&candidate))
            {
                return (*found).to_owned();
            }
            candidate = candidate
                .rsplit_once('-')
                .map(|(head, _)| head.to_owned())
                .unwrap_or_default();
        }
    }
    "en".to_owned()
}

/// Renders messages in the best available language for `wanted` from now on; returns
/// the language chosen.
pub fn set_language(wanted: &[String]) -> String {
    let language = pick_language(wanted, &languages());
    let catalog = if language == "en" {
        english()
    } else {
        let json = CATALOGS
            .iter()
            .find(|(lang, _)| *lang == language)
            .map_or("{}", |(_, json)| json);
        Arc::new(Catalog::load(&language, json))
    };
    if let Ok(mut slot) = active().write() {
        *slot = Some(catalog);
    }
    language
}

/// The language messages render in.
pub fn language() -> String {
    current().language.clone()
}

fn lookup<'a>(catalog: &'a Catalog, english: &'a Catalog, text: &Text) -> Option<&'a str> {
    if let Some(Arg::Num(count)) = text.args.get("count") {
        let variant = |c: &'a Catalog| {
            c.messages
                .get(&format!("{}_{}", text.key, c.category(*count)))
                .or_else(|| c.messages.get(&format!("{}_other", text.key)))
        };
        if let Some(found) = variant(catalog).or_else(|| variant(english)) {
            return Some(found);
        }
    }
    catalog
        .messages
        .get(&text.key)
        .or_else(|| english.messages.get(&text.key))
        .map(String::as_str)
}

fn render(catalog: &Catalog, text: &Text) -> String {
    let english = english();
    let Some(message) = lookup(catalog, &english, text) else {
        // Unreachable for generated messages; keeps a bad key visible, not silent.
        return text.key.clone();
    };
    let mut out = String::with_capacity(message.len() + 16);
    let mut rest = message;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if text.args.contains_key(&after[..end]) => {
                out.push_str(&text.args[&after[..end]].to_string());
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_closest_language() {
        let available = ["de", "en", "pt-BR"];
        let pick = |tags: &[&str]| {
            pick_language(
                &tags.iter().map(ToString::to_string).collect::<Vec<_>>(),
                &available,
            )
        };
        assert_eq!(pick(&["de-CH", "fr"]), "de");
        assert_eq!(pick(&["pt_BR.UTF-8"]), "pt-BR");
        assert_eq!(pick(&["pt-PT", "de"]), "de");
        assert_eq!(pick(&["ja", "C"]), "en");
        assert_eq!(pick(&[]), "en");
    }

    #[test]
    fn renders_placeholders_and_plurals() {
        let catalog = Catalog::load(
            "en",
            r#"{"a": {"b": "{name} has {thing}", "n_one": "{count} route", "n_other": "{count} routes"}}"#,
        );
        let render = |text: &Text| render(&catalog, text);
        assert_eq!(
            render(&Text::new("a.b").arg("name", "Mac").arg("thing", 3)),
            "Mac has 3"
        );
        assert_eq!(render(&Text::new("a.n").count(1)), "1 route");
        assert_eq!(render(&Text::new("a.n").count(0)), "0 routes");
        // A missing argument stays visible rather than vanishing.
        assert_eq!(
            render(&Text::new("a.b").arg("name", "Mac")),
            "Mac has {thing}"
        );
        assert_eq!(render(&Text::new("nope")), "nope");
    }

    #[test]
    fn uses_the_languages_plural_rules() {
        let polish = Catalog::load(
            "pl",
            r#"{"n_one": "{count} trasa", "n_few": "{count} trasy", "n_many": "{count} tras", "n_other": "{count} trasy"}"#,
        );
        let render = |n| render(&polish, &Text::new("n").count(n));
        assert_eq!(render(1), "1 trasa");
        assert_eq!(render(3), "3 trasy");
        assert_eq!(render(5), "5 tras");
    }

    #[test]
    fn generated_constructors_carry_key_and_arguments() {
        let text = msg::error::plan::no_zone("a.xyz.com");
        assert_eq!(text.key, "core.error.plan.noZone");
        assert_eq!(text.args["hostname"], Arg::Str("a.xyz.com".into()));
        assert!(
            text.english()
                .starts_with("a.xyz.com isn't in any of this account's domains")
        );
        assert_eq!(msg::raw("as is").english(), "as is");
    }

    #[test]
    fn serializes_for_the_ui() {
        let text = Text::new("core.x").arg("host", "a.b").count(2);
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            serde_json::json!({"key": "core.x", "args": {"host": "a.b", "count": 2}})
        );
        let back: Text = serde_json::from_value(serde_json::to_value(&text).unwrap()).unwrap();
        assert_eq!(back, text);
        // Stored before messages had keys: shown as it was.
        let legacy: Text = serde_json::from_value(serde_json::json!("Couldn't do it")).unwrap();
        assert_eq!(legacy.english(), "Couldn't do it");
    }
}
