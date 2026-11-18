//! The Markdown a plugin's report may be written in: the subset the report window draws.
//!
//! The window parses the text once, when the report opens, into [`Document`]'s blocks, and draws those blocks on
//! every frame. The plugin never draws anything: it sends text, and this module is what turns it into headings,
//! paragraphs, lists, tables, code, quotes, rules and inline spans.
//!
//! The subset is deliberately small and predictable (see `docs/areas/plugins.md`): ATX headings, paragraphs,
//! strong and emphasised text, strikethrough, inline code, fenced code, bullet and numbered lists, GFM tables,
//! quotes and rules. Anything else — HTML, images, syntax highlighting, footnotes — stays text and changes
//! nothing. Links are shown with their address on hover; only `http` and `https` may be opened, and only when
//! the user clicks them. A code fence that is never closed is the one error the parser reports, so the window
//! can fall back to plain text with a warning.

use egui::{
    Align, Color32, FontId, Frame, Label, Layout, Margin, OpenUrl, Rangef, RichText, ScrollArea, Sense, Stroke, TextStyle, TextWrapMode,
    Ui, vec2,
};

use crate::style::pal;

/// The most text a report may hold: a longer one is cut at a character boundary and the window says so.
pub(crate) const MAX_REPORT_TEXT: usize = 256 * 1024;
/// The most blocks one report draws.
const MAX_BLOCKS: usize = 4096;
/// The deepest list indentation drawn, and the deepest quote parsed.
const MAX_DEPTH: usize = 8;
/// The longest line a table may have; a longer one stays text.
const MAX_TABLE_LINE: usize = 4096;
/// The longest link that may be opened.
const MAX_LINK: usize = 2048;
/// How far an emphasis marker may recurse into itself.
const MAX_INLINE_DEPTH: usize = 8;

/// A parsed report: the blocks the window draws, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Document {
    pub(crate) blocks: Vec<Block>,
}

/// One block of a report.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Block {
    Heading { level: u8, spans: Vec<Span> },
    Paragraph(Vec<Span>),
    List(List),
    Code(String),
    Quote(Vec<Block>),
    Rule,
    Table(Table),
}

/// A bullet or numbered list. Items keep the indentation they were written with, so a nested list is a list of
/// items with a deeper indent rather than a tree: what the window draws is the same.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct List {
    pub(crate) items: Vec<ListItem>,
}

/// One item of a list: its marker and its text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListItem {
    pub(crate) indent: usize,
    pub(crate) ordered: bool,
    pub(crate) number: u64,
    pub(crate) spans: Vec<Span>,
}

/// A GFM table: the columns' alignment, the header and the rows. Its columns are measured when it is drawn, so a
/// long cell widens the table rather than wrapping inside a column.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    pub(crate) aligns: Vec<Column>,
    pub(crate) header: Vec<Vec<Span>>,
    pub(crate) rows: Vec<Vec<Vec<Span>>>,
}

/// What a table's column lines up with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Column {
    Left,
    Center,
    Right,
}

/// A run of text with the inline marks that apply to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) strong: bool,
    pub(crate) italic: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
    pub(crate) link: Option<String>,
}

/// The Markdown marks a recursive parse adds to the spans it returns.
#[derive(Clone, Copy, Default)]
struct Style {
    strong: bool,
    italic: bool,
    strike: bool,
}

impl Style {
    fn apply(self, span: &mut Span) {
        span.strong |= self.strong;
        span.italic |= self.italic;
        span.strike |= self.strike;
    }
}

/// Parses `text` into the blocks the window draws. An unclosed code fence is the only error: the caller then
/// shows the report as plain text and one line of warning.
pub(crate) fn parse(text: &str) -> Result<Document, String> {
    let lines: Vec<&str> = text.lines().collect();
    let blocks = parse_lines(&lines, 0)?;
    Ok(Document { blocks })
}

