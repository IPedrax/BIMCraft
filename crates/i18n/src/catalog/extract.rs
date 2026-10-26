//! Extract syntax, not text matches: English literals passed to translation functions and widget
//! helpers, and explicit `msg!` markers for labels chosen indirectly. Test modules are excluded.

use super::{Document, Entries, Entry};
use proc_macro2::{Span, TokenStream, TokenTree};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Expr, Lit, LitStr, Token};

struct Extractor<'a> {
    entries: &'a mut Entries,
    path: String,
    lines: Vec<&'a str>,
    ui: bool,
    markers_only: bool,
}

impl Extractor<'_> {
    fn add(&mut self, context: &str, message: &LitStr) {
        let id = message.value();
        if id.is_empty() {
            return;
        }
        let entry =
            self.entries.entry((context.into(), id.clone())).or_insert_with(|| Entry { context: context.into(), id, ..Entry::default() });
        let line = message.span().start().line;
        let reference = format!("#: {}:{line}", self.path);
        if !entry.comments.contains(&reference) {
            entry.comments.push(reference);
        }
        if let Some(comment) = line.checked_sub(2).and_then(|i| self.lines.get(i)).and_then(|l| l.trim().strip_prefix("// TRANSLATORS:")) {
            let comment = format!("#. {}", comment.trim());
            if !entry.comments.contains(&comment) {
                entry.comments.push(comment);
            }
        }
    }

    fn arguments(&mut self, name: &str, args: &[&Expr]) {
        if self.markers_only {
            return;
        }
        let indices: &[usize] = match name {
            "t" | "tf" | "word" => &[0],
            "big_button" | "small_button" => &[2, 4],
            "icon_button" => &[2],
            "tab_button" | "tab_button_with" => &[1],
            _ => &[],
        };
        for &index in indices {
            if let Some(Expr::Lit(literal)) = args.get(index).copied()
                && let Lit::Str(message) = &literal.lit
            {
                self.add("", message);
            }
        }
        if name == "tc"
            && let [Expr::Lit(context), Expr::Lit(message), ..] = args
            && let (Lit::Str(context), Lit::Str(message)) = (&context.lit, &message.lit)
        {
            self.add(&context.value(), message);
        }
    }

    fn tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            if let TokenTree::Ident(name) = token {
                if (name == "msg" || (self.ui && name == "ui_msg"))
                    && matches!(tokens.get(index + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
                    && let Some(TokenTree::Group(group)) = tokens.get(index + 2)
                    && let Ok(args) = Punctuated::<LitStr, Token![,]>::parse_terminated.parse2(group.stream())
                {
                    let args: Vec<_> = args.iter().collect();
                    match args.as_slice() {
                        [message] => self.add("", message),
                        [context, message] => self.add(&context.value(), message),
                        _ => {}
                    }
                } else if let Some(TokenTree::Group(group)) = tokens.get(index + 1)
                    && let Ok(args) = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(group.stream())
                {
                    self.arguments(&name.to_string(), &args.iter().collect::<Vec<_>>());
                }
            }
            if let TokenTree::Group(group) = token {
                self.tokens(group.stream());
            }
        }
    }
}

fn test_attributes(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")))
}

impl<'ast> Visit<'ast> for Extractor<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !test_attributes(&module.attrs) {
            visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if !test_attributes(&function.attrs) {
            visit::visit_item_fn(self, function);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*call.func
            && let Some(name) = path.path.segments.last()
        {
            self.arguments(&name.ident.to_string(), &call.args.iter().collect::<Vec<_>>());
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        // Only Language::word is a translating method; egui's label/button methods are not.
        if call.method == "word" {
            self.arguments("word", &call.args.iter().collect::<Vec<_>>());
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(name) = mac.path.segments.last().filter(|s| s.ident == "msg" || s.ident == "ui_msg") {
            // Include the macro's name and ! so the same scanner handles outer and nested markers.
            let mut tokens = TokenStream::new();
            tokens.extend([
                TokenTree::Ident(proc_macro2::Ident::new(&name.ident.to_string(), Span::call_site())),
                TokenTree::Punct(proc_macro2::Punct::new('!', proc_macro2::Spacing::Alone)),
                TokenTree::Group(proc_macro2::Group::new(proc_macro2::Delimiter::Parenthesis, mac.tokens.clone())),
            ]);
            self.tokens(tokens);
        } else {
            self.tokens(mac.tokens.clone());
        }
    }
}

fn sources(directory: &Path, paths: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.file_stem().is_some_and(|n| n == "tests" || n.to_string_lossy().ends_with("_tests")) {
            continue;
        }
        if path.is_dir() {
            sources(&path, paths)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            paths.insert(path);
        }
    }
    Ok(())
}

pub fn extract(root: &Path, domain: &str) -> Result<Document, String> {
    let directory = match domain {
        "ui" => "crates/ui-egui/src",
        "project" => "crates/core/src",
        _ => return Err(format!("unknown domain {domain:?}")),
    };
    let mut paths = BTreeSet::new();
    sources(&root.join(directory), &mut paths)?;
    if domain == "ui" {
        sources(&root.join("crates/core/src"), &mut paths)?;
        sources(&root.join("apps/bimcraft/src"), &mut paths)?;
    }
    let mut entries = Entries::new();
    for path in paths {
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let syntax = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let relative = path.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        let markers_only = !relative.starts_with(directory);
        let mut extractor =
            Extractor { entries: &mut entries, path: relative, lines: text.lines().collect(), ui: domain == "ui", markers_only };
        extractor.visit_file(&syntax);
    }
    Ok(Document {
        entries,
        header:
            "Project-Id-Version: BIMCraft\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n"
                .into(),
        header_comments: vec![format!("# BIMCraft {domain} translation template."), "# SPDX-License-Identifier: MIT OR Apache-2.0".into()],
        ..Document::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_covers_helpers_markers_macros_and_contexts_but_excludes_tests() {
        let code = r#"
            fn window() {
                t("Direct"); tf("Value {}", &["user data"]);
                big_button(ui, icon, "Button", false, "Tip");
                let labels = [msg!("Dynamic"), msg!("verb", "Open")];
                let nested = vec![msg!("Nested"), t("In macro")];
                tc("adjective", "Open");
            }
            #[cfg(test)] mod tests { fn test() { t("Test only"); } }
        "#;
        let mut entries = Entries::new();
        Extractor { entries: &mut entries, path: "window.rs".into(), lines: code.lines().collect(), ui: true, markers_only: false }
            .visit_file(&syn::parse_file(code).unwrap());
        let keys: BTreeSet<_> = entries.keys().map(|(c, s)| (c.as_str(), s.as_str())).collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                ("", "Direct"),
                ("", "Value {}"),
                ("", "Button"),
                ("", "Tip"),
                ("", "Dynamic"),
                ("verb", "Open"),
                ("adjective", "Open"),
                ("", "Nested"),
                ("", "In macro")
            ])
        );
    }
}
