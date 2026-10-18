//! Which project's MCP server the owner approved (`docs/plans/harness.md`, HS5, M2): `~/.axh/mcp-approved.json`.
//!
//! A project's server starts a process of its own and sends data out, so trusting the folder (P5) is not enough:
//! each server is approved on its own, keyed by the project's path, its name and the SHA-256 of its config — a
//! changed command, argument, env or URL asks again. The owner's own servers in `~/.axh/mcp.json` need no
//! approval; they are their own.
//!
//! Where nobody can answer (the `axh` binary, a pipe) an unapproved project server is left out and the fact goes
//! to stderr: the run goes on without it rather than stopping, and never starts a process nobody said yes to.

use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::config::{Origin, ServerEntry};
use crate::error::HarnessError;
use crate::permissions::{printable, settings::USER_DIR};

/// The approvals file, below the owner's settings folder.
const APPROVALS_FILE: &str = "mcp-approved.json";

/// What the owner approved once (M2), kept in `~/.axh/mcp-approved.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Approvals {
    #[serde(default)]
    approved: Vec<Approval>,
}

/// One approval: this project, this server, this very config.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Approval {
    project: PathBuf,
    name: String,
    /// What [`ServerEntry::config_hash`] said when the owner said yes.
    hash: String,
}

impl Approvals {
    fn path(home: &Path) -> PathBuf {
        home.join(USER_DIR).join(APPROVALS_FILE)
    }

    /// The approvals so far; none when the file is missing.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] for a file that exists but cannot be read: approving one server must not drop
    /// the approvals already given.
    pub fn load(home: &Path) -> Result<Self, HarnessError> {
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|err| fail(err.to_string())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Approvals::default()),
            Err(err) => Err(fail(err.to_string())),
        }
    }

    /// Whether the owner approved this server in this project, with this very config (M2).
    pub fn is_approved(&self, project: &Path, name: &str, hash: &str) -> bool {
        self.approved.iter().any(|approval| {
            approval.project == project && approval.name == name && approval.hash == hash
        })
    }

    /// Records the owner's yes for this server's config, from now on.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the file cannot be written.
    pub fn approve(
        &mut self,
        home: &Path,
        project: &Path,
        name: &str,
        hash: &str,
    ) -> Result<(), HarnessError> {
        use std::os::unix::fs::DirBuilderExt;
        if self.is_approved(project, name, hash) {
            return Ok(());
        }
        self.approved.push(Approval {
            project: project.to_path_buf(),
            name: name.to_owned(),
            hash: hash.to_owned(),
        });
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|err| fail(err.to_string()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|err| fail(err.to_string()))?;
        // Written beside and renamed over: a crash halfway leaves the old list, never half a list. Readable by
        // the owner only — an approval decides what starts a process.
        let temp = path.with_extension("json.tmp");
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temp)
                .map_err(|err| fail(err.to_string()))?;
            file.write_all(text.as_bytes())
                .map_err(|err| fail(err.to_string()))?;
        }
        std::fs::rename(&temp, &path).map_err(|err| fail(err.to_string()))
    }
}

/// The servers a run may start (M2): the owner's own, and a project's where the owner approved this very
/// config. Where there is a terminal (`can_ask`) an unapproved one is shown — its command line or URL, through
/// [`printable`] — and the question read from it; without one it is left out and said so on stderr. Either way
/// the run goes on.
///
/// # Errors
///
/// [`HarnessError::Settings`] when the approvals file cannot be read or written.
pub fn permitted(
    entries: Vec<ServerEntry>,
    home: &Path,
    project: &Path,
    can_ask: bool,
) -> Result<Vec<ServerEntry>, HarnessError> {
    let mut approvals = Approvals::load(home)?;
    let mut kept = Vec::new();
    for entry in entries {
        match entry.origin {
            Origin::Owner => kept.push(entry),
            Origin::Project => {
                let hash = entry.config_hash()?;
                if approvals.is_approved(project, &entry.name, &hash) {
                    kept.push(entry);
                    continue;
                }
                if can_ask && ask(&entry, project) {
                    approvals.approve(home, project, &entry.name, &hash)?;
                    kept.push(entry);
                } else {
                    eprintln!(
                        "mcp: the project server {} was not approved, and is left out",
                        printable(&entry.name)
                    );
                }
            }
        }
    }
    Ok(kept)
}