fn parse_lines(lines: &[&str], depth: usize) -> Result<Vec<Block>, String> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() && blocks.len() < MAX_BLOCKS {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some((code, next)) = code_block(lines, i)? {
            blocks.push(Block::Code(code));
            i = next;
            continue;
        }
        if depth < MAX_DEPTH && line.trim_start().starts_with('>') {
            let (inner, next) = quote(lines, i);
            blocks.push(Block::Quote(parse_lines(&inner, depth + 1)?));
            i = next;
            continue;
        }
        if let Some((level, text)) = heading(line) {
            blocks.push(Block::Heading { level, spans: inline(text) });
            i += 1;
            continue;
        }
        if rule(line) {
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        if let Some((table, next)) = table(lines, i) {
            blocks.push(Block::Table(table));
            i = next;
            continue;
        }
        if marker(line).is_some() {
            let (list, next) = list(lines, i);
            blocks.push(Block::List(list));
            i = next.max(i + 1);
            continue;
        }
        let (spans, next) = paragraph(lines, i);
        if !spans.is_empty() {
            blocks.push(Block::Paragraph(spans));
        }
        i = next.max(i + 1);
    }
    Ok(blocks)
}

/// The fence that opens a code block at `line`: its character and how long it is.
fn fence(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let fence = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|c| *c == fence).count();
    (n >= 3 && line.len() - t.len() <= 3).then_some((fence, n))
}

/// A fenced code block, with its fence's info string ignored and its lines joined back together.
fn code_block(lines: &[&str], i: usize) -> Result<Option<(String, usize)>, String> {
    let Some(&line) = lines.get(i) else { return Ok(None) };
    let Some((fence, n)) = fence(line) else { return Ok(None) };
    let mut content = Vec::new();
    let mut j = i + 1;
    while let Some(&next) = lines.get(j) {
        let trimmed = next.trim();
        let closing = trimmed.chars().take_while(|c| *c == fence).count();
        if closing >= n && trimmed.chars().all(|c| c == fence) {
            return Ok(Some((content.join("\n"), j + 1)));
        }
        content.push(next);
        j += 1;
    }
    Err("the code block is never closed".to_string())
}

/// The lines of a quote, with their `>` and the space after it taken off.
fn quote<'a>(lines: &[&'a str], i: usize) -> (Vec<&'a str>, usize) {
    let mut inner = Vec::new();
    let mut j = i;
    while let Some(&line) = lines.get(j) {
        let Some(rest) = line.trim_start().strip_prefix('>') else { break };
        inner.push(rest.strip_prefix(' ').unwrap_or(rest));
        j += 1;
    }
    (inner, j)
}

/// An ATX heading: one to six `#`, then a space and the heading's text.
fn heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    let level = t.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &t[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((level as u8, rest.trim()))
}

