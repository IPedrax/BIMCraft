//! The agent skill for writing PyAddins (`skills/bimcraft-pyaddins/SKILL.md`), built into the programs so an installed BIMCraft
//! has it: `bimcraft-cli skill install` and PyAddins > Python's Install Skill write it where Claude Code (`~/.claude/skills`)
//! and Codex (`~/.agents/skills`) read skills, never over a file they didn't write, and say how to give each the MCP server.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The skill, as the repository has it.
pub const SKILL: &str = include_str!("../../../skills/bimcraft-pyaddins/SKILL.md");
/// Its folder's name.
pub const NAME: &str = "bimcraft-pyaddins";

/// The agents that read skills, and where under the user's home folder.
pub const AGENTS: [(&str, &str); 2] = [("Claude Code", ".claude/skills"), ("Codex", ".agents/skills")];

/// The user's home folder.
pub fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// The line the installer writes after the skill: with the skill's `name`, it is how a later install knows the file is its
/// own to replace.
pub const MARKER: &str =
    "<!-- Installed by BIMCraft (bimcraft-cli skill install, or PyAddins > Python): installing it again replaces this file. -->";

/// The skill as the installer writes it: [`SKILL`], then the [`MARKER`].
pub fn written() -> String {
    format!("{SKILL}\n{MARKER}\n")
}

/// Whether `text` is a skill the installer wrote: its frontmatter names [`NAME`] and it has the [`MARKER`] line.
fn ours(text: &str) -> bool {
    let mut lines = text.lines().map(str::trim_end);
    let name = format!("name: {NAME}");
    lines.next() == Some("---") && lines.by_ref().take_while(|l| *l != "---").any(|l| l == name) && lines.any(|l| l == MARKER)
}

/// What is at `file`, up to a size no skill reaches.
fn read(file: &Path) -> std::io::Result<String> {
    let mut text = String::new();
    std::fs::File::open(file)?.take(1 << 20).read_to_string(&mut text)?;
    Ok(text)
}

/// Writes the skill for every agent under `home`: the files written, by agent. A file it didn't write is the user's, or
/// another program's: it is never written over, and the error names it. Every place is looked at before any is written, so
/// a refusal leaves them all as they were.
pub fn install(home: &Path) -> Result<Vec<(&'static str, PathBuf)>, String> {
    let files: Vec<(&'static str, PathBuf)> =
        AGENTS.iter().map(|(agent, dir)| (*agent, home.join(dir).join(NAME).join("SKILL.md"))).collect();
    for (_, file) in &files {
        match read(file) {
            Ok(text) if ours(&text) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(format!(
                    "{} is not BIMCraft's skill, so it was left as it is: move it away to install BIMCraft's",
                    file.display()
                ));
            }
            Err(e) => return Err(format!("{}: {e}", file.display())),
        }
    }
    let text = written();
    files
        .into_iter()
        .map(|(agent, file)| {
            if let Some(folder) = file.parent() {
                std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
            }
            std::fs::write(&file, &text).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((agent, file))
        })
        .collect()
}

/// Whether the skill is in place, as this version writes it, for every agent under `home`.
pub fn installed(home: &Path) -> bool {
    let text = written();
    AGENTS.iter().all(|(_, dir)| read(&home.join(dir).join(NAME).join("SKILL.md")).is_ok_and(|t| t == text))
}

/// How each agent is given the MCP server, `cli` being the command line program: Claude Code's and Codex's commands.
pub fn mcp_commands(cli: &Path) -> Vec<String> {
    let cli = cli.display().to_string();
    let cli = if cli.contains(' ') { format!("\"{cli}\"") } else { cli };
    vec![format!("claude mcp add bimcraft -- {cli} mcp --connect"), format!("codex mcp add bimcraft -- {cli} mcp --connect")]
}

/// The command line program beside the running one (the desktop's `bimcraft`, or `bimcraft-cli` itself).
pub fn cli_beside() -> PathBuf {
    let name = if cfg!(windows) { "bimcraft-cli.exe" } else { "bimcraft-cli" };
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_is_written_where_each_agent_reads_skills() {
        let home = tempfile::tempdir().unwrap();
        assert!(!installed(home.path()));
        let written = install(home.path()).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(written[0].1, home.path().join(".claude/skills/bimcraft-pyaddins/SKILL.md"));
        assert_eq!(written[1].1, home.path().join(".agents/skills/bimcraft-pyaddins/SKILL.md"));
        assert!(installed(home.path()));
        assert!(std::fs::read_to_string(&written[0].1).unwrap().ends_with(&format!("\n{MARKER}\n")), "it says it wrote it");
        // An old one it wrote is replaced, one edited by hand on Windows too.
        std::fs::write(&written[1].1, format!("---\r\nname: bimcraft-pyaddins\r\ndescription: old\r\n---\r\nold\r\n{MARKER}\r\n")).unwrap();
        assert!(!installed(home.path()));
        install(home.path()).unwrap();
        assert!(installed(home.path()));
    }

    #[test]
    fn a_file_the_installer_did_not_write_is_never_written_over() {
        let home = tempfile::tempdir().unwrap();
        let written = install(home.path()).unwrap();
        let old = format!("---\nname: bimcraft-pyaddins\ndescription: old\n---\nold\n{MARKER}\n");
        // The user's own skill of that name, a file with no frontmatter, another skill with the marker pasted in.
        let other = format!("---\nname: other\n---\n{MARKER}\n");
        for theirs in ["---\nname: bimcraft-pyaddins\ndescription: mine\n---\nmine\n", "notes\n", other.as_str()] {
            std::fs::write(&written[0].1, &old).unwrap();
            std::fs::write(&written[1].1, theirs).unwrap();
            let e = install(home.path()).unwrap_err();
            assert!(e.starts_with(&format!("{} is not BIMCraft's skill", written[1].1.display())), "{e}");
            assert_eq!(std::fs::read_to_string(&written[1].1).unwrap(), theirs, "theirs is as it was");
            assert_eq!(std::fs::read_to_string(&written[0].1).unwrap(), old, "and nothing was written for the other agent");
            assert!(!installed(home.path()));
        }
        // Moved away, it installs.
        std::fs::remove_file(&written[1].1).unwrap();
        install(home.path()).unwrap();
        assert!(installed(home.path()));
    }

    #[test]
    fn the_skill_names_itself_and_says_when_to_use_it() {
        assert!(SKILL.starts_with("---\nname: bimcraft-pyaddins\ndescription: "));
        assert!(SKILL.contains("pyaddin_save") && SKILL.contains("pyaddin_run"));
    }

    #[test]
    fn the_mcp_commands_quote_a_path_with_spaces() {
        let c = mcp_commands(Path::new("/opt/BIM Craft/bimcraft-cli"));
        assert_eq!(c[0], "claude mcp add bimcraft -- \"/opt/BIM Craft/bimcraft-cli\" mcp --connect");
        assert!(c[1].starts_with("codex mcp add bimcraft -- "));
    }
}
