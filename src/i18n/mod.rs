//! UI translations. The English text is the key (gettext style): `tr("Settings")` returns the
//! text of the active language and falls back to English for anything its tables don't have.
//! The log stays English on purpose (support/debugging).
//!
//! A language is data: a code (`"de"`), its name in its own language (`"Deutsch"`) and a table
//! (one file with one `match`, like `de.rs`). The core's languages are listed in [`SHIPPED`];
//! English is the source language and has no table. Code outside this crate adds texts to a
//! language – or a language of its own – with [`register`] and checks them in a test with
//! [`untranslated`].

#[cfg(not(test))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

mod de;

/// A table of one language's texts: the text of an English one, if it has one (e.g. a function
/// with one `match`, like this crate's `de.rs`).
pub type Table = fn(&str) -> Option<&'static str>;

/// The code of the source language, whose texts are the keys.
pub const SOURCE: &str = "en";

/// The key under which a table that adds a language (one the core doesn't ship) gives that
/// language's name in its own language, for the language picker; without it the code is shown.
pub const NATIVE_NAME: &str = "@native-name";

/// The languages the core ships: code, name in its own language, table (`None`: the source
/// language). A new language is its table file and one line here; every one is complete (a test
/// checks each UI text).
const SHIPPED: &[(&str, &str, Option<Table>)] = &[(SOURCE, "English", None), ("de", "Deutsch", Some(de::table))];

/// A language the app has: shipped by the core or added with [`register`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lang {
    /// Lower case, e.g. `"de"` or `"pt-br"`; a macOS locale like `de-AT` matches it by prefix.
    pub code: &'static str,
    /// Its name in its own language, e.g. `"Deutsch"`.
    pub name: &'static str,
}

/// Tables added with [`register`], by language code, in the order they were added.
static TABLES: RwLock<Vec<(&'static str, Table)>> = RwLock::new(Vec::new());

fn tables() -> std::sync::RwLockReadGuard<'static, Vec<(&'static str, Table)>> {
    TABLES.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Adds a table of texts in the language `code`, e.g. for the pages another crate adds to the
/// app. [`tr`] and [`tr_detail`] look in the core's table of that language first, then in the
/// added ones in the order they were added. A code the core doesn't ship adds that language to
/// the picker (named by the table's [`NATIVE_NAME`] text). Call it once per table, before the
/// UI is shown.
pub fn register(code: &'static str, table: Table) {
    TABLES.write().unwrap_or_else(std::sync::PoisonError::into_inner).push((code, table));
}

/// Every language the app has: the shipped ones (English first), then the ones added with
/// [`register`] in the order their first table came.
#[must_use]
pub fn languages() -> Vec<Lang> {
    let mut out: Vec<Lang> = SHIPPED.iter().map(|&(code, name, _)| Lang { code, name }).collect();
    let tables = tables();
    for &(code, _) in tables.iter() {
        if !out.iter().any(|l| l.code == code) {
            let name = tables.iter().filter(|(c, _)| *c == code).find_map(|(_, table)| table(NATIVE_NAME));
            out.push(Lang { code, name: name.unwrap_or(code) });
        }
    }
    out
}

/// The text of `en` in the language `code`: the core's table, then the registered ones.
/// `None` for English (the source) and for texts no table has.
fn text(code: &str, en: &str) -> Option<&'static str> {
    if code == SOURCE {
        return None;
    }
    SHIPPED
        .iter()
        .find(|l| l.0 == code)
        .and_then(|l| l.2)
        .and_then(|table| table(en))
        .or_else(|| tables().iter().filter(|(c, _)| *c == code).find_map(|(_, table)| table(en)))
}

/// Language setting as stored in the settings file: `"system"` (follow macOS) or a language code.
/// A code this build doesn't have is kept (and saved again) and resolves like `"system"`; the
/// names of older builds (`"english"`, `"german"`) read as their codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct Language(String);

impl Default for Language {
    fn default() -> Self {
        Self::system()
    }
}

impl From<String> for Language {
    fn from(value: String) -> Self {
        match value.as_str() {
            "english" => Self(SOURCE.into()),
            "german" => Self("de".into()),
            _ => Self(value),
        }
    }
}