/// A horizontal rule: three or more of `-`, `*` or `_`, spaces allowed between them.
fn rule(line: &str) -> bool {
    let mut marks = line.trim().chars().filter(|c| !c.is_whitespace());
    let Some(first) = marks.next() else { return false };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut count = 1;
    for c in marks {
        if c != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// A list marker: its indentation in spaces (a tab counts four), whether it is numbered and with what, and the
/// item's text.
struct Marker<'a> {
    indent: usize,
    ordered: bool,
    number: u64,
    content: &'a str,
}

fn marker(line: &str) -> Option<Marker<'_>> {
    let mut indent = 0;
    let mut at = 0;
    for c in line.chars() {
        match c {
            ' ' => indent += 1,
            '\t' => indent += 4,
            _ => break,
        }
        at += c.len_utf8();
    }
    let rest = &line[at..];
    for mark in ["- ", "* ", "+ "] {
        if let Some(content) = rest.strip_prefix(mark) {
            return Some(Marker { indent, ordered: false, number: 0, content });
        }
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && digits <= 9 {
        let tail = &rest[digits..];
        if let Some(content) = tail.strip_prefix(". ").or_else(|| tail.strip_prefix(") ")) {
            return Some(Marker { indent, ordered: true, number: rest[..digits].parse::<u64>().unwrap_or(1), content });
        }
    }
    None
}

/// The indentation of a line in spaces, a tab counting four.
fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// A list block: consecutive items, each keeping its own indentation. A line indented deeper than its marker is
/// more of the item's text; a blank line ends the block.
fn list(lines: &[&str], i: usize) -> (List, usize) {
    let mut items = Vec::new();
    let mut j = i;
    while let Some(&line) = lines.get(j) {
        let Some(m) = marker(line) else { break };
        let base = m.indent;
        let mut spans = inline(m.content.trim());
        j += 1;
        while let Some(&next) = lines.get(j) {
            if next.trim().is_empty() || marker(next).is_some() {
                break;
            }
            if leading_spaces(next) > base {
                if !spans.is_empty() {
                    spans.push(Span { text: " ".to_string(), ..Span::default() });
                }
                spans.extend(inline(next.trim()));
                j += 1;
            } else {
                break;
            }
        }
        items.push(ListItem { indent: (base / 2).min(MAX_DEPTH), ordered: m.ordered, number: m.number, spans });
    }
    (List { items }, j)
}

/// A paragraph: the lines up to a blank one or up to a block that may interrupt it, joined with spaces.
fn paragraph(lines: &[&str], i: usize) -> (Vec<Span>, usize) {
    let mut text = String::new();
    let mut j = i;
    while let Some(&line) = lines.get(j) {
        if line.trim().is_empty() || (j > i && interrupts(lines, j)) {
            break;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(line.trim());
        j += 1;
    }
    (inline(&text), j)
}

/// Whether the line at `i` starts a block of its own, so a paragraph above it ends there.
fn interrupts(lines: &[&str], i: usize) -> bool {
    let Some(&line) = lines.get(i) else { return true };
    heading(line).is_some()
        || rule(line)
        || line.trim_start().starts_with('>')
        || marker(line).is_some()
        || fence(line).is_some()
        || table(lines, i).is_some()
}

/// A GFM table: a header line, a divider with the alignment, then rows. Without the divider the header stays a
/// paragraph.
fn table(lines: &[&str], i: usize) -> Option<(Table, usize)> {
    let header_line = *lines.get(i)?;
    if header_line.chars().count() > MAX_TABLE_LINE || !has_unescaped_pipe(header_line) {
        return None;
    }
    let divider = *lines.get(i + 1)?;
    if divider.chars().count() > MAX_TABLE_LINE {
        return None;
    }
    let aligns = delimiter(divider)?;
    let columns = aligns.len();
    if columns == 0 {
        return None;
    }
    let header = row(header_line, columns);
    let mut rows = Vec::new();
    let mut j = i + 2;
    while let Some(&line) = lines.get(j) {
        if line.trim().is_empty()
            || line.chars().count() > MAX_TABLE_LINE
            || !has_unescaped_pipe(line)
            || heading(line).is_some()
            || rule(line)
            || marker(line).is_some()
            || line.trim_start().starts_with('>')
        {
            break;
        }
        rows.push(row(line, columns));
        j += 1;
    }
    Some((Table { aligns, header, rows }, j))
}

/// The divider under a table's header: `---`, `:---`, `:---:` or `---:`, one cell for a column.
fn delimiter(line: &str) -> Option<Vec<Column>> {
    let cells = split_cells(line);
    if cells.is_empty() {
        return None;
    }
    let mut aligns = Vec::with_capacity(cells.len());
    for cell in cells {
        let c = cell.trim();
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        let dashes = c.trim_start_matches(':').trim_end_matches(':');
        if dashes.is_empty() || !dashes.chars().all(|ch| ch == '-') {
            return None;
        }

        aligns.push(match (left, right) {
            (true, true) => Column::Center,
            (false, true) => Column::Right,
            _ => Column::Left,
        });
    }
    Some(aligns)
}

/// One table row, with missing cells filled in and extra cells dropped.
fn row(line: &str, columns: usize) -> Vec<Vec<Span>> {
    let mut cells = split_cells(line);
    cells.resize(columns, String::new());
    cells.truncate(columns);
    cells.iter().map(|cell| inline(cell)).collect()
}

/// A line's cells: `|` divides them, `\|` is a literal vertical bar, even inside inline code.
fn split_cells(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                    cell.push('|');
                } else {
                    cell.push('\\');
                }
            }
            '|' => {
                cells.push(cell.trim().to_string());
                cell.clear();
            }
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    if line.trim_start().starts_with('|') && cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    if line.trim_end().ends_with('|') && cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

/// Whether a line has a vertical bar that divides cells: one no backslash escapes.
fn has_unescaped_pipe(line: &str) -> bool {
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            return true;
        }
    }
    false
}

/// Parses inline marks: escapes, code, links, strong, emphasis and strikethrough. Unmatched marks stay text.
fn inline(text: &str) -> Vec<Span> {
    inline_marked(text, Style::default(), 0)
}

