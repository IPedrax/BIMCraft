//! The interface's language: English, as the code writes it, Brazilian Portuguese, Spanish or Simplified Chinese. What
//! the window shows goes through [`t`], English being its own key; the widget helpers translate
//! their labels and tips, so most call sites pass English and read it back translated. What a
//! project holds (names, what its drawings say) follows the project's language instead, and the
//! core's messages, which agents read too, stay English.
//!
//! Chinese is the window's alone ([`Lang`]): a drawing's lettering is drawn in code and has no hanzi, so a project is never
//! in it. Chinese interface translations, including schedule headings, live in `translations/ui/zh.po`, one entry for each of
//! the interface's messages: an entry it lacks shows in English. The window draws hanzi with a system font (`font.rs`).

use bimcraft_i18n::msg;

mod font;
include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

// English keys retained for compatibility with text selected indirectly or stored by older versions.
const _: &[&str] = &[
    msg!("Backups kept"),
    msg!("start"),
    msg!("end"),
    msg!("Click which way it climbs: its risers take it up to the level above. Esc cancels."),
    msg!("Open a schedule"),
    msg!("Stair: click where it starts, then which way it climbs"),
    msg!("Dimension"),
    msg!("Dimensions"),
    msg!("Tag"),
    msg!("Windows"),
];

use std::cell::Cell;

use bimcraft_core::language::Language;
use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Context, FontData, Id};

/// The interface's language: one of a project's languages, or Simplified Chinese.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    English,
    Portuguese,
    Spanish,
    Czech,
    French,
    Chinese,
}

impl Lang {
    pub const ALL: [Lang; 6] = [Lang::English, Lang::Portuguese, Lang::Spanish, Lang::Czech, Lang::French, Lang::Chinese];

    /// The project language that goes with it, which new projects are named in: Chinese has none, so English.
    pub fn project(self) -> Language {
        match self {
            Lang::English | Lang::Chinese => Language::English,
            Lang::Portuguese => Language::Portuguese,
            Lang::Spanish => Language::Spanish,
            Lang::Czech => Language::Czech,
            Lang::French => Language::French,
        }
    }

    /// Its code in the settings file: en, pt, es, zh.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Chinese => "zh",
            other => other.project().code(),
        }
    }

    /// Its name, in itself: Chinese in hanzi only once the window is drawn in it, which is when it can draw them.
    pub fn label(self) -> &'static str {
        match self {
            Lang::Chinese if get() == Lang::Chinese => "简体中文",
            Lang::Chinese => "Chinese (Simplified)",
            other => other.project().label(),
        }
    }

    /// A language by its code or name, or from a locale as systems give one: pt-BR, es_419.UTF-8, zh-CN. Only Simplified
    /// Chinese is here: zh-TW, zh-HK and the other Traditional locales are none.
    pub fn from_code(code: &str) -> Option<Lang> {
        let lower = code.trim().to_lowercase();
        let parts: Vec<&str> = lower.split(['-', '_', '.', '@']).collect();
        if matches!(parts[0], "zh" | "chinese") || lower == "简体中文" {
            let traditional = parts.iter().any(|p| matches!(*p, "tw" | "hk" | "mo" | "hant"));
            return (!traditional).then_some(Lang::Chinese);
        }
        Language::from_code(code).map(Lang::from)
    }
}

impl From<Language> for Lang {
    fn from(language: Language) -> Lang {
        match language {
            Language::English | Language::Swedish => Lang::English,
            Language::Portuguese => Lang::Portuguese,
            Language::Spanish => Lang::Spanish,
            Language::Czech => Lang::Czech,
            Language::French => Lang::French,
        }
    }
}

thread_local! {
    /// The language this frame is drawn in: each window draws on its own thread.
    static LANG: Cell<Lang> = const { Cell::new(Lang::English) };
}

pub fn set(language: Lang) {
    LANG.with(|l| l.set(language));
}