impl From<Language> for String {
    fn from(language: Language) -> Self {
        language.0
    }
}

impl Language {
    const SYSTEM: &'static str = "system";

    /// Follow the macOS language.
    #[must_use]
    pub fn system() -> Self {
        Self(Self::SYSTEM.into())
    }

    /// The language with this code (or `"system"`), e.g. `Language::new("de")`.
    #[must_use]
    pub fn new(code: &str) -> Self {
        Self::from(code.to_string())
    }

    /// The stored value: `"system"` or a code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The choices of the language picker: System, then every language the app has.
    #[must_use]
    pub fn choices() -> Vec<Self> {
        std::iter::once(Self::system()).chain(languages().into_iter().map(|l| Self(l.code.into()))).collect()
    }

    /// Name shown in the language picker (each language in its own language; System, and a code
    /// this build doesn't have, as "System" in the active language).
    #[must_use]
    pub fn label(&self) -> &'static str {
        languages().into_iter().find(|l| l.code == self.0).map_or_else(|| tr("System"), |l| l.name)
    }

    /// The index in [`languages`] this setting means.
    fn resolve(&self) -> usize {
        self.resolve_with(sys_locale::get_locale().as_deref())
    }

    /// [`Self::resolve`] with the macOS locale `locale`.
    fn resolve_with(&self, locale: Option<&str>) -> usize {
        let languages = languages();
        languages.iter().position(|l| l.code == self.0).unwrap_or_else(|| locale_index(&languages, locale))
    }
}

/// The language a macOS locale (e.g. `de-AT`) means: the longest code it is or starts with
/// (followed by `-` or `_`), else English.
fn locale_index(languages: &[Lang], locale: Option<&str>) -> usize {
    let Some(locale) = locale.map(|l| l.to_lowercase().replace('_', "-")) else { return 0 };
    languages
        .iter()
        .enumerate()
        .filter(|(_, l)| locale == l.code || locale.starts_with(&format!("{}-", l.code)))
        .max_by_key(|(_, l)| l.code.len())
        .map_or(0, |(i, _)| i)
}

#[cfg(not(test))]
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
thread_local! {
    /// Unit tests: the language of this test thread, so tests that switch it (the UI tests, too)
    /// can't change it for the tests running beside them.
    static ACTIVE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Switch the UI language.
pub fn set(language: &Language) {
    let index = language.resolve();
    #[cfg(not(test))]
    ACTIVE.store(index, Ordering::Relaxed);
    #[cfg(test)]
    ACTIVE.set(index);
}

/// The code of the active language, e.g. `"en"`.
#[must_use]
pub fn active() -> &'static str {
    #[cfg(not(test))]
    let index = ACTIVE.load(Ordering::Relaxed);
    #[cfg(test)]
    let index = ACTIVE.get();
    match SHIPPED.get(index) {
        Some(l) => l.0,
        None => languages().get(index).map_or(SOURCE, |l| l.code),
    }
}

/// The text of `en` in the active language, if it has one.
fn active_text(en: &str) -> Option<&'static str> {
    text(active(), en)
}