fn inline_marked(text: &str, style: Style, depth: usize) -> Vec<Span> {
    let mut spans = Vec::new();
    if depth > MAX_INLINE_DEPTH {
        push_span(
            &mut spans,
            Span { text: text.to_string(), strong: style.strong, italic: style.italic, strike: style.strike, ..Span::default() },
        );
        return spans;
    }
    let mut run = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let bytes = text.as_bytes();
        if bytes[i] == b'\\' {
            let mut chars = rest.chars();
            chars.next();
            match chars.next() {
                Some(c) if c.is_ascii_punctuation() => {
                    run.push(c);
                    i += 1 + c.len_utf8();
                }
                _ => {
                    run.push('\\');
                    i += 1;
                }
            }
            continue;
        }
        if bytes[i] == b'`' {
            let n = rest.chars().take_while(|c| *c == '`').count();
            let from = i + n;
            if let Some(content) = text[from..].find(&"`".repeat(n)).map(|at| (from, from + at)) {
                flush(&mut spans, &mut run, style);
                push_span(&mut spans, Span { text: text[content.0..content.1].to_string(), code: true, ..Span::default() });
                i = content.1 + n;
                continue;
            }
        }
        if rest.starts_with("![")
            && let Some(end) = image_end(rest)
        {
            run.push_str(&rest[..end]);
            i += end;
            continue;
        }
        if rest.starts_with('[')
            && let Some((label, url, end)) = link(rest)
        {
            flush(&mut spans, &mut run, style);
            let mut inner = inline_marked(label, style, depth + 1);
            let link = (!url.trim().is_empty()).then(|| url.trim().to_string());
            for span in &mut inner {
                span.link = link.clone();
            }
            for span in inner {
                push_span(&mut spans, span);
            }
            i += end;
            continue;
        }
        let mut pushed = false;
        for (marker, mark) in [("**", Style { strong: true, ..Style::default() }), ("__", Style { strong: true, ..Style::default() })] {
            if let Some(at) = span_open(text, i, marker, depth) {
                flush(&mut spans, &mut run, style);
                let mut inner = inline_marked(&text[i + marker.len()..at], style.and(mark), depth + 1);
                for span in &mut inner {
                    mark.apply(span);
                }
                spans.append(&mut inner);
                i = at + marker.len();
                pushed = true;
                break;
            }
        }
        if pushed {
            continue;
        }
        if let Some(at) = span_open(text, i, "~~", depth) {
            flush(&mut spans, &mut run, style);
            let mut inner = inline_marked(&text[i + 2..at], style.and(Style { strike: true, ..Style::default() }), depth + 1);
            for span in &mut inner {
                Style { strike: true, ..Style::default() }.apply(span);
            }
            spans.append(&mut inner);
            i = at + 2;
            continue;
        }
        let emphasis = rest.starts_with('*').then_some('*').or_else(|| rest.starts_with('_').then_some('_'));
        if let Some(ch) = emphasis {
            let word = ch == '_' && text[..i].chars().next_back().is_some_and(char::is_alphanumeric);
            if !word && let Some(at) = span_open(text, i, ch.encode_utf8(&mut [0; 4]), depth) {
                flush(&mut spans, &mut run, style);
                let mut inner = inline_marked(&text[i + 1..at], style.and(Style { italic: true, ..Style::default() }), depth + 1);
                for span in &mut inner {
                    Style { italic: true, ..Style::default() }.apply(span);
                }
                spans.append(&mut inner);
                i = at + 1;
                continue;
            }
        }
        let mut chars = rest.chars();
        match chars.next() {
            Some(c) => {
                run.push(c);
                i += c.len_utf8();
            }
            None => break,
        }
    }
    flush(&mut spans, &mut run, style);
    spans
}

impl Style {
    fn and(self, other: Style) -> Style {
        Style { strong: self.strong || other.strong, italic: self.italic || other.italic, strike: self.strike || other.strike }
    }
}

