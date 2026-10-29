//! Our own single-stroke lettering for plan annotation: capitals, Latin and Cyrillic, digits, a few marks and the
//! accents of Latin languages (acute, grave, circumflex, tilde, diaeresis, double acute, caron, ring,
//! macron, breve, dot, cedilla, ogonek and the stroke of Ł and Ø), drawn on an 8 x 12 grid (y up).
//! Lower case is drawn as capitals, as architectural lettering is. What it has no letter for, a text
//! type in it letters from IBM Plex ([`crate::text`]).

type Glyph = &'static [&'static [(i8, i8)]];

const O: &[(i8, i8)] = &[(2, 0), (0, 2), (0, 10), (2, 12), (6, 12), (8, 10), (8, 2), (6, 0), (2, 0)];
const P: &[(i8, i8)] = &[(0, 0), (0, 12), (6, 12), (8, 10), (8, 8), (6, 6), (0, 6)];
const QUESTION: Glyph = &[&[(0, 10), (2, 12), (6, 12), (8, 10), (8, 8), (4, 5), (4, 3)], &[(4, 1), (4, 0)]];

fn glyph(c: char) -> Glyph {
    known(c).unwrap_or(QUESTION)
}

/// Its letter for capital `c`, if it has one.
fn known(c: char) -> Option<Glyph> {
    Some(match c {
        'A' => &[&[(0, 0), (4, 12), (8, 0)], &[(1, 3), (7, 3)]],
        'B' => &[&[(0, 0), (0, 12), (6, 12), (8, 10), (8, 8), (6, 6), (0, 6)], &[(6, 6), (8, 4), (8, 2), (6, 0), (0, 0)]],
        'C' => &[&[(8, 10), (6, 12), (2, 12), (0, 10), (0, 2), (2, 0), (6, 0), (8, 2)]],
        'D' => &[&[(0, 0), (0, 12), (5, 12), (8, 9), (8, 3), (5, 0), (0, 0)]],
        'E' => &[&[(8, 12), (0, 12), (0, 0), (8, 0)], &[(0, 6), (6, 6)]],
        'F' => &[&[(8, 12), (0, 12), (0, 0)], &[(0, 6), (6, 6)]],
        'G' => &[&[(8, 10), (6, 12), (2, 12), (0, 10), (0, 2), (2, 0), (6, 0), (8, 2), (8, 5), (5, 5)]],
        'H' => &[&[(0, 0), (0, 12)], &[(8, 0), (8, 12)], &[(0, 6), (8, 6)]],
        'I' => &[&[(2, 12), (6, 12)], &[(4, 12), (4, 0)], &[(2, 0), (6, 0)]],
        'J' => &[&[(8, 12), (8, 2), (6, 0), (2, 0), (0, 2)]],
        'K' => &[&[(0, 0), (0, 12)], &[(8, 12), (0, 4)], &[(3, 7), (8, 0)]],
        'L' => &[&[(0, 12), (0, 0), (8, 0)]],
        'M' => &[&[(0, 0), (0, 12), (4, 6), (8, 12), (8, 0)]],
        'N' => &[&[(0, 0), (0, 12), (8, 0), (8, 12)]],
        'O' => &[O],
        'P' => &[P],
        'Q' => &[O, &[(5, 3), (8, 0)]],
        'R' => &[P, &[(4, 6), (8, 0)]],
        'S' => &[&[(8, 10), (6, 12), (2, 12), (0, 10), (0, 8), (2, 6), (6, 6), (8, 4), (8, 2), (6, 0), (2, 0), (0, 2)]],
        'T' => &[&[(0, 12), (8, 12)], &[(4, 12), (4, 0)]],
        'U' => &[&[(0, 12), (0, 2), (2, 0), (6, 0), (8, 2), (8, 12)]],
        'V' => &[&[(0, 12), (4, 0), (8, 12)]],
        'W' => &[&[(0, 12), (2, 0), (4, 6), (6, 0), (8, 12)]],
        'X' => &[&[(0, 0), (8, 12)], &[(0, 12), (8, 0)]],
        'Y' => &[&[(0, 12), (4, 6), (8, 12)], &[(4, 6), (4, 0)]],
        'Z' => &[&[(0, 12), (8, 12), (0, 0), (8, 0)]],
        // Norwegian and Danish: an A grown into an E, its crossbar running on through the stem as the E's middle bar.
        'Æ' => &[&[(0, 0), (4, 12), (8, 12)], &[(4, 12), (4, 0), (8, 0)], &[(2, 6), (7, 6)]],
        '0' => &[&[(2, 0), (1, 2), (1, 10), (2, 12), (6, 12), (7, 10), (7, 2), (6, 0), (2, 0)]],
        '1' => &[&[(2, 10), (4, 12), (4, 0)], &[(2, 0), (6, 0)]],
        '2' => &[&[(0, 10), (2, 12), (6, 12), (8, 10), (8, 8), (0, 0), (8, 0)]],
        '3' => &[&[(0, 10), (2, 12), (6, 12), (8, 10), (8, 8), (6, 6), (8, 4), (8, 2), (6, 0), (2, 0), (0, 2)], &[(3, 6), (6, 6)]],
        '4' => &[&[(6, 0), (6, 12), (0, 4), (8, 4)]],
        '5' => &[&[(8, 12), (0, 12), (0, 7), (6, 7), (8, 5), (8, 2), (6, 0), (2, 0), (0, 2)]],
        '6' => &[&[(8, 10), (6, 12), (2, 12), (0, 10), (0, 2), (2, 0), (6, 0), (8, 2), (8, 5), (6, 7), (2, 7), (0, 5)]],
        '7' => &[&[(0, 12), (8, 12), (3, 0)]],
        '8' => &[&[
            (2, 6),
            (0, 8),
            (0, 10),
            (2, 12),
            (6, 12),
            (8, 10),
            (8, 8),
            (6, 6),
            (2, 6),
            (0, 4),
            (0, 2),
            (2, 0),
            (6, 0),
            (8, 2),
            (8, 4),
            (6, 6),
        ]],
        '9' => &[&[(0, 2), (2, 0), (6, 0), (8, 2), (8, 10), (6, 12), (2, 12), (0, 10), (0, 7), (2, 5), (6, 5), (8, 7)]],
        ' ' => &[],
        '.' => &[&[(3, 0), (4, 0), (4, 1), (3, 1), (3, 0)]],
        ',' => &[&[(4, 1), (4, 0), (3, -2)]],
        '-' => &[&[(1, 6), (7, 6)]],
        '+' => &[&[(4, 2), (4, 10)], &[(0, 6), (8, 6)]],
        '=' => &[&[(1, 4), (7, 4)], &[(1, 8), (7, 8)]],
        '_' => &[&[(0, -1), (8, -1)]],
        '/' => &[&[(0, 0), (8, 12)]],
        '(' => &[&[(6, 12), (4, 9), (4, 3), (6, 0)]],
        '{' => &[&[(6, 12), (4, 11), (4, 7), (2, 6), (4, 5), (4, 1), (6, 0)]],
        '}' => &[&[(2, 12), (4, 11), (4, 7), (6, 6), (4, 5), (4, 1), (2, 0)]],
        ')' => &[&[(2, 12), (4, 9), (4, 3), (2, 0)]],
        ':' => &[&[(4, 2), (4, 3)], &[(4, 8), (4, 9)]],
        '\'' => &[&[(4, 12), (4, 9)]],
        '"' => &[&[(3, 12), (3, 9)], &[(5, 12), (5, 9)]],
        '#' => &[&[(2, 0), (3, 12)], &[(5, 0), (6, 12)], &[(0, 4), (8, 4)], &[(0, 8), (8, 8)]],
        '×' => &[&[(1, 2), (7, 10)], &[(1, 10), (7, 2)]],
        '²' => &[&[(2, 11), (3, 12), (5, 12), (6, 11), (6, 10), (2, 7), (6, 7)]],
        '°' => &[&[(3, 12), (5, 12), (6, 11), (6, 9), (5, 8), (3, 8), (2, 9), (2, 11), (3, 12)]],
        '±' => &[&[(4, 5), (4, 11)], &[(1, 8), (7, 8)], &[(1, 2), (7, 2)]],
        // Cyrillic: the capitals the Latin alphabet shares are its strokes.
        'А' => glyph('A'),
        'В' => glyph('B'),
        'Е' => glyph('E'),
        'К' => glyph('K'),
        'М' => glyph('M'),
        'Н' => glyph('H'),
        'О' => glyph('O'),
        'Р' => glyph('P'),
        'С' => glyph('C'),
        'Т' => glyph('T'),
        'Х' => glyph('X'),
        'З' => glyph('3'),
        'Б' => &[&[(8, 12), (0, 12), (0, 0), (6, 0), (8, 2), (8, 5), (6, 7), (0, 7)]],
        'Г' => &[&[(8, 12), (0, 12), (0, 0)]],
        'Д' => &[&[(0, -2), (0, 0), (8, 0), (8, -2)], &[(1, 0), (3, 12), (7, 12), (7, 0)]],
        'Ж' => &[&[(4, 0), (4, 12)], &[(0, 12), (4, 6), (8, 12)], &[(0, 0), (4, 6), (8, 0)]],
        'И' => &[&[(0, 12), (0, 0), (8, 12), (8, 0)]],
        'Л' => &[&[(0, 0), (2, 2), (3, 12), (8, 12), (8, 0)]],
        'П' => &[&[(0, 0), (0, 12), (8, 12), (8, 0)]],
        'У' => &[&[(0, 12), (4, 5)], &[(8, 12), (3, 0), (1, 0)]],
        'Ф' => &[&[(4, 0), (4, 12)], &[(4, 10), (1, 10), (0, 9), (0, 5), (1, 4), (7, 4), (8, 5), (8, 9), (7, 10), (4, 10)]],
        'Ц' => &[&[(0, 12), (0, 0), (7, 0), (7, 12)], &[(7, 0), (8, 0), (8, -2)]],
        'Ч' => &[&[(0, 12), (0, 8), (2, 6), (8, 6)], &[(8, 12), (8, 0)]],
        'Ш' => &[&[(0, 12), (0, 0), (8, 0), (8, 12)], &[(4, 12), (4, 0)]],
        'Щ' => &[&[(0, 12), (0, 0), (6, 0), (6, 12)], &[(3, 12), (3, 0)], &[(6, 0), (8, 0), (8, -2)]],
        'Ъ' => &[&[(0, 12), (2, 12), (2, 0), (6, 0), (8, 2), (8, 5), (6, 7), (2, 7)]],
        'Ы' => &[&[(0, 12), (0, 0), (4, 0), (6, 2), (6, 5), (4, 7), (0, 7)], &[(8, 12), (8, 0)]],
        'Ь' => &[&[(0, 12), (0, 0), (6, 0), (8, 2), (8, 5), (6, 7), (0, 7)]],
        'Э' => &[&[(0, 10), (2, 12), (6, 12), (8, 10), (8, 2), (6, 0), (2, 0), (0, 2)], &[(3, 6), (8, 6)]],
        'Ю' => &[&[(0, 12), (0, 0)], &[(0, 6), (3, 6)], &[(5, 12), (3, 10), (3, 2), (5, 0), (6, 0), (8, 2), (8, 10), (6, 12), (5, 12)]],
        'Я' => &[&[(8, 0), (8, 12), (2, 12), (0, 10), (0, 8), (2, 6), (8, 6)], &[(4, 6), (0, 0)]],
        '!' => &[&[(4, 12), (4, 3)], &[(4, 1), (4, 0)]],
        ';' => &[&[(4, 8), (4, 9)], &[(4, 2), (3, -1)]],
        '*' => &[&[(4, 3), (4, 11)], &[(1, 5), (7, 9)], &[(1, 9), (7, 5)]],
        '%' => &[&[(0, 0), (8, 12)], &[(1, 12), (2, 12), (2, 10), (1, 10), (1, 12)], &[(6, 2), (7, 2), (7, 0), (6, 0), (6, 2)]],
        '&' => &[&[(8, 0), (1, 9), (1, 11), (2, 12), (4, 12), (5, 11), (5, 9), (0, 4), (0, 2), (2, 0), (5, 0), (8, 4)]],
        '[' => &[&[(6, 12), (3, 12), (3, 0), (6, 0)]],
        ']' => &[&[(2, 12), (5, 12), (5, 0), (2, 0)]],
        '<' => &[&[(7, 10), (1, 6), (7, 2)]],
        '>' => &[&[(1, 10), (7, 6), (1, 2)]],
        '@' => &[&[(6, 4), (6, 8), (3, 8), (2, 6), (3, 4), (6, 4), (8, 5), (8, 10), (6, 12), (2, 12), (0, 10), (0, 2), (2, 0), (7, 0)]],
        '?' => QUESTION,
        '\\' => &[&[(0, 12), (8, 0)]],
        '|' => &[&[(4, -2), (4, 13)]],
        '~' => &[&[(1, 6), (3, 8), (5, 6), (7, 8)]],
        '$' => &[&[(8, 10), (6, 11), (2, 11), (0, 9), (0, 8), (2, 6), (6, 6), (8, 4), (8, 3), (6, 1), (2, 1), (0, 3)], &[(4, 13), (4, -1)]],
        '€' => &[&[(8, 10), (6, 12), (3, 12), (1, 10), (1, 2), (3, 0), (6, 0), (8, 2)], &[(0, 7), (5, 7)], &[(0, 5), (5, 5)]],
        '³' => &[&[(2, 12), (6, 12), (4, 10), (6, 9), (5, 7), (2, 7)]],
        '·' => &[&[(4, 6), (4, 7)]],
        '–' | '—' => &[&[(0, 6), (8, 6)]],
        '‘' | '’' => &[&[(4, 12), (4, 9)]],
        '“' | '”' | '„' => &[&[(3, 12), (3, 9)], &[(5, 12), (5, 9)]],
        _ => return None,
    })
}