/// Translate a fixed UI text.
#[must_use]
pub fn tr(en: &'static str) -> &'static str {
    active_text(en).unwrap_or(en)
}

/// Whether `text` is `en` in any of the languages (e.g. a default name the user typed in German).
#[must_use]
pub fn is_text(en: &str, text: &str) -> bool {
    text == en || languages().iter().any(|l| self::text(l.code, en) == Some(text))
}

/// Translate a status text that may carry a trailing detail in parentheses,
/// e.g. "Connected (127.0.0.1:921)" → "Verbunden (127.0.0.1:921)".
#[must_use]
pub fn tr_detail(text: &str) -> String {
    if let Some(t) = active_text(text) {
        return t.to_string();
    }
    if let Some(i) = text.rfind(" (")
        && text.ends_with(')')
        && let Some(t) = active_text(&text[..i])
    {
        return format!("{t}{}", &text[i..]);
    }
    text.to_string()
}

/// The UI texts in `sources` (Rust source code) passed to `tr("…")` that have no text in the
/// language `code`, neither in this crate's table nor in a registered one (none for English, the
/// source). For a test of a crate that adds UI texts: register its tables, then for each code
/// `assert!(i18n::untranslated("de", &[include_str!("page.rs")]).is_empty())`.
#[must_use]
pub fn untranslated(code: &str, sources: &[&str]) -> Vec<String> {
    if code == SOURCE {
        return Vec::new();
    }
    sources
        .iter()
        .flat_map(|src| literals(src, "tr("))
        .map(|t| t.replace("\\n", "\n"))
        .filter(|t| text(code, t).is_none())
        .collect()
}

/// String literals that directly follow `prefix` in `src`, e.g. every `tr("…")`.
fn literals<'a>(src: &'a str, prefix: &str) -> Vec<&'a str> {
    let pat = format!("{prefix}\"");
    src.match_indices(&pat)
        .filter_map(|(i, _)| {
            let rest = &src[i + pat.len()..];
            rest.find("\")").map(|end| &rest[..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every source file of the UI (`src/ui/`, with its subfolders).
    fn ui_sources() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        let mut out = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui"), &mut out);
        assert!(out.len() >= 6, "the UI's files are found");
        out
    }

    /// Every language the core ships has every UI text.
    #[test]
    fn every_ui_text_has_a_translation_in_every_shipped_language() {
        let sources = ui_sources();
        let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
        let gui = sources.concat();
        let bridge = [include_str!("../engine.rs"), include_str!("../gspro.rs")].concat();
        let mut checked = 0;
        for &(code, _, _) in SHIPPED.iter().filter(|l| l.0 != SOURCE) {
            let missing_in = |texts: Vec<&str>| -> Vec<String> {
                texts.into_iter().filter(|t| text(code, t).is_none()).map(String::from).collect()
            };
            let mut missing = untranslated(code, &sources);
            missing.extend(missing_in(literals(&gui, "send_now(")));
            // Status details set by the bridge threads.
            for prefix in ["R10Status::Searching, ", "R10Status::Connected, ", "R10Status::Ready, ", "set_gspro(false, "] {
                missing.extend(missing_in(literals(&bridge, prefix)));
            }
            // Notification texts, including formatted ones like "R10 battery low ({level} %)".
            for kind in ["BatteryLow", "DeviceError", "R10Lost", "GsproLost", "ShotNotDelivered"] {
                for prefix in [format!("Alert::{kind}, "), format!("Alert::{kind}, &format!(")] {
                    let texts = literals(&bridge, &prefix).into_iter().map(|t| t.split(" (").next().unwrap_or(t)).collect();
                    missing.extend(missing_in(texts));
                }
            }
            assert!(missing.is_empty(), "missing texts in {code}: {missing:#?}");
            checked += 1;
        }
        assert!(checked >= 1, "German is checked");
    }

    /// A crate that adds UI texts registers its own table; the core table comes first.
    #[test]
    fn registered_tables_add_texts() {
        fn extension(en: &str) -> Option<&'static str> {
            Some(match en {
                "Only in an extension" => "Nur in einer Erweiterung",
                "SETTINGS" => "NICHT DIESE",
                _ => return None,
            })
        }
        let page = r#"ui.label(tr("Only in an extension")); ui.label(tr("SETTINGS"));"#;
        assert_eq!(untranslated("de", &[page]), ["Only in an extension"]);
        assert!(untranslated(SOURCE, &[page]).is_empty(), "English is the source");
        register("de", extension);
        assert!(untranslated("de", &[page]).is_empty());
        set(&Language::new("de"));
        assert_eq!(tr("Only in an extension"), "Nur in einer Erweiterung");
        assert_eq!(tr_detail("Only in an extension (2)"), "Nur in einer Erweiterung (2)");
        assert_eq!(tr("SETTINGS"), "EINSTELLUNGEN");
        set(&Language::new("en"));
        assert_eq!(tr("Only in an extension"), "Only in an extension");
    }

    /// A language the core doesn't ship comes with a table alone: it is in the picker and `tr`
    /// uses it, with English for what it lacks; no feature code changes.
    #[test]
    fn a_registered_table_adds_a_language() {
        fn klingon(en: &str) -> Option<&'static str> {
            Some(match en {
                NATIVE_NAME => "tlhIngan Hol",
                "SETTINGS" => "SETTINGS-XX",
                "Connected" => "Connected-XX",
                _ => return None,
            })
        }
        register("xx", klingon);
        assert!(languages().contains(&Lang { code: "xx", name: "tlhIngan Hol" }));
        let choices = Language::choices();
        assert!(choices.contains(&Language::new("xx")));
        assert_eq!(Language::new("xx").label(), "tlhIngan Hol");
        set(&Language::new("xx"));
        assert_eq!(active(), "xx");
        assert_eq!(tr("SETTINGS"), "SETTINGS-XX");
        assert_eq!(tr_detail("Connected (127.0.0.1:921)"), "Connected-XX (127.0.0.1:921)");
        assert_eq!(tr("Ball flight"), "Ball flight", "English where the table has no text");
        assert!(is_text("SETTINGS", "SETTINGS-XX"));
        assert_eq!(untranslated("xx", &[r#"tr("SETTINGS") tr("Ball flight")"#]), ["Ball flight"]);
        set(&Language::new("en"));
    }

    #[test]
    fn the_picker_lists_system_and_each_language_in_its_own_language() {
        let labels: Vec<&str> = Language::choices().iter().map(Language::label).collect();
        assert_eq!(labels[..3], ["System", "English", "Deutsch"]);
        set(&Language::new("de"));
        assert_eq!(Language::system().label(), "System");
        assert_eq!(Language::new("de").label(), "Deutsch");
        assert_eq!(Language::new("en").label(), "English");
        set(&Language::new("en"));
    }

    #[test]
    fn the_system_locale_picks_a_language_by_prefix() {
        let languages = languages();
        let de = languages.iter().position(|l| l.code == "de").unwrap();
        assert_eq!(locale_index(&languages, Some("de-AT")), de);
        assert_eq!(locale_index(&languages, Some("de_DE")), de);
        assert_eq!(locale_index(&languages, Some("DE")), de);
        assert_eq!(locale_index(&languages, Some("ja-JP")), 0, "English");
        assert_eq!(locale_index(&languages, Some("dee")), 0, "a prefix of the code only before '-'");
        assert_eq!(locale_index(&languages, None), 0);
        // The longest code wins.
        let regional = [Lang { code: "en", name: "English" }, Lang { code: "pt", name: "Português" }, Lang { code: "pt-br", name: "Português (Brasil)" }];
        assert_eq!(locale_index(&regional, Some("pt-BR")), 2);
        assert_eq!(locale_index(&regional, Some("pt-PT")), 1);
    }

    /// A code this build doesn't have resolves like System.
    #[test]
    fn an_unknown_code_follows_the_system() {
        let de = languages().iter().position(|l| l.code == "de").unwrap();
        assert_eq!(Language::system().resolve_with(Some("de-AT")), de);
        assert_eq!(Language::new("fr").resolve_with(Some("de-AT")), de);
        assert_eq!(Language::new("en").resolve_with(Some("de-AT")), 0, "a chosen language beats the locale");
        assert_eq!(Language::new("fr").as_str(), "fr", "kept");
        assert_eq!(Language::new("german"), Language::new("de"));
        assert_eq!(Language::new("english"), Language::new("en"));
    }

    #[test]
    fn texts_are_recognised_in_every_language() {
        assert!(is_text("Ball flight", "Ball flight"));
        assert!(is_text("Ball flight", "Ballflug"));
        assert!(!is_text("Ball flight", "Range"));
    }

    #[test]
    fn details_keep_their_suffix() {
        set(&Language::new("de"));
        assert_eq!(tr_detail("Connected (127.0.0.1:921)"), "Verbunden (127.0.0.1:921)");
        assert_eq!(
            tr_detail("R10 is tilted – please level it (Roll 1.0°, Pitch 4.2°)"),
            "R10 steht schief – bitte gerade ausrichten (Roll 1.0°, Pitch 4.2°)"
        );
        assert_eq!(tr_detail("Radar saturated (too close to wall/net?)"), "Radar übersteuert (zu nah an Wand/Netz?)");
        assert_eq!(tr_detail("Something new"), "Something new");
        assert_eq!(tr("Not in the table"), "Not in the table", "English where no table has a text");
        set(&Language::new("en"));
        assert_eq!(tr("SETTINGS"), "SETTINGS");
    }
}