/// Where the mark opened at `i` closes, if it does. A mark needs text after it to open, text before it to close,
/// and it may not be escaped.
fn span_open(text: &str, i: usize, marker: &str, depth: usize) -> Option<usize> {
    if depth > MAX_INLINE_DEPTH || !text[i..].starts_with(marker) {
        return None;
    }
    let from = i + marker.len();
    let next = text[from..].chars().next()?;
    if next.is_whitespace() {
        return None;
    }
    let mut at = from;
    while let Some(rel) = text.get(at..)?.find(marker) {
        let pos = at + rel;
        if pos > from && !escaped(text, pos) && text[from..pos].chars().next_back().is_some_and(|c| !c.is_whitespace()) {
            return Some(pos);
        }
        at = pos + marker.len();
    }
    None
}

fn escaped(text: &str, at: usize) -> bool {
    let bytes = text.as_bytes();
    let mut backslashes = 0;
    let mut i = at;
    while i > 0 && bytes[i - 1] == b'\\' {
        backslashes += 1;
        i -= 1;
    }
    backslashes % 2 == 1
}

/// Where an image `![alt](url)` ends: it stays text, and this stops its bracket from becoming a link.
fn image_end(text: &str) -> Option<usize> {
    let close = text.find("](")?;
    let after = close + 2;
    let end = text[after..].find(')')?;
    Some(after + end + 1)
}

/// A link `[text](url)`: the label, the address and where the whole thing ends.
fn link(text: &str) -> Option<(&str, &str, usize)> {
    let close = text.find("](")?;
    let label = text.get(1..close)?;
    let from = close + 2;
    let end = text.get(from..)?.find(')')? + from;
    Some((label, text.get(from..end)?, end + 1))
}

fn flush(spans: &mut Vec<Span>, run: &mut String, style: Style) {
    if run.is_empty() {
        return;
    }
    push_span(
        spans,
        Span { text: std::mem::take(run), strong: style.strong, italic: style.italic, strike: style.strike, ..Span::default() },
    );
}

fn push_span(spans: &mut Vec<Span>, span: Span) {
    if span.text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.strong == span.strong
        && last.italic == span.italic
        && last.strike == span.strike
        && last.code == span.code
        && last.link == span.link
    {
        last.text.push_str(&span.text);
        return;
    }
    spans.push(span);
}

/// Whether a link's address may be opened: `http` or `https`, no control characters, and no longer than
/// [`MAX_LINK`]. Everything else — `javascript:`, `data:`, `file:`, `ftp:`, a relative path — stays text.
pub(crate) fn active_link(url: &str) -> bool {
    let url = url.trim();
    !url.is_empty()
        && url.len() <= MAX_LINK
        && !url.chars().any(char::is_control)
        && (url.as_bytes().get(..8).is_some_and(|p| p.eq_ignore_ascii_case(b"https://"))
            || url.as_bytes().get(..7).is_some_and(|p| p.eq_ignore_ascii_case(b"http://")))
}

/// Draws a parsed report's blocks. The window's `popup::body` owns the vertical scroll: this draws no vertical
/// [`ScrollArea`] of its own, and only a table or a code block may scroll sideways.
pub(crate) fn show(ui: &mut Ui, document: &Document) {
    let mut nth = 0_u64;
    show_blocks(ui, &document.blocks, &mut nth);
}

fn show_blocks(ui: &mut Ui, blocks: &[Block], nth: &mut u64) {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            ui.add_space(6.0);
        }
        show_block(ui, block, nth);
    }
}

fn show_block(ui: &mut Ui, block: &Block, nth: &mut u64) {
    match block {
        Block::Heading { level, spans } => {
            let size = match level {
                1 => 22.0,
                2 => 18.0,
                3 => 16.0,
                4 => 14.5,
                5 => 13.5,
                _ => 13.0,
            };
            inline_ui(ui, spans, Some(size), true, 0.0, true);
        }
        Block::Paragraph(spans) => inline_ui(ui, spans, None, false, 0.0, true),
        Block::List(list) => show_list(ui, list),
        Block::Code(code) => show_code(ui, code, nth),
        Block::Quote(blocks) => show_quote(ui, blocks, nth),
        Block::Rule => {
            ui.add_space(2.0);
            ui.add(egui::Separator::default().spacing(6.0));
        }
        Block::Table(table) => show_table(ui, table, nth),
    }
}

