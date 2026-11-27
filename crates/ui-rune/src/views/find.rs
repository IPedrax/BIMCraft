//! The browser's search: the words typed, and where a word matches a name, with egui's wildcards (`*` any run of characters, `?` any one,
//! case aside). Pure, so what a search finds is tested without a window. egui's `browser.rs` is the reference.

/// How long a search may be, in characters.
pub const MAX_FIND: usize = 64;

/// Where `word` matches in `text`, case aside: `*` any run of characters, `?` any one. The spans, in bytes, each as short as it can
/// be; none when it doesn't match, and an empty list when it matches nothing but itself (`*`).
pub fn spans(text: &str, word: &str) -> Option<Vec<(usize, usize)>> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let at: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold(c))).collect();
    let pattern: Vec<char> = word.chars().map(fold).collect();
    let n = at.len();
    let byte = |i: usize| at.get(i).map_or(text.len(), |(b, _)| *b);
    // The shortest match from `start`, if any: where it ends.
    let end_from = |start: usize| {
        let mut can = vec![false; n + 1];
        can[start] = true;
        for &c in &pattern {
            let mut next = vec![false; n + 1];
            match c {
                '*' => {
                    if let Some(first) = can.iter().position(|b| *b) {
                        next[first..].iter_mut().for_each(|b| *b = true);
                    }
                }
                _ => {
                    for (i, (_, ch)) in at.iter().enumerate() {
                        if can[i] && (c == '?' || *ch == c) {
                            next[i + 1] = true;
                        }
                    }
                }
            }
            can = next;
        }
        can.iter().position(|b| *b)
    };
    let mut out = Vec::new();
    let mut matched = false;
    let mut start = 0;
    while start <= n {
        match end_from(start) {
            Some(end) if end > start => {
                out.push((byte(start), byte(end)));
                matched = true;
                start = end;
            }
            Some(_) => {
                matched = true;
                start += 1;
            }
            None => start += 1,
        }
    }
    matched.then_some(out)
}

/// The words of a search: at most [`MAX_FIND`] characters of what is typed, split at its spaces.
pub fn words(find: &str) -> Vec<String> {
    find.chars().take(MAX_FIND).collect::<String>().split_whitespace().map(str::to_string).collect()
}

/// Whether a row shows for `words`: each must match the row's name or its group's (a word the group's name matches is spent).
pub fn shows(group: &str, row: &str, words: &[String]) -> bool {
    words.iter().all(|w| spans(group, w).is_some() || spans(row, w).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_matches_anywhere_case_aside_and_marks_each_place() {
        assert_eq!(spans("Level 1", "lev"), Some(vec![(0, 3)]));
        assert_eq!(spans("Level 1", "EL"), Some(vec![(3, 5)]));
        assert_eq!(spans("Levels", "e"), Some(vec![(1, 2), (3, 4)]), "every place");
        assert_eq!(spans("Level 1", "x"), None);
    }

    #[test]
    fn a_star_is_any_run_and_a_question_mark_any_one() {
        assert_eq!(spans("Level 12", "l*1"), Some(vec![(0, 7)]), "as short as it can");
        assert_eq!(spans("Level 12", "level ?2"), Some(vec![(0, 8)]));
        assert_eq!(spans("Level 2", "level ?2"), None, "? is one character, not none");
        assert_eq!(spans("Door", "*"), Some(vec![]), "matches, with nothing to mark");
        assert_eq!(spans("A101 - Plans", "a1?1"), Some(vec![(0, 4)]));
    }

    #[test]
    fn spans_are_in_bytes_whatever_the_letters() {
        let text = "N\u{ed}vel 1";
        let hit = spans(text, "vel").unwrap_or_default();
        assert_eq!(hit.iter().map(|(a, b)| &text[*a..*b]).collect::<Vec<_>>(), ["vel"]);
        assert_eq!(spans("\u{c9}L\u{c9}VATION", "\u{e9}l\u{e9}"), Some(vec![(0, 5)]));
    }

    #[test]
    fn a_search_is_a_few_words_and_each_must_match_the_row_or_its_group() {
        assert_eq!(words("  ceiling   2 "), ["ceiling", "2"]);
        assert!(words("   ").is_empty());
        let long = "a".repeat(MAX_FIND * 2);
        assert_eq!(words(&long)[0].len(), MAX_FIND, "a hostile search is cut");
        let w = words("ceiling 2");
        assert!(shows("Ceiling Plans", "Level 2", &w), "the group says one word, the row the other");
        assert!(!shows("Floor Plans", "Level 2", &w));
        assert!(!shows("Ceiling Plans", "Level 1", &w));
        assert!(shows("Sheets", "A101", &words("sheets")), "the group's name matches every word: all its rows show");
        assert!(!shows("Sheets", "A101", &words("nothing")));
    }

    #[test]
    fn a_hostile_pattern_is_no_hang() {
        let text = "a".repeat(200);
        assert!(spans(&text, &"*a".repeat(MAX_FIND / 2)).is_some());
        assert!(spans("", "?").is_none() && spans("", "*").is_some());
    }
}
