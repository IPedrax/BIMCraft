# Translation catalogs

English messages stay in Rust as lookup keys and fallback text. Translations belong in UTF-8 gettext PO files:

| Domain | Template | Language catalogs | Used by |
|---|---|---|---|
| Interface | `translations/ui/bimcraft-ui.pot` | `pt.po`, `es.po`, `zh.po` | `lang::t`, `lang::tf`, `lang::tc` |
| Project | `translations/project/bimcraft-project.pot` | `pt.po`, `es.po` | `Language::word`, `Language::translated`, `Language::find` |

Interface and project language remain independent. Settings and project files retain `en`, `pt`, `es` and (interface only)
`zh`; no file migration is needed. Native language names and locale aliases remain language metadata in Rust. Chinese
still needs a system font, and the project's stroke lettering still cannot draw it. Diagnostics returned by the core,
CLI and MCP remain English. Existing project and user-supplied names are never translated in place.

## Translating

Edit the language's `.po` file in a text editor or a gettext-compatible translation editor. Change `msgstr`, keeping
`msgid` and `msgctxt` intact. Keep every `{}` placeholder in the same argument order: `tf` substitutes them sequentially.
Keep deliberate spaces, line breaks and punctuation. Entries may be left unfinished.

```po
#. Prompt shown before closing a changed project.
msgid "Save changes to {}?"
msgstr "¿Guardar los cambios en {}?"
```

An unknown locale, missing message, empty or whitespace-only translation, or entry marked `#, fuzzy` uses English.
Obsolete entries (`#~`) are retained for translators and excluded from the application. Fuzzy entries can remain in a
catalog while awaiting review; remove the flag when the translation is ready. Coverage is reported, not enforced as 100%.
The interface catalog includes translations of shared project terms so that an unfinished interface message falls back
to English directly, without borrowing a translation from another domain.

## Adding or changing English messages

Use `t("English")` for interface labels, `tf("Value {}", &[value])` for sequential substitution, and
`Language::word("English")` for project text. An ambiguous word can use `tc("context", "English")`, which looks up exactly
that `msgctxt`; a translation in another context never satisfies it. Category labels use `category` and `category-plural`
contexts for the existing one/several selection.

The extractor recognizes literal arguments to these functions and the ribbon's button helpers. A label selected through
an enum, table, conditional or other indirect path must be marked where its English literal is defined:

```rust
use bimcraft_i18n::{msg, ui_msg};

let prompt = msg!("Click the wall.");
let category = msg!("category", "Wall");
// In the core or desktop launcher, a label translated only by the interface:
let heading = ui_msg!("Properties");
```

`msg!` and `ui_msg!` return the English literal unchanged. Core `msg!` messages enter the project template and are also
available to interface translators; `ui_msg!` messages enter only the interface template. Markers in the UI enter only
the interface template. Do not mark user data, JSON field keys or arbitrary diagnostics. A `// TRANSLATORS:` comment
immediately before a literal becomes an extracted comment. Templates include source references. Test code is excluded.
The small unnamed English list in `lang.rs` retains historical keys still accepted by translation lookup.

From the repository root:

```sh
cargo i18n extract  # Refresh the English templates only.
cargo i18n update   # Refresh templates and merge all PO files.
cargo i18n check    # Check extraction, PO syntax, duplicate keys and placeholders.
cargo i18n stats    # Report finished translations in each catalog.
```

Run `update` after formatting Rust sources, since references contain line numbers. Merging matches exact English and
context, preserves translator comments, flags and metadata, refreshes developer comments/references, and keeps removed
translations as obsolete entries. Reintroduced messages can recover their obsolete translations. Changed English gets
an empty translation; the tool does not guess one. `check` is part of CI and accepts partial catalogs.

## Building and supported syntax

Core and UI build scripts compile their own PO files into sorted static tables through `crates/i18n` (`bimcraft-i18n`).
Cargo tracks the catalog directories and recompiles after edits. Lookup uses binary search, with no runtime parsing,
filesystem reads, process-wide locale changes, network access, gettext installation or native `libintl` requirement.
Translation changes take effect after rebuilding; installed applications do not load external catalog overrides.

The compiler supports UTF-8, contexts, translator/developer comments, references, fuzzy flags, obsolete entries, multiline
quoted strings and the standard escaped quotes, backslashes and control characters. Duplicate active keys, malformed
syntax and incorrect `{}` counts in finished translations fail the build with the file and line or message named.
The current APIs do not select grammatical plural forms: `msgid_plural`/`msgstr[n]` entries are rejected explicitly.
Category one/several labels remain separate contextual messages. A count-based plural API is a future extension.

Adding a language starts with a copy of the relevant `.pot` template saved as `<code>.po`, adding its `Language` header.
Build scripts discover PO files automatically. Exposing a new selectable language still needs its locale metadata in
`Lang` or `Language`, and project-language additions need compatible serialization and font support. Updating translations
for an existing language requires no Rust edits.
