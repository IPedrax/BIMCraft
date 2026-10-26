//! Translation lookup without a runtime parser, filesystem access or a system gettext dependency.

#[cfg(any(test, feature = "catalog"))]
pub mod catalog;

/// Sorted (context, English message, translation) entries compiled from one PO file.
pub type Messages = &'static [(&'static str, &'static str, &'static str)];
/// Language codes and their message tables, sorted by code.
pub type Catalogs = &'static [(&'static str, Messages)];

/// Mark an English message selected indirectly (a table entry, an enum label, a tool prompt).
/// The catalog tool extracts it; at runtime it is the original string literal.
#[macro_export]
macro_rules! msg {
    ($english:literal) => {
        $english
    };
    ($context:literal, $english:literal) => {
        $english
    };
}

/// Mark a core- or app-owned label that only the interface translates.
#[macro_export]
macro_rules! ui_msg {
    ($english:literal) => {
        $english
    };
    ($context:literal, $english:literal) => {
        $english
    };
}

/// A translation in exactly this context, or none when the catalog or message is missing.
pub fn translated(catalogs: Catalogs, language: &str, context: &str, english: &str) -> Option<&'static str> {
    if language == "en" {
        return None;
    }
    let index = catalogs.binary_search_by_key(&language, |&(code, _)| code).ok()?;
    let messages = catalogs[index].1;
    let index = messages.binary_search_by_key(&(context, english), |&(context, id, _)| (context, id)).ok()?;
    Some(messages[index].2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_keeps_contexts_separate_and_missing_messages_fall_back() {
        static CATALOGS: Catalogs =
            &[("en", &[("", "Save", "Do not replace English")]), ("pt", &[("", "Save", "Salvar"), ("verb", "Open", "Abrir")])];
        assert_eq!(translated(CATALOGS, "pt", "", "Save"), Some("Salvar"));
        assert_eq!(translated(CATALOGS, "pt", "verb", "Open"), Some("Abrir"));
        for (language, context, english) in [("en", "", "Save"), ("fr", "", "Save"), ("pt", "", "Open"), ("pt", "verb", "Missing")] {
            assert_eq!(translated(CATALOGS, language, context, english).unwrap_or(english), english);
        }
        assert_eq!(msg!("verb", "Open"), "Open");
    }
}