/// An accented capital as its base letter and accent.
fn accent(c: char) -> Option<(char, Glyph)> {
    const ACUTE: Glyph = &[&[(3, 13), (5, 15)]];
    const GRAVE: Glyph = &[&[(3, 15), (5, 13)]];
    const CIRCUMFLEX: Glyph = &[&[(2, 13), (4, 15), (6, 13)]];
    const TILDE: Glyph = &[&[(1, 13), (3, 15), (5, 13), (7, 15)]];
    const DIAERESIS: Glyph = &[&[(2, 14), (2, 15)], &[(6, 14), (6, 15)]];
    const DOUBLE_ACUTE: Glyph = &[&[(1, 13), (3, 15)], &[(5, 13), (7, 15)]];
    const CARON: Glyph = &[&[(2, 15), (4, 13), (6, 15)]];
    const RING: Glyph = &[&[(3, 13), (3, 15), (5, 15), (5, 13), (3, 13)]];
    const MACRON: Glyph = &[&[(2, 14), (6, 14)]];
    const BREVE: Glyph = &[&[(2, 15), (3, 13), (5, 13), (6, 15)]];
    const DOT: Glyph = &[&[(4, 14), (4, 15)]];
    const CEDILLA: Glyph = &[&[(4, 0), (5, -1), (3, -3)]];
    const OGONEK: Glyph = &[&[(6, 0), (5, -2), (7, -3)]];
    const SLASH: Glyph = &[&[(-1, -1), (9, 13)]];
    const BAR: Glyph = &[&[(-1, 4), (3, 8)]];
    let mark = match c {
        'Ё' => return Some(('Е', DIAERESIS)),
        'Й' => return Some(('И', BREVE)),
        'Á' | 'É' | 'Í' | 'Ó' | 'Ú' | 'Ý' | 'Ć' | 'Ĺ' | 'Ń' | 'Ŕ' | 'Ś' | 'Ź' => ACUTE,
        'À' | 'È' | 'Ì' | 'Ò' | 'Ù' => GRAVE,
        'Â' | 'Ê' | 'Î' | 'Ô' | 'Û' => CIRCUMFLEX,
        'Ã' | 'Õ' | 'Ñ' => TILDE,
        'Ä' | 'Ë' | 'Ï' | 'Ö' | 'Ü' | 'Ÿ' => DIAERESIS,
        'Ő' | 'Ű' => DOUBLE_ACUTE,
        'Č' | 'Ď' | 'Ě' | 'Ň' | 'Ř' | 'Š' | 'Ť' | 'Ž' => CARON,
        'Å' | 'Ů' => RING,
        'Ā' | 'Ē' | 'Ī' | 'Ō' | 'Ū' => MACRON,
        'Ă' | 'Ğ' => BREVE,
        'Ė' | 'Ż' | 'İ' => DOT,
        'Ç' | 'Ş' => CEDILLA,
        'Ą' | 'Ę' => OGONEK,
        'Ø' => SLASH,
        'Ł' => BAR,
        _ => return None,
    };
    Some((base(c)?, mark))
}