pub fn get() -> Lang {
    LANG.with(Cell::get)
}

/// The language the system's settings ask for (on Linux, `LC_ALL`, `LC_MESSAGES` or `LANG`), when
/// it's one there is; else English.
pub fn from_env() -> Lang {
    sys_locale::get_locale().and_then(|l| Lang::from_code(&l)).unwrap_or_default()
}

/// Said when the window cannot be put in Chinese.
pub const NO_CHINESE_FONT: &str =
    msg!("No font with Chinese characters was found on this system: install one (such as Noto Sans CJK) and choose Chinese again.");

/// Whether `ctx` has a font to draw Chinese with: [`use_chinese_font`] has put one there.
fn has_chinese_font(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(Id::new("chinese-font"))).unwrap_or(false)
}

/// Looks for a font with Chinese glyphs: its bytes and the face to use. The app holds one, so that a test can say there is none.
pub type FontFinder = fn() -> Option<(Vec<u8>, u32)>;

/// The font of the system's [`use_chinese_font`] looks for.
pub fn chinese_font() -> Option<(Vec<u8>, u32)> {
    font::find()
}

/// Puts a font that `find` finds, with Chinese glyphs, behind the window's own, once; false where there is none. egui's fonts have
/// no hanzi, so a window in Chinese without one would be boxes.
pub fn use_chinese_font(ctx: &Context, find: FontFinder) -> bool {
    if has_chinese_font(ctx) {
        return true;
    }
    let Some((bytes, index)) = find() else { return false };
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    let families = crate::style::families().map(|family| InsertFontFamily { family, priority: FontPriority::Lowest });
    ctx.add_font(FontInsert::new("chinese", data, families.into()));
    ctx.data_mut(|d| d.insert_temp(Id::new("chinese-font"), true));
    true
}

/// Tests have no say in which fonts a machine has: a window is told it has one.
#[cfg(test)]
pub(crate) fn assume_chinese_font(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new("chinese-font"), true));
}

/// `english` in the interface's language.
pub fn t(english: &str) -> &str {
    translated_in(get(), english).unwrap_or(english)
}

/// `english` in `lang`, when a non-English interface catalog has a finished translation.
pub fn translated_in(lang: Lang, english: &str) -> Option<&'static str> {
    match lang {
        Lang::Chinese => bimcraft_i18n::translated(CATALOGS, lang.code(), "", english),
        other => translated(other.project(), english),
    }
}

/// An interface translation for one of the project languages, when the catalog has it.
pub fn translated(language: Language, english: &str) -> Option<&'static str> {
    if language == Language::English {
        return None;
    }
    bimcraft_i18n::translated(CATALOGS, language.code(), "", english)
}

/// A message whose meaning is distinguished by a PO `msgctxt`.
pub fn tc<'a>(context: &str, english: &'a str) -> &'a str {
    bimcraft_i18n::translated(CATALOGS, get().code(), context, english).unwrap_or(english)
}

/// `template` translated, its `{}`s filled with `args` in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    args.iter().fold(t(template).to_string(), |s, a| s.replacen("{}", a, 1))
}

