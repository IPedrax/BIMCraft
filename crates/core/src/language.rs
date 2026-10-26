//! A project's language: the words its drawings print (a stair's UP, a title block's SCALE, a
//! schedule's headings) and the names commands give what they make (levels, views, sheets, rooms).
//! English, Brazilian Portuguese or Spanish. Names already given stay as they are; the core's
//! messages, which agents read, stay English.

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
}

impl Language {
    pub const ALL: [Language; 3] = [Language::English, Language::Portuguese, Language::Spanish];

    /// Its code in commands and files: en, pt, es.
    pub fn code(self) -> &'static str {
        self.of(["en", "pt", "es"])
    }

    /// Its name, in itself.
    pub fn label(self) -> &'static str {
        self.of(["English", "Português (Brasil)", "Español"])
    }

    /// Of one thing in each language, English first, this language's.
    pub fn of<T>(self, [english, portuguese, spanish]: [T; 3]) -> T {
        match self {
            Language::English => english,
            Language::Portuguese => portuguese,
            Language::Spanish => spanish,
        }
    }

    /// A language by its code or its name, or from a locale as systems give one: pt-BR, es_419.UTF-8.
    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.trim().to_lowercase();
        Some(match code.split(['-', '_', '.']).next().unwrap_or_default() {
            "en" | "english" => Language::English,
            "pt" | "portuguese" | "português" => Language::Portuguese,
            "es" | "spanish" | "español" => Language::Spanish,
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
            ["pt-BR", "pt_BR.UTF-8", "es", "es-419", "Español", "en-US", "en", "de", "C", ""].map(Language::from_code),
            [Some(pt), Some(pt), Some(es), Some(es), Some(es), Some(Language::English), Some(Language::English), None, None, None]
        );
    }
}