fn show_list(ui: &mut Ui, list: &List) {
    for item in &list.items {
        ui.horizontal_top(|ui| {
            ui.add_space(8.0 + item.indent as f32 * 14.0);
            let marker = if item.ordered { format!("{}.", item.number) } else { "•".to_string() };
            ui.add(Label::new(RichText::new(marker).color(pal().muted)).selectable(false));
            ui.add_space(2.0);
            ui.vertical(|ui| inline_ui(ui, &item.spans, None, false, 0.0, true));
        });
    }
}

fn show_code(ui: &mut Ui, code: &str, nth: &mut u64) {
    Frame::new().fill(pal().bar).corner_radius(4.0).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ScrollArea::horizontal().id_salt(("plugin-report-code", *nth)).auto_shrink([false, true]).show(ui, |ui| {
            let height = ui.text_style_height(&TextStyle::Monospace);
            for line in code.split('\n') {
                if line.is_empty() {
                    ui.add_space(height);
                } else {
                    ui.add(Label::new(RichText::new(line).monospace()).selectable(true).wrap_mode(TextWrapMode::Extend));
                }
            }
        });
    });
    *nth = nth.wrapping_add(1);
}

fn show_quote(ui: &mut Ui, blocks: &[Block], nth: &mut u64) {
    let frame = Frame::new()
        .fill(pal().bar)
        .inner_margin(Margin { left: 12, right: 8, top: 4, bottom: 4 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            show_blocks(ui, blocks, nth);
        })
        .response
        .rect;
    ui.painter().vline(frame.left() + 1.5, Rangef::new(frame.top() + 2.0, frame.bottom() - 2.0), Stroke::new(3.0, pal().border));
}

fn show_table(ui: &mut Ui, table: &Table, nth: &mut u64) {
    let widths = table_widths(ui, table);
    let total: f32 = widths.iter().sum();
    ScrollArea::horizontal().id_salt(("plugin-report-table", *nth)).auto_shrink([true, true]).show(ui, |ui| {
        let left = ui.cursor().left();
        let top = ui.cursor().top();
        Frame::new().fill(pal().bar).inner_margin(Margin::symmetric(0, 4)).show(ui, |ui| {
            ui.set_width(total);
            table_row(ui, &widths, &table.aligns, &table.header, true);
        });
        let y = ui.cursor().top();
        ui.painter().hline(Rangef::new(left, left + total), y, Stroke::new(1.0, pal().border));
        for row in &table.rows {
            ui.add_space(2.0);
            table_row(ui, &widths, &table.aligns, row, false);
            ui.add_space(2.0);
            let y = ui.cursor().top();
            ui.painter().hline(Rangef::new(left, left + total), y, Stroke::new(1.0, pal().border));
        }
        let bottom = ui.cursor().top().max(top + 1.0);
        let mut x = left;
        for (i, width) in widths.iter().enumerate() {
            if i > 0 {
                ui.painter().vline(x, Rangef::new(top, bottom), Stroke::new(1.0, pal().border));
            }
            x += *width;
        }
        ui.painter().vline(left, Rangef::new(top, bottom), Stroke::new(1.0, pal().border));
        ui.painter().vline(left + total, Rangef::new(top, bottom), Stroke::new(1.0, pal().border));
    });
    *nth = nth.wrapping_add(1);
}

fn table_row(ui: &mut Ui, widths: &[f32], aligns: &[Column], cells: &[Vec<Span>], head: bool) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, width) in widths.iter().enumerate() {
            let cell = cells.get(i).map_or(&[][..], |spans| spans.as_slice());
            ui.allocate_ui_with_layout(vec2(*width, 0.0), Layout::top_down(Align::LEFT), |ui| {
                ui.set_min_width(*width);
                ui.set_max_width(*width);
                ui.set_min_height(ui.text_style_height(&TextStyle::Body));
                Frame::new().inner_margin(Margin::symmetric(6, 2)).show(ui, |ui| {
                    let available = (*width - 12.0).max(1.0);
                    ui.set_width(available);
                    let content = inline_width(ui, cell);
                    let lead = match aligns.get(i).copied().unwrap_or(Column::Left) {
                        Column::Left => 0.0,
                        Column::Center => ((available - content) / 2.0).max(0.0),
                        Column::Right => (available - content).max(0.0),
                    };
                    inline_ui(ui, cell, None, head, lead, false);
                });
            });
        }
    });
}

