//! The interface's words. English is the key, as the code writes it (`t("Properties")`), and the catalogs of
//! `translations/ui/*.po` (the ones the egui front end reads) say it in the interface's language; a message a catalog hasn't
//! finished comes back in English. The same four calls as `bimcraft-ui-egui`'s `lang`: [`t`], [`tf`], [`tc`] and [`set`].
//!
//! The language is the thread's, as in the egui crate: a window draws on its own thread.

use std::cell::Cell;

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

thread_local! {
    /// The catalog this thread's window speaks (`"en"` is no catalog: the English the code writes).
    static CODE: Cell<&'static str> = const { Cell::new("en") };
}

/// The catalog codes the interface has: English, then one for each `translations/ui/<code>.po`.
const CODES: [&str; 12] = ["en", "pt", "es", "pl", "cs", "fr", "de", "nl", "it", "vi", "zh", "zh_TW"];

/// The code of the catalog a system locale asks for (`pt-BR`, `es_419.UTF-8`, `zh-TW`, `vi_VN`), English when there is none:
/// the language's two letters, except Chinese, whose traditional locales (TW, HK, MO, Hant) have a catalog of their own.
pub fn code_of(locale: &str) -> &'static str {
    let lower = locale.trim().to_lowercase();
    let parts: Vec<&str> = lower.split(['-', '_', '.', '@']).collect();
    let first = parts.first().copied().unwrap_or_default();
    if first == "zh" {
        return if parts.iter().any(|p| matches!(*p, "tw" | "hk" | "mo" | "hant")) { "zh_TW" } else { "zh" };
    }
    CODES.iter().copied().find(|c| *c == first).unwrap_or("en")
}

/// Speaks `locale`'s language from now on, on this thread; an unknown one is English.
pub fn set(locale: &str) {
    CODE.with(|c| c.set(code_of(locale)));
}

/// The code of the system's language (`LC_ALL`, `LC_MESSAGES` or `LANG` on Linux; the OS setting elsewhere).
pub fn from_system() -> &'static str {
    sys_locale::get_locale().map_or("en", |l| code_of(&l))
}

/// `english` in the interface's language.
pub fn t(english: &str) -> &str {
    tc("", english)
}

/// A message whose meaning a PO `msgctxt` tells apart from another's.
pub fn tc<'a>(context: &str, english: &'a str) -> &'a str {
    let code = CODE.with(Cell::get);
    if code == "en" {
        return english;
    }
    bimcraft_i18n::translated(CATALOGS, code, context, english).unwrap_or(english)
}

/// `template` translated, its `{}`s filled with `args` in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    args.iter().fold(t(template).to_string(), |s, a| s.replacen("{}", a, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locale_names_its_catalog() {
        assert_eq!(code_of("pt-BR"), "pt");
        assert_eq!(code_of("es_419.UTF-8"), "es");
        assert_eq!(code_of("vi_VN"), "vi");
        assert_eq!(code_of("zh-CN"), "zh");
        assert_eq!(code_of("zh_TW.UTF-8"), "zh_TW");
        assert_eq!(code_of("zh-Hant"), "zh_TW");
        assert_eq!(code_of("pl"), "pl");
        assert_eq!(code_of("tlh"), "en", "a language with no catalog is English");
        assert_eq!(code_of(""), "en");
    }

    #[test]
    fn a_finished_message_is_translated_and_an_unfinished_one_falls_back_to_english() {
        set("en");
        assert_eq!(t("Properties"), "Properties");
        set("pt-BR");
        assert_eq!(t("Properties"), "Propriedades");
        set("pl");
        assert_eq!(t("Properties"), "Właściwości");
        set("zh-CN");
        assert_eq!(t("Properties"), "属性");
        assert_eq!(t("A message no catalog has, written for this test"), "A message no catalog has, written for this test");
        set("en");
    }

    #[test]
    fn a_template_has_its_holes_filled_in_order() {
        set("en");
        assert_eq!(tf("{} of {}", &["2", "5"]), "2 of 5");
        assert_eq!(tf("no holes", &["x"]), "no holes");
    }
}
