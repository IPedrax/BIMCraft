//! A project file's JSON text: indented as `serde_json`'s pretty printer does, so a file still reads and diffs line by line,
//! but a list of numbers or strings, or a list of such lists (a mesh's points and faces, a path's vertices), on one line.
//! With a number a line, a meshed model's file was more than four times its size (295 MB for one Revit export).

/// `compact`, JSON text as `serde_json::to_string` writes it (no space outside strings), laid out as a project file holds
/// it. Keys keep their order; the text means the same.
pub fn pretty(compact: &str) -> String {
    let b = compact.as_bytes();
    let mut out = String::with_capacity(compact.len() + compact.len() / 4);
    let mut i = 0;
    value(b, &mut i, &mut out, 0);
    out
}

/// The index just past the string that starts at `b[i]` (a `"`).
fn string_end(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// The index just past the list or object that starts at `b[i]`, and whether a list there holds no object and nests no
/// deeper than lists of lists of scalars.
fn scan(b: &[u8], start: usize) -> (usize, bool) {
    let (mut i, mut depth, mut deepest, mut object) = (start, 0usize, 0usize, false);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i = string_end(b, i);
                continue;
            }
            b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b'{' => {
                object = true;
                depth += 1;
            }
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (i + 1, !object && deepest <= 2);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (b.len(), false)
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// Copies `b[from..to]`, which is whole UTF-8 (it starts and ends on ASCII punctuation or quotes).
fn copy(out: &mut String, b: &[u8], from: usize, to: usize) {
    out.push_str(std::str::from_utf8(&b[from..to]).unwrap_or_default());
}

fn value(b: &[u8], i: &mut usize, out: &mut String, depth: usize) {
    let Some(&c) = b.get(*i) else { return };
    match c {
        b'{' if b.get(*i + 1) == Some(&b'}') => {
            out.push_str("{}");
            *i += 2;
        }
        b'{' => {
            *i += 1;
            out.push('{');
            let mut first = true;
            while *i < b.len() && b[*i] != b'}' {
                if b[*i] == b',' {
                    *i += 1;
                }
                out.push_str(if first { "\n" } else { ",\n" });
                first = false;
                indent(out, depth + 1);
                let end = string_end(b, *i);
                copy(out, b, *i, end);
                out.push_str(": ");
                *i = end + 1; // the key and its `:`
                value(b, i, out, depth + 1);
            }
            *i += 1;
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        b'[' => {
            let (end, inline) = scan(b, *i);
            if inline || b.get(*i + 1) == Some(&b']') {
                copy(out, b, *i, end);
                *i = end;
                return;
            }
            *i += 1;
            out.push('[');
            let mut first = true;
            while *i < b.len() && b[*i] != b']' {
                if b[*i] == b',' {
                    *i += 1;
                }
                out.push_str(if first { "\n" } else { ",\n" });
                first = false;
                indent(out, depth + 1);
                value(b, i, out, depth + 1);
            }
            *i += 1;
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        b'"' => {
            let end = string_end(b, *i);
            copy(out, b, *i, end);
            *i = end;
        }
        _ => {
            let start = *i;
            while *i < b.len() && !matches!(b[*i], b',' | b']' | b'}') {
                *i += 1;
            }
            copy(out, b, start, *i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn laid_out(v: &Value) -> String {
        pretty(&serde_json::to_string(v).unwrap())
    }

    #[test]
    fn objects_and_lists_of_objects_are_indented_and_lists_of_numbers_stay_on_a_line() {
        let v = json!({"name": "a \"b\" [x], {y}", "points": [[0.0, 1.5], [2, 3]], "ids": [1, 2, 3], "walls": [{"id": 1}], "empty": [], "none": {}});
        let text = laid_out(&v);
        assert_eq!(
            text,
            "{\n  \"empty\": [],\n  \"ids\": [1,2,3],\n  \"name\": \"a \\\"b\\\" [x], {y}\",\n  \"none\": {},\n  \"points\": [[0.0,1.5],[2,3]],\n  \"walls\": [\n    {\n      \"id\": 1\n    }\n  ]\n}"
        );
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), v, "it reads back the same");
    }

    #[test]
    fn deeper_or_mixed_lists_are_indented_and_read_back_the_same() {
        for v in [json!([[1, [2]], [[3]]]), json!([1, {"a": [1, 2]}, "é\\n"]), json!({"k": [[[1, 2], [3, 4]]]}), json!(null), json!("x")] {
            assert_eq!(serde_json::from_str::<Value>(&laid_out(&v)).unwrap(), v, "{v}");
        }
        assert!(laid_out(&json!([[1, [2]]])).contains('\n'));
    }

    #[test]
    fn the_order_of_keys_is_kept() {
        let text = pretty(r#"{"z":1,"a":{"y":[1,2],"b":2}}"#);
        assert_eq!(text, "{\n  \"z\": 1,\n  \"a\": {\n    \"y\": [1,2],\n    \"b\": 2\n  }\n}");
    }
}