/// An element category's name, of one (`Wall`) or of several (`Walls`).
pub fn category(category: &str, several: bool) -> String {
    let (one, many) = match category {
        "wall" => (msg!("category", "Wall"), msg!("category-plural", "Walls")),
        "door" => (msg!("category", "Door"), msg!("category-plural", "Doors")),
        "window" => (msg!("category", "Window"), msg!("category-plural", "Windows")),
        "floor" => (msg!("category", "Floor"), msg!("category-plural", "Floors")),
        "room" => (msg!("category", "Room"), msg!("category-plural", "Rooms")),
        "level" => (msg!("category", "Level"), msg!("category-plural", "Levels")),
        "grid" => (msg!("category", "Grid"), msg!("category-plural", "Grids")),
        "dimension" => (msg!("category", "Dimension"), msg!("category-plural", "Dimensions")),
        "tag" => (msg!("category", "Tag"), msg!("category-plural", "Tags")),
        "section" => (msg!("category", "Section"), msg!("category-plural", "Sections")),
        "elevation" => (msg!("category", "Elevation"), msg!("category-plural", "Elevations")),
        "sheet" => (msg!("category", "Sheet"), msg!("category-plural", "Sheets")),
        "viewport" => (msg!("category", "Viewport"), msg!("category-plural", "Viewports")),
        "column" => (msg!("category", "Column"), msg!("category-plural", "Columns")),
        "component" => (msg!("category", "Component"), msg!("category-plural", "Components")),
        "stair" => (msg!("category", "Stair"), msg!("category-plural", "Stairs")),
        "roof" => (msg!("category", "Roof"), msg!("category-plural", "Roofs")),
        "ceiling" => (msg!("category", "Ceiling"), msg!("category-plural", "Ceilings")),
        "shaft" => (msg!("category", "Shaft Opening"), msg!("category-plural", "Shaft Openings")),
        "mass" => (msg!("category", "Mass"), msg!("category-plural", "Masses")),
        "region" => (msg!("category", "Filled Region"), msg!("category-plural", "Filled Regions")),
        "massfloor" => (msg!("category", "Mass Floor"), msg!("category-plural", "Mass Floors")),
        "ramp" => (msg!("category", "Ramp"), msg!("category-plural", "Ramps")),
        "railing" => (msg!("category", "Railing"), msg!("category-plural", "Railings")),
        "roomseparator" => (msg!("category", "Room Separator"), msg!("category-plural", "Room Separators")),
        "beam" => (msg!("category", "Beam"), msg!("category-plural", "Beams")),
        "structuralcolumn" => (msg!("category", "Structural Column"), msg!("category-plural", "Structural Columns")),
        "footing" => (msg!("category", "Footing"), msg!("category-plural", "Footings")),
        "stripfooting" => (msg!("category", "Strip Footing"), msg!("category-plural", "Strip Footings")),
        "pipe" => (msg!("category", "Pipe"), msg!("category-plural", "Pipes")),
        "conduit" => (msg!("category", "Conduit"), msg!("category-plural", "Conduits")),
        "cabletray" => (msg!("category", "Cable Tray"), msg!("category-plural", "Cable Trays")),
        "wire" => (msg!("category", "Wire"), msg!("category-plural", "Wires")),
        "cable" => (msg!("category", "Cable"), msg!("category-plural", "Cables")),
        "downconductor" => (msg!("category", "Down Conductor"), msg!("category-plural", "Down Conductors")),
        "groundconductor" => (msg!("category", "Earthing Conductor"), msg!("category-plural", "Earthing Conductors")),
        "text" => (msg!("category", "Text Note"), msg!("category-plural", "Text Notes")),
        "line" => (msg!("category", "Detail Line"), msg!("category-plural", "Lines")),
        "spot" => (msg!("category", "Spot Elevation"), msg!("category-plural", "Spot Elevations")),
        "schedule" => (msg!("category", "Schedule"), msg!("category-plural", "Schedules")),
        "view3d" => (msg!("category", "3D View"), msg!("category-plural", "3D Views")),
        "link" => (msg!("category", "Linked Model"), msg!("category-plural", "Linked Models")),
        "pointcloud" => (msg!("category", "Point Cloud"), msg!("category-plural", "Point Clouds")),
        "cadimport" => (msg!("category", "CAD Import"), msg!("category-plural", "CAD Imports")),
        "circuit" => (msg!("category", "Circuit"), msg!("category-plural", "Circuits")),
        other => return other.to_string(),
    };
    tc(if several { "category-plural" } else { "category" }, if several { many } else { one }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_its_own_key_and_project_terms_have_interface_translations() {
        set(Lang::English);
        assert_eq!((t("Wall Schedule"), t("Wall")), ("Wall Schedule", "Wall"));
        set(Lang::Portuguese);
        assert_eq!((t("Wall Schedule"), t("Arquitetura"), t("Architecture")), ("Tabela de paredes", "Arquitetura", "Arquitetura"));
        assert_eq!(
            tf("roof: the slope `{}` and the overhang `{}` must be numbers", &["x", "y"]),
            "telhado: a inclinação `x` e o beiral `y` precisam ser números"
        );
        assert_eq!((category("wall", true), category("spot", false)), ("Paredes".to_string(), "Elevação de ponto".to_string()));
        set(Lang::Spanish);
        assert_eq!((t("Wall Schedule"), t("Save changes to {}?")), ("Tabla de muros", "¿Guardar los cambios en {}?"));
        assert_eq!((category("room", true), category("viewport", false)), ("Habitaciones".to_string(), "Vista en plano".to_string()));
        set(Lang::English);
    }

    #[test]
    fn chinese_has_its_own_words_and_what_it_lacks_is_english() {
        set(Lang::Chinese);
        assert_eq!(
            (t("Architecture"), t("File"), t("Wall Schedule"), t("Copy")),
            ("建筑", "文件", "墙明细表", "复制"),
            "the window's words first, then the core's"
        );
        assert_eq!(tf("Save changes to {}?", &["house"]), "是否将更改保存到 house？");
        assert_eq!((category("wall", true), category("wall", false), category("spot", false)), ("墙".into(), "墙".into(), "高程点".into()));
        assert_eq!(category("airterminal", true), "airterminal", "a category without a name is its key, as in the other languages");
        assert_eq!((Lang::Chinese.label(), Lang::Portuguese.label()), ("简体中文", "Português (Brasil)"));
        set(Lang::English);
        assert_eq!(
            (t("Architecture"), Lang::Chinese.label()),
            ("Architecture", "Chinese (Simplified)"),
            "hanzi only once the window can draw them"
        );
    }

    #[test]
    fn missing_catalog_entries_fall_back_and_categories_use_their_own_contexts() {
        for language in Lang::ALL {
            set(language);
            assert_eq!(t("A new untranslated label"), "A new untranslated label");
            assert_eq!(tf("Untranslated {}", &["value"]), "Untranslated value");
            assert_eq!(tc("missing-context", "Copy"), "Copy");
        }
        set(Lang::Chinese);
        assert_eq!((t("Copy"), tc("missing-context", "Copy")), ("复制", "Copy"));
        assert_eq!(category("link", true), "Linked Models");
        set(Lang::English);
    }

    #[test]
    fn the_codes_and_locales_that_mean_simplified_chinese_and_the_ones_that_do_not() {
        for code in ["zh", "zh-CN", "zh_CN.UTF-8", "zh-Hans", "zh_Hans_CN", "zh-SG", "ZH", "chinese", "简体中文"] {
            assert_eq!(Lang::from_code(code), Some(Lang::Chinese), "{code}");
        }
        for code in ["zh-TW", "zh_HK.UTF-8", "zh-Hant", "zh-Hant-TW", "zh_MO"] {
            assert_eq!(Lang::from_code(code), None, "{code}: Traditional is not what is here");
        }
        assert_eq!(
            ["pt-BR", "es_419.UTF-8", "en-US", "de", ""].map(Lang::from_code),
            [Some(Lang::Portuguese), Some(Lang::Spanish), Some(Lang::English), None, None]
        );
        assert_eq!((Lang::Chinese.code(), Lang::Chinese.project()), ("zh", Language::English), "a project is never in Chinese");
        assert_eq!(Lang::from(Language::Swedish), Lang::English, "Swedish is a project language until its interface catalog is added");
        assert_eq!(Lang::ALL.map(|l| Lang::from_code(l.code())), Lang::ALL.map(Some));
    }
}
