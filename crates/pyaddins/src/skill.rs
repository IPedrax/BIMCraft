//! The agent skill for writing PyAddins (`skills/bimcraft-pyaddins/SKILL.md`), built into the programs so an installed BIMCraft
//! has it: `bimcraft-cli skill install` and PyAddins > Python's Install Skill write it where Claude Code (`~/.claude/skills`)
//! and Codex (`~/.agents/skills`) read skills, and say how to give each the MCP server.

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

/// Writes the skill for every agent under `home`: the files written, by agent.
pub fn install(home: &Path) -> Result<Vec<(&'static str, PathBuf)>, String> {
    AGENTS
        .iter()
        .map(|(agent, dir)| {
            let folder = home.join(dir).join(NAME);
            std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
            let file = folder.join("SKILL.md");
            std::fs::write(&file, SKILL).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((*agent, file))
        })
        .collect()
}

/// Whether the skill is in place, as this version has it, for every agent under `home`.
pub fn installed(home: &Path) -> bool {
    AGENTS.iter().all(|(_, dir)| std::fs::read_to_string(home.join(dir).join(NAME).join("SKILL.md")).is_ok_and(|t| t == SKILL))
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
        // An old one is replaced.
        std::fs::write(&written[1].1, "old").unwrap();
        assert!(!installed(home.path()));
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