/// The question for one project server (M2): what it is, then one line from the terminal. Anything but yes is
/// no — a terminal that cannot be read decides against it, never for it.
fn ask(entry: &ServerEntry, project: &Path) -> bool {
    eprintln!(
        "{} declares the MCP server {}:",
        printable(&project.display().to_string()),
        printable(&entry.name)
    );
    for line in entry.describe().lines() {
        eprintln!("  {}", printable(line));
    }
    eprint!("Start it? [y/N]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(_) => matches!(
            line.trim().to_lowercase().as_str(),
            "y" | "j" | "yes" | "ja"
        ),
        Err(_) => false,
    }
}

/// Whether someone is there to be asked: the run's own terminal, which is where the trust question is asked
/// too (P5). Without one, [`permitted`] skips an unapproved project server instead of asking.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::config::ServerConfig;
    use std::collections::BTreeMap;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-mcp-approval-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project_server(name: &str, command: &str, args: &[&str]) -> ServerEntry {
        ServerEntry {
            name: name.into(),
            origin: Origin::Project,
            config: ServerConfig::Stdio {
                command: command.into(),
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                env: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn an_unapproved_project_server_is_left_out_and_one_approved_runs() {
        let (home, project) = (temp("home"), temp("project"));
        let server = project_server("mail", "uvx", &["mcp-mail"]);
        let hash = server.config_hash().unwrap();
        assert!(
            !Approvals::load(&home)
                .unwrap()
                .is_approved(&project, "mail", &hash),
            "nothing approved yet"
        );
        let kept = permitted(vec![server.clone()], &home, &project, false).unwrap();
        assert!(kept.is_empty(), "no terminal, no approval, no server");

        let mut approvals = Approvals::load(&home).unwrap();
        approvals.approve(&home, &project, "mail", &hash).unwrap();
        let kept = permitted(vec![server.clone()], &home, &project, false).unwrap();
        assert_eq!(kept.len(), 1, "approved runs");
    }

    #[test]
    fn changed_arguments_are_not_approved_any_more() {
        let (home, project) = (temp("changed-home"), temp("changed-project"));
        let server = project_server("mail", "uvx", &["mcp-mail"]);
        let mut approvals = Approvals::load(&home).unwrap();
        approvals
            .approve(&home, &project, "mail", &server.config_hash().unwrap())
            .unwrap();
        let changed = project_server("mail", "uvx", &["mcp-mail", "--read-only"]);
        assert!(!Approvals::load(&home).unwrap().is_approved(
            &project,
            "mail",
            &changed.config_hash().unwrap()
        ));
        // The approval of the old config is still there: it was not overwritten.
        assert!(Approvals::load(&home).unwrap().is_approved(
            &project,
            "mail",
            &server.config_hash().unwrap()
        ));
        // Another project is a different server to approve, and so is another name.
        let other = temp("other");
        assert!(!Approvals::load(&home).unwrap().is_approved(
            &other,
            "mail",
            &server.config_hash().unwrap()
        ));
        assert!(!Approvals::load(&home).unwrap().is_approved(
            &project,
            "other",
            &server.config_hash().unwrap()
        ));
    }

    #[test]
    fn the_approvals_file_is_the_owners_alone() {
        let home = temp("mode");
        let project = temp("mode-project");
        let server = project_server("mail", "uvx", &["mcp-mail"]);
        let hash = server.config_hash().unwrap();
        Approvals::load(&home)
            .unwrap()
            .approve(&home, &project, "mail", &hash)
            .unwrap();
        let path = Approvals::path(&home);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "readable by the owner only");
        assert!(!path.with_extension("json.tmp").exists(), "renamed over");
        // A broken approvals file is not overwritten: it would drop the approvals it holds.
        std::fs::write(&path, "{ not json").unwrap();
        assert!(Approvals::load(&home).is_err());
    }

    #[test]
    fn the_owner_needs_no_approval_for_his_own_servers() {
        let (home, project) = (temp("owner-home"), temp("owner-project"));
        let own = ServerEntry {
            name: "mail".into(),
            origin: Origin::Owner,
            config: ServerConfig::Http {
                url: "https://x.example".into(),
                headers: BTreeMap::new(),
            },
        };
        let kept = permitted(vec![own], &home, &project, false).unwrap();
        assert_eq!(kept.len(), 1);
        assert!(Approvals::load(&home).unwrap().approved.is_empty());
    }
}
