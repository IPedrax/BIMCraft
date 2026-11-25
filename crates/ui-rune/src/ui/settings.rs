//! The Settings dialog (Ctrl+, and the Settings button on the top bar): the interface's language, the project's units, the theme and the
//! profile, each a select, and a button for Manage > Project Information. The state and what a choice does are `app/settings.rs`; the
//! file is `settings.rs`. A choice is kept in the user's `settings.json` as it is made. What stopped one is said under the rows.

use bimcraft_core::units::Units;
use rune::{El, Tx, col, row, text};
use rune_kit::{Button, Dialog, Kit, Select, SelectAppearance};

use crate::app::App;
use crate::app::settings::Choice;
use crate::lang::{self, t};
use crate::profile::Profile;
use crate::settings::{LANGUAGES, Theme};

/// The dialog's width, points.
const WIDTH: f32 = 520.0;

/// One select: `options` are (what is shown, the value), `current` the value chosen, and `apply` what choosing a value does.
#[allow(clippy::too_many_arguments)]
fn select(
    k: &Kit,
    layer: &mut Vec<El>,
    key: &'static str,
    label: &str,
    options: Vec<(String, String)>,
    current: &str,
    which: Choice,
    open: bool,
    apply: fn(&mut App, &str),
) -> El {
    let shown: Vec<String> = options.iter().map(|(shown, _)| shown.clone()).collect();
    let current_shown: String = options.iter().find(|(_, value)| value == current).map(|(shown, _)| shown.clone()).unwrap_or_default();
    let picked = Select::new(key, &shown)
        .label(label.to_string())
        .current(Some(current_shown.as_str()))
        .appearance(SelectAppearance::Field)
        .width(260.0)
        .open(open)
        .on_toggle(move |s: &mut App| s.toggle_choice(which))
        .on_close(|s: &mut App| s.close_choice())
        .on_choose(move |s: &mut App, chosen: &str, _: &mut Tx| {
            if let Some((_, value)) = options.iter().find(|(shown, _)| shown == chosen) {
                apply(s, value);
            }
        })
        .build(k, layer);
    row()
        .key(format!("{key}-row"))
        .w_full()
        .gap(12.0)
        .center_y()
        .child(text(label.to_string()).font(k.ty.ui).color(k.c.text_base).w(140.0))
        .child(picked.grow(1.0))
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(shown, value)| (t(shown).to_string(), (*value).to_string())).collect()
}

/// The dialog's scrim and panel, when it is open.
pub fn dialog(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.dialogs.settings else { return };
    let kept = &s.prefs.kept;
    let language = kept.language.as_deref().unwrap_or_else(lang::from_system);
    let languages = LANGUAGES.iter().map(|(code, name)| ((*name).to_string(), (*code).to_string())).collect();
    let units = Units::ALL.iter().map(|u| (t(u.label()).to_string(), u.name().to_string())).collect();
    let profile = match s.profile {
        Profile::Bimcraft => "bimcraft",
        Profile::Rev => "rev",
    };
    let rows = col()
        .key("settings-rows")
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(select(
            k,
            layer,
            "language",
            t("Language"),
            languages,
            language,
            Choice::Language,
            d.open == Some(Choice::Language),
            |s, v| {
                s.set_language(v);
            },
        ))
        .child(select(
            k,
            layer,
            "units",
            t("Units"),
            units,
            s.session.model.units.name(),
            Choice::Units,
            d.open == Some(Choice::Units),
            |s, v| {
                s.set_units(v);
            },
        ))
        .child(select(
            k,
            layer,
            "theme",
            t("Theme"),
            pairs(&[("Dark", "dark"), ("Light", "light"), ("System", "system")]),
            kept.theme.code(),
            Choice::Theme,
            d.open == Some(Choice::Theme),
            |s, v| {
                if let Some(theme) = Theme::from_code(v) {
                    s.set_theme(theme);
                }
            },
        ))
        .child(select(
            k,
            layer,
            "profile",
            t("Profile"),
            pairs(&[("BIMCraft", "bimcraft"), ("Rev", "rev")]),
            profile,
            Choice::Profile,
            d.open == Some(Choice::Profile),
            |s, v| s.set_profile(if v == "rev" { Profile::Rev } else { Profile::Bimcraft }),
        ));
    let mut body = col().w_full().shrink().min_h(0.0).child(rows);
    if let Some(why) = &d.error {
        body = body.child(
            col().w_full().px(16.0).child(text(why.clone()).key("settings-error").font(k.ty.ui).color(k.c.danger.fg).wrap().w_full()),
        );
    }
    let footer = row()
        .key("settings-footer")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_between()
        .child(
            Button::new(t("Project Information").to_string())
                .on_press(|s: &mut App| {
                    s.open_command("project_info");
                })
                .build(k)
                .key("settings-project-info"),
        )
        .child(Button::new(t("Close").to_string()).on_press(|s: &mut App| s.close_dialogs()).build(k).key("settings-close"));
    Dialog::new("settings", t("Settings").to_string())
        .width(WIDTH)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_dialogs())
        .child(body)
        .child(footer)
        .build(k, layer);
}
