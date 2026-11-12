//! Co-Work (COWORK-PLAN.md): the interface changes the project only through `BimApp::exec` and `exec_batch`, so that a guest's
//! commands can go to the host instead of to a copy that would drift from it.

/// The interface's own sources, outside the tests: each file's path from `src` and its text.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if name == "tests" || name == "tests.rs" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if name.ends_with(".rs") {
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out
}

#[test]
fn the_interface_changes_the_project_only_through_exec() {
    let files = sources();
    assert!(files.len() > 50, "the sources were found: {}", files.len());
    let mut found = Vec::new();
    for (name, text) in &files {
        // A file's own tests, at its end, drive the session as a test does.
        let text = text.split("#[cfg(test)]\nmod tests {").next().unwrap_or(text);
        // Joined, so a call split over lines (`self.session` and then `.transaction(`) is still one.
        let joined: String = text.split_whitespace().collect();
        for call in ["self.session.execute(", "self.session.transaction(", "self.session.script("] {
            found.extend(std::iter::repeat_n(format!("{name}: {call}"), joined.matches(call).count()));
        }
    }
    // The funnel itself, once each.
    let funnel = ["lib.rs: self.session.execute(", "lib.rs: self.session.transaction("];
    for f in funnel {
        let at = found.iter().position(|x| x == f).unwrap_or_else(|| panic!("the funnel is there: {f}"));
        found.remove(at);
    }
    assert_eq!(found, Vec::<String>::new(), "call BimApp::exec or exec_batch instead");
}