fn base(c: char) -> Option<char> {
    Some(match c {
        'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => 'A',
        'Ć' | 'Č' | 'Ç' => 'C',
        'Ď' => 'D',
        'É' | 'È' | 'Ê' | 'Ë' | 'Ě' | 'Ē' | 'Ė' | 'Ę' => 'E',
        'Ğ' => 'G',
        'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'İ' => 'I',
        'Ĺ' | 'Ł' => 'L',
        'Ñ' | 'Ń' | 'Ň' => 'N',
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'Ő' | 'Ō' | 'Ø' => 'O',
        'Ŕ' | 'Ř' => 'R',
        'Ś' | 'Š' | 'Ş' => 'S',
        'Ť' => 'T',
        'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ű' | 'Ů' | 'Ū' => 'U',
        'Ý' | 'Ÿ' => 'Y',
        'Ź' | 'Ž' | 'Ż' => 'Z',
        _ => return None,
    })
}

/// Whether it has a letter for `c` (in either case): else it draws a question mark.
pub fn has(c: char) -> bool {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => accent(u).is_some() || known(u).is_some(),
        // ß is SS.
        (Some(_), Some(_)) => c.to_uppercase().all(has),
        _ => false,
    }
}

/// Grid units from one character to the next.
const ADVANCE: f32 = 11.0;

