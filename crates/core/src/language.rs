//! A project's language: the words its drawings print (a stair's UP, a title block's SCALE, a
//! schedule's headings) and the names commands give what they make (levels, views, sheets, rooms).
//! English, Brazilian Portuguese, Spanish, Swedish, Czech, French, German, Dutch or Italian. Names already given stay as they
//! are; the core's messages, which agents read, stay English.

use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "pt")]
    Portuguese,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "sv")]
    Swedish,
    #[serde(rename = "cs")]
    Czech,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "nl")]
    Dutch,
    #[serde(rename = "it")]
    Italian,
}

impl Language {
    pub const ALL: [Language; 9] = [
        Language::English,
        Language::Portuguese,
        Language::Spanish,
        Language::Swedish,
        Language::Czech,
        Language::French,
        Language::German,
        Language::Dutch,
        Language::Italian,
    ];

    /// Its code in commands and files: en, pt, es, sv, cs, fr, de, nl, it.
    pub fn code(self) -> &'static str {
        self.of(["en", "pt", "es", "sv", "cs", "fr", "de", "nl", "it"])
    }

    /// Its name, in itself.
    pub fn label(self) -> &'static str {
        self.of(["English", "Português (Brasil)", "Español", "Svenska", "Čeština", "Français", "Deutsch", "Nederlands", "Italiano"])
    }

    /// Of one thing in each language, English first, this language's.
    pub fn of<T>(self, [english, portuguese, spanish, swedish, czech, french, german, dutch, italian]: [T; 9]) -> T {
        match self {
            Language::English => english,
            Language::Portuguese => portuguese,
            Language::Spanish => spanish,
            Language::Swedish => swedish,
            Language::Czech => czech,
            Language::French => french,
            Language::German => german,
            Language::Dutch => dutch,
            Language::Italian => italian,
        }
    }

    /// A language by its code or its name, or from a locale as systems give one: pt-BR, es_419.UTF-8, sv-SE, cs-CZ, fr-FR, de-DE, nl-BE, it-IT.
    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.trim().to_lowercase();
        Some(match code.split(['-', '_', '.']).next().unwrap_or_default() {
            "en" | "english" => Language::English,
            "pt" | "portuguese" | "português" => Language::Portuguese,
            "es" | "spanish" | "español" => Language::Spanish,
            "sv" | "swedish" | "svenska" => Language::Swedish,
            "cs" | "cz" | "czech" | "čeština" | "cestina" => Language::Czech,
            "fr" | "french" | "français" | "francais" => Language::French,
            "de" | "german" | "deutsch" => Language::German,
            "nl" | "dutch" | "nederlands" | "vlaams" | "flemish" => Language::Dutch,
            "it" | "italian" | "italiano" => Language::Italian,
            _ => return None,
        })
    }

    /// `english`, a word or a name the core writes, in this language.
    pub fn word(self, english: &str) -> &str {
        self.translated(english).unwrap_or(english)
    }

    /// `english` in this language, when it isn't English and the core's words have it.
    pub fn translated(self, english: &str) -> Option<&'static str> {
        if self == Language::English {
            return None;
        }
        bimcraft_i18n::translated(CATALOGS, self.code(), "", english)
    }

    /// Which of `options` (English words) `name` is, in English or in this language.
    pub fn find(self, name: &str, options: &[&'static str]) -> Option<&'static str> {
        let name = name.trim();
        options.iter().copied().find(|o| o.eq_ignore_ascii_case(name) || self.word(o).to_lowercase() == name.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_come_in_the_projects_language() {
        let pt = Language::Portuguese;
        assert_eq!([pt.word("Level"), pt.word("UP"), pt.word("SCALE AS INDICATED")], ["Nível", "SOBE", "ESCALA CONFORME INDICADO"]);
        assert_eq!(Language::English.word("Level"), "Level");
        assert_eq!(pt.word("Id"), "Id", "a word with no translation stays");
        assert_eq!(pt.find("tabela de ambientes", &["Room Schedule", "Door Schedule"]), Some("Room Schedule"));
        assert_eq!(pt.find("Door Schedule", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        let es = Language::Spanish;
        assert_eq!([es.word("Level"), es.word("UP"), es.word("Room Schedule")], ["Nivel", "SUBE", "Tabla de habitaciones"]);
        assert_eq!(es.find("TABLA DE PUERTAS", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        // Codes as commands write them, and locales as systems give them.
        assert_eq!(
            ["pt-BR", "pt_BR.UTF-8", "es", "es-419", "Español", "en-US", "en", "C", ""].map(Language::from_code),
            [Some(pt), Some(pt), Some(es), Some(es), Some(es), Some(Language::English), Some(Language::English), None, None]
        );
    }

    #[test]
    fn german_words_and_locales_are_project_language() {
        let de = Language::German;
        assert_eq!([de.word("Level"), de.word("UP"), de.word("Room Schedule")], ["Geschoss", "AUF", "Raumtabelle"]);
        assert_eq!(de.find("RAUMTABELLE", &["Room Schedule", "Door Schedule"]), Some("Room Schedule"));
        assert_eq!(["de", "de-DE", "de_DE.UTF-8", "de-AT", "de-CH", "german", "Deutsch"].map(Language::from_code), [Some(de); 7]);
        assert_eq!((de.code(), de.label()), ("de", "Deutsch"));
    }

    #[test]
    fn swedish_words_and_locales_are_project_language() {
        let sv = Language::Swedish;
        assert_eq!([sv.word("Level"), sv.word("UP"), sv.word("Room Schedule")], ["Nivå", "UPP", "Rumsförteckning"]);
        assert_eq!(sv.find("DÖRRFÖRTECKNING", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        assert_eq!(
            ["sv", "sv-SE", "sv_SE.UTF-8", "swedish", "Svenska"].map(Language::from_code),
            [Some(sv), Some(sv), Some(sv), Some(sv), Some(sv)]
        );
    }

    #[test]
    fn french_words_and_locales_are_project_language() {
        let fr = Language::French;
        assert_eq!([fr.word("Level"), fr.word("UP"), fr.word("Room Schedule")], ["Niveau", "MONTE", "Nomenclature des pièces"]);
        assert_eq!(fr.find("NOMENCLATURE DES PORTES", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        assert_eq!(["fr", "fr-FR", "fr_CA.UTF-8", "french", "Français"].map(Language::from_code), [Some(fr); 5]);
    }

    #[test]
    fn czech_words_and_locales_are_project_language() {
        let cs = Language::Czech;
        assert_eq!([cs.word("Level"), cs.word("UP"), cs.word("Room Schedule")], ["Podlaží", "NAHORU", "Výkaz místností"]);
        assert_eq!(cs.find("VÝKAZ DVEŘÍ", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        assert_eq!(["cs", "cs-CZ", "cs_CZ.UTF-8", "czech", "Čeština"].map(Language::from_code), [Some(cs); 5]);
    }

    #[test]
    fn dutch_words_and_locales_are_project_language() {
        let nl = Language::Dutch;
        assert_eq!((nl.code(), nl.label()), ("nl", "Nederlands"));
        assert_eq!([nl.word("Level"), nl.word("UP"), nl.word("Room Schedule")], ["Niveau", "OMHOOG", "Ruimtelijst"]);
        assert_eq!(nl.find("DEURENLIJST", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        assert_eq!(["nl", "nl-BE", "nl-NL", "nl_BE.UTF-8", "dutch", "Nederlands", "NL"].map(Language::from_code), [Some(nl); 7]);
        assert!(Language::ALL.contains(&nl));
    }

    #[test]
    fn italian_words_and_locales_are_project_language() {
        let it = Language::Italian;
        assert_eq!((it.code(), it.label()), ("it", "Italiano"));
        assert_eq!([it.word("Level"), it.word("UP"), it.word("Room Schedule")], ["Piano", "SU", "Abaco zone"]);
        assert_eq!(it.find("ABACO PORTE", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        assert_eq!(["it", "it-IT", "it_CH.UTF-8", "italian", "Italiano", "IT"].map(Language::from_code), [Some(it); 6]);
        assert!(Language::ALL.contains(&it));
    }
}