/// Each column's width in points, from its widest cell: no cell wraps, so a long one widens the table, and the
/// table itself scrolls sideways when it outgrows the window.
fn table_widths(ui: &Ui, table: &Table) -> Vec<f32> {
    let mut widths = vec![60.0_f32; table.aligns.len()];
    for row in std::iter::once(&table.header).chain(table.rows.iter()) {
        for (at, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(at) {
                *width = width.max(inline_width(ui, cell) + 16.0);
            }
        }
    }
    widths
}

/// The width of inline spans as they are drawn: each run in its own font, since code is monospace.
fn inline_width(ui: &Ui, spans: &[Span]) -> f32 {
    let body = ui.style().text_styles.get(&TextStyle::Body).cloned().unwrap_or_else(|| FontId::proportional(14.0));
    let mono = ui.style().text_styles.get(&TextStyle::Monospace).cloned().unwrap_or_else(|| FontId::monospace(14.0));
    let width: f32 = spans
        .iter()
        .map(|span| {
            let font = if span.code { mono.clone() } else { body.clone() };
            ui.ctx().fonts_mut(|fonts| fonts.layout_no_wrap(span.text.clone(), font, Color32::PLACEHOLDER)).size().x
        })
        .sum();
    width
}

fn inline_ui(ui: &mut Ui, spans: &[Span], size: Option<f32>, strong: bool, lead: f32, wrap: bool) {
    if wrap {
        ui.horizontal_wrapped(|ui| inline_row(ui, spans, size, strong, lead, true));
    } else {
        ui.horizontal(|ui| inline_row(ui, spans, size, strong, lead, false));
    }
}