/// How wide `text` is at `height` px.
pub fn width(text: &str, height: f32) -> f32 {
    let n = text.chars().count() as f32;
    if n == 0.0 { 0.0 } else { (n * ADVANCE - 3.0) * height / 12.0 }
}

/// Polylines drawing `text` with capitals `height` px tall, from `(x, y)` on the baseline, y down.
pub fn strokes(text: &str, x: f32, y: f32, height: f32) -> Vec<Vec<(f32, f32)>> {
    let k = height / 12.0;
    let mut out = Vec::new();
    for (i, c) in text.chars().flat_map(char::to_uppercase).enumerate() {
        let x0 = x + i as f32 * ADVANCE * k;
        let (letter, mark): (Glyph, Glyph) = match accent(c) {
            Some((b, m)) => (glyph(b), m),
            None => (glyph(c), &[]),
        };
        for line in letter.iter().chain(mark) {
            out.push(line.iter().map(|&(gx, gy)| (x0 + f32::from(gx) * k, y - f32::from(gy) * k)).collect());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_case_is_drawn_as_capitals_and_unknown_characters_as_a_question_mark() {
        assert_eq!(strokes("living", 0.0, 0.0, 12.0), strokes("LIVING", 0.0, 0.0, 12.0));
        assert_eq!(strokes("ω", 0.0, 0.0, 12.0), strokes("?", 0.0, 0.0, 12.0));
        assert!(strokes(" ", 0.0, 0.0, 12.0).is_empty());
    }

    #[test]
    fn accented_letters_are_their_base_letter_plus_an_accent() {
        let plain = strokes("A", 0.0, 0.0, 12.0);
        let acute = strokes("á", 0.0, 0.0, 12.0);
        assert_eq!(acute.len(), plain.len() + 1);
        assert_eq!(acute[..plain.len()], plain[..]);
    }

    #[test]
    fn cyrillic_capitals_are_drawn_each_its_own_and_lower_case_as_them() {
        let unknown = strokes("?", 0.0, 0.0, 12.0);
        let all = "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ";
        let drawn: Vec<_> = all.chars().map(|c| strokes(&c.to_string(), 0.0, 0.0, 12.0)).collect();
        for (c, s) in all.chars().zip(&drawn) {
            assert!(!s.is_empty() && *s != unknown, "{c} is not drawn");
        }
        for (i, a) in drawn.iter().enumerate() {
            assert!(drawn.iter().skip(i + 1).all(|b| b != a), "{} looks like another", all.chars().nth(i).unwrap_or(' '));
        }
        // The letters the Latin alphabet shares are the same strokes.
        assert_eq!(strokes("АВЕКМНОРСТХ", 0.0, 0.0, 12.0), strokes("ABEKMHOPCTX", 0.0, 0.0, 12.0));
        assert_eq!(strokes("шлейф ёж", 0.0, 0.0, 12.0), strokes("ШЛЕЙФ ЁЖ", 0.0, 0.0, 12.0));
        // Й and Ё are И and Е with their marks over them.
        let (i, short) = (strokes("И", 0.0, 0.0, 12.0), strokes("Й", 0.0, 0.0, 12.0));
        assert_eq!(short[..i.len()], i[..]);
        let (e, yo) = (strokes("Е", 0.0, 0.0, 12.0), strokes("Ё", 0.0, 0.0, 12.0));
        assert_eq!(yo[..e.len()], e[..]);
    }

    #[test]
    fn it_says_which_letters_it_has_so_a_text_type_can_letter_the_rest_from_plex() {
        for c in "aÁőŰčŘåŁøßé€!?Жй".chars() {
            assert!(has(c), "{c}");
        }
        for c in "ωΣ漢".chars() {
            assert!(!has(c), "{c}");
        }
        assert_eq!(strokes("Ő", 0.0, 0.0, 12.0).len(), strokes("O", 0.0, 0.0, 12.0).len() + 2, "a double acute");
    }

    #[test]
    fn the_ae_ligature_is_drawn_not_questioned() {
        let ae = strokes("Æ", 0.0, 0.0, 12.0);
        assert_ne!(ae, strokes("?", 0.0, 0.0, 12.0));
        assert_eq!(strokes("æ", 0.0, 0.0, 12.0), ae);
        assert!(ae.len() > strokes("E", 0.0, 0.0, 12.0).len(), "an A grown into an E has more strokes than an E");
        assert!(has('Æ'));
    }

    #[test]
    fn ae_is_an_a_grown_into_an_e_that_a_small_tag_still_reads() {
        let ae = strokes("Æ", 0.0, 0.0, 12.0);
        for other in ["A", "E"] {
            assert_ne!(strokes(other, 0.0, 0.0, 12.0), ae, "{other}");
        }
        // In its own cell, clear of the letters either side.
        assert!(ae.iter().flatten().all(|&(x, y)| (0.0..=8.0).contains(&x) && (-12.0..=0.0).contains(&y)), "{ae:?}");
        // One bar between the baseline and the top, the A's crossbar running on through the stem as the E's middle bar:
        // two bars at different heights run together into one smudge in a tag a few millimetres high.
        let segments: Vec<_> = ae.iter().flat_map(|line| line.windows(2).map(|w| (w[0], w[1]))).collect();
        let bars: Vec<_> = segments.iter().filter(|(a, b)| a.1 == b.1 && a.1 < 0.0 && a.1 > -12.0).collect();
        assert_eq!(bars.len(), 1, "{bars:?}");
        let stem = 4.0;
        assert!(bars.iter().all(|(a, b)| a.0.min(b.0) < stem && a.0.max(b.0) > stem), "{bars:?}");
        // Its left leg slopes up from the baseline as an A's does; the E's top and bottom bars run right from the stem.
        assert!(segments.iter().any(|&(a, b)| (a, b) == ((0.0, 0.0), (stem, -12.0))), "{segments:?}");
        for y in [0.0, -12.0] {
            assert!(segments.iter().any(|&(a, b)| a.1 == y && b.1 == y && a.0.min(b.0) == stem && a.0.max(b.0) == 8.0), "{y}");
        }
    }

    #[test]
    fn text_is_scaled_to_its_height_from_the_baseline() {
        let s = strokes("I", 10.0, 100.0, 24.0);
        let ys: Vec<f32> = s.iter().flatten().map(|p| p.1).collect();
        assert_eq!(ys.iter().copied().fold(f32::MAX, f32::min), 76.0);
        assert_eq!(ys.iter().copied().fold(f32::MIN, f32::max), 100.0);
        assert_eq!(width("II", 12.0), 19.0);
    }
}
