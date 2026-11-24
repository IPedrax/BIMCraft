//! The status bar's line: what the pointer is over and where, in words the interface's language says. A pure function of what the
//! app knows, so the bar is tested without a window.

use crate::lang::{t, tf};

/// What the bar says something of.
pub struct Facts<'a> {
    /// The plan being drawn, by name.
    pub level: &'a str,
    /// Where the pointer is on the plan, model millimetres.
    pub cursor: Option<(f64, f64)>,
    /// The element under the pointer: its category and id.
    pub hovered: Option<(&'a str, u64)>,
    pub selected: usize,
    /// The tool in hand, by its (English) label.
    pub tool: Option<&'a str>,
    /// A command's refusal, as the core says it (English: agents read it too).
    pub message: Option<&'a str>,
}

/// The line: a refusal first (it is the one thing a person must read), else the level, the pointer and what is under it.
pub fn line(f: &Facts) -> String {
    if let Some(m) = f.message {
        return m.to_string();
    }
    let mut parts = vec![f.level.to_string()];
    if let Some(tool) = f.tool {
        parts.push(tf("Tool: {}", &[t(tool)]));
    }
    if let Some((x, y)) = f.cursor {
        parts.push(tf("x {} y {} mm", &[&format!("{x:.0}"), &format!("{y:.0}")]));
    }
    if let Some((category, id)) = f.hovered {
        parts.push(format!("{} {id}", crate::lang::tc("category", &capital(category))));
    }
    if f.selected > 0 {
        parts.push(tf("{} selected", &[&f.selected.to_string()]));
    } else {
        parts.push(t("Nothing selected").to_string());
    }
    parts.join("   |   ")
}

/// `wall` as `Wall`: the key a category's name has in the catalogs.
pub fn capital(category: &str) -> String {
    let mut chars = category.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::set;

    fn facts<'a>() -> Facts<'a> {
        Facts { level: "Level 1", cursor: Some((1234.4, 56.6)), hovered: Some(("wall", 100234)), selected: 2, tool: None, message: None }
    }

    #[test]
    fn the_line_says_level_pointer_what_is_under_it_and_the_selection() {
        set("en");
        assert_eq!(line(&facts()), "Level 1   |   x 1234 y 57 mm   |   Wall 100234   |   2 selected");
        let quiet = Facts { cursor: None, hovered: None, selected: 0, ..facts() };
        assert_eq!(line(&quiet), "Level 1   |   Nothing selected");
        assert_eq!(line(&Facts { tool: Some("Wall"), ..quiet }), "Level 1   |   Tool: Wall   |   Nothing selected");
    }

    #[test]
    fn a_refusal_is_all_the_bar_says() {
        assert_eq!(line(&Facts { message: Some("delete: no such element"), ..facts() }), "delete: no such element");
    }

    #[test]
    fn a_category_is_named_as_the_catalogs_key_it() {
        assert_eq!(capital("wall"), "Wall");
        assert_eq!(capital(""), "");
        set("pt");
        assert!(line(&Facts { cursor: None, ..facts() }).contains("Parede 100234"), "the catalog's `category` Wall");
        set("en");
    }
}