fn inline_row(ui: &mut Ui, spans: &[Span], size: Option<f32>, strong: bool, lead: f32, wrap: bool) {
    ui.spacing_mut().item_spacing.x = 0.0;
    if lead > 0.0 {
        ui.add_space(lead);
    }
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        let active = span.link.as_deref().filter(|url| active_link(url));
        let mut text = RichText::new(&span.text);
        if strong || span.strong {
            text = text.strong();
        }
        if span.italic {
            text = text.italics();
        }
        if span.strike {
            text = text.strikethrough();
        }
        if span.code {
            text = text.code();
        }
        if let Some(size) = size {
            text = text.size(size);
        }
        if active.is_some() {
            text = text.color(pal().accent_ink).underline();
        }
        let mut label = Label::new(text).selectable(true);
        if !wrap {
            label = label.wrap_mode(TextWrapMode::Extend);
        }
        if active.is_some() {
            label = label.sense(Sense::click_and_drag());
        }
        let response = ui.add(label);
        if let Some(url) = active {
            if response.clicked() {
                ui.ctx().open_url(OpenUrl::new_tab(url));
            }
            let _ = response.on_hover_text(url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraphs(document: &Document) -> Vec<&[Span]> {
        document.blocks.iter().filter_map(|block| if let Block::Paragraph(spans) = block { Some(spans.as_slice()) } else { None }).collect()
    }

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn headings_paragraphs_lists_and_tables_become_blocks() {
        let document =
            parse("# Report\n\nIntro **bold** and `code`.\n\n- one\n- two\n\n| Category | Count |\n|---|---:|\n| Walls | 12 |\n").unwrap();
        assert!(matches!(document.blocks.first(), Some(Block::Heading { level: 1, .. })));
        assert!(paragraphs(&document).iter().any(|spans| text(spans) == "Intro bold and code."));
        assert!(document.blocks.iter().any(|block| matches!(block, Block::List(list) if list.items.len() == 2)));
        assert!(matches!(document.blocks.last(), Some(Block::Table(table)) if table.header.len() == 2 && table.rows.len() == 1));
    }

    #[test]
    fn markdown_table_renders_header_rows_and_column_alignment() {
        let document = parse("| Category | Count | Share |\n| --- | ---: | :---: |\n| Walls | 12 | 60 % |\n").unwrap();
        let Some(Block::Table(table)) = document.blocks.first() else { panic!("a table") };
        assert_eq!(table.aligns, [Column::Left, Column::Right, Column::Center]);
        assert_eq!(text(&table.header[1]), "Count");
        assert_eq!(text(&table.rows[0][1]), "12");
    }

    #[test]
    fn markdown_table_escaped_pipe_stays_inside_one_cell() {
        let document = parse("| A | B |\n|---|---|\n| one \\| two | three |\n").unwrap();
        let Some(Block::Table(table)) = document.blocks.first() else { panic!("a table") };
        assert_eq!(text(&table.rows[0][0]), "one | two");
        assert_eq!(table.rows[0].len(), 2);
    }

    #[test]
    fn markdown_table_short_row_pads_and_long_row_drops_extra_cells() {
        let document = parse("| A | B | C |\n|---|---|---|\n| one |\n| one | two | three | four |\n").unwrap();
        let Some(Block::Table(table)) = document.blocks.first() else { panic!("a table") };
        assert_eq!(text(&table.rows[0][1]), "");
        assert_eq!(table.rows[0].len(), 3);
        assert_eq!(table.rows[1].len(), 3);
    }

    #[test]
    fn markdown_table_without_a_divider_stays_text() {
        let document = parse("| Category | Count |\n| Walls | 12 |\n").unwrap();
        assert!(!document.blocks.iter().any(|block| matches!(block, Block::Table(_))));
        let joined: String =
            document.blocks.iter().filter_map(|b| if let Block::Paragraph(s) = b { Some(text(s)) } else { None }).collect();
        assert!(joined.contains("| Category | Count |"), "{joined}");
    }

    #[test]
    fn broken_markdown_falls_back_to_plain_text() {
        assert!(parse("```\nunclosed\n").is_err());
    }

    #[test]
    fn report_html_and_images_stay_text() {
        let document = parse("<script>alert(1)</script> ![alt](https://example.org/a.png)\n").unwrap();
        let joined = text(paragraphs(&document)[0]);
        assert_eq!(joined, "<script>alert(1)</script> ![alt](https://example.org/a.png)");
        assert!(paragraphs(&document)[0].iter().all(|span| span.link.is_none()));
    }

    #[test]
    fn report_links_with_javascript_or_file_scheme_are_not_active() {
        assert!(!active_link("javascript:alert(1)"));
        assert!(!active_link("file:///etc/passwd"));
        assert!(!active_link("data:text/html,hi"));
        assert!(!active_link("/relative"));
        assert!(active_link("https://example.org/report"));
        assert!(active_link("HTTP://example.org"));
        assert!(!active_link(&format!("https://example.org/{}", "a".repeat(MAX_LINK))));
    }

    #[test]
    fn inline_marks_claim_their_text_and_unmatched_marks_stay_text() {
        let spans = inline("**bold** *italic* ~~gone~~ `code` [site](https://example.org)");
        assert!(spans.iter().any(|s| s.text == "bold" && s.strong));
        assert!(spans.iter().any(|s| s.text == "italic" && s.italic));
        assert!(spans.iter().any(|s| s.text == "gone" && s.strike));
        assert!(spans.iter().any(|s| s.text == "code" && s.code));
        assert!(spans.iter().any(|s| s.text == "site" && s.link.as_deref() == Some("https://example.org")));
        assert_eq!(text(&inline("2 * 3")), "2 * 3");
    }

    #[test]
    fn markdown_report_does_not_block_the_frame_on_a_typical_report() {
        let mut text = String::from("# Report\n\n");
        for i in 0..500 {
            text.push_str(&format!("- item {i}\n"));
        }
        text.push_str("\n| Category | Count | Share |\n|---|---:|---:|\n");
        for i in 0..200 {
            text.push_str(&format!("| Category {i} | {i} | {} % |\n", i % 100));
        }
        let started = std::time::Instant::now();
        let document = parse(&text).unwrap();
        let elapsed = started.elapsed();
        assert!(document.blocks.len() >= 3);
        assert!(elapsed < std::time::Duration::from_millis(16), "parsing took {elapsed:?}");
    }

    #[test]
    fn nested_lists_keep_their_indentation_and_numbers() {
        let document = parse("- one\n  - nested\n\n1. first\n2. second\n").unwrap();
        let lists: Vec<&List> = document.blocks.iter().filter_map(|b| if let Block::List(l) = b { Some(l) } else { None }).collect();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].items[1].indent, 1);
        assert_eq!(lists[1].items[1].number, 2);
    }
}
