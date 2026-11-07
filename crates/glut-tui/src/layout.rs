//! The panes kept for the next start (TUI9e): at the end, a window split into panes or
//! with agents in the background writes how it stood — each pane's session by its id, its role, an agent's name, a
//! worktree's branch, and the main pane's share of the width (T36f). `glut -c` offers it once. A file from before
//! T36f, with the shape of a split, comes back as the grid: its panes in their reading order, the first the main one.
//!
//! It is kept in the owner's `~/.glut/layouts/`, one file per project folder (named as Claude Code names its project
//! folders), never in the project: a repository can neither ship one nor point its `.glut` elsewhere for Glut to write
//! through. It holds names, never paths: a session is found by its id among the owner's own kept for that folder, a
//! worktree by its branch where Glut puts worktrees (`worktree::reopen`), and anything odd in the file makes it no
//! layout at all.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The folder below `~/.glut`.
const DIR: &str = "layouts";
/// A layout file larger than this is none.
const MAX_BYTES: u64 = 64 * 1024;
/// The most panes on the screen a layout holds, as many as Glut opens.
const MOST_SHOWN: usize = crate::panes::MOST;
/// The most agents in the background a layout holds: Glut sets them no limit (T36f), a file is bounded all the same.
const MOST_AGENTS: usize = 64;
/// The longest name, role, branch or session id a layout holds.
const MOST_NAME: usize = 128;

/// How the panes stood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    /// The panes on the screen in reading order — the main one, in the project's own folder, first —, then the
    /// agents in the background.
    pub panes: Vec<Kept>,
    /// The main pane's share of the width, in percent (T36f).
    #[serde(default = "share")]
    pub share: u16,
}

/// A layout from before T36f had no share: the main pane at its start.
fn share() -> u16 {
    crate::panes::SHARE
}

/// One pane or agent as it stood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// Its session's id.
    pub session: String,
    /// The role it played, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// An agent in the background: its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// A pane in a worktree: its branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The branch a take-over of that worktree merges into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
}

impl Layout {
    /// "3 panes, 1 agent".
    pub fn summary(&self) -> String {
        let agents = self.agents().count();
        let panes = self.panes.len() - agents;
        let texts = crate::texts::t();
        let panes = texts.panes_count.of(panes as u64);
        match agents {
            0 => panes,
            _ => tr!(
                two_counts,
                first = panes,
                second = texts.agents_count.of(agents as u64)
            ),
        }
    }

    /// What comes back, to read before saying yes: the roles, the worktrees' branches and the agents' names.
    pub fn details(&self) -> String {
        let said: Vec<String> = self
            .panes
            .iter()
            .map(|kept| {
                let mut parts = Vec::new();
                if let Some(agent) = &kept.agent {
                    parts.push(tr!(layout_agent, name = agent));
                }
                if let Some(role) = &kept.role {
                    parts.push(tr!(layout_role, role = role));
                }
                if let Some(branch) = &kept.branch {
                    parts.push(tr!(layout_branch, branch = branch));
                }
                if parts.is_empty() {
                    tr!(layout_a_pane).to_owned()
                } else {
                    parts.join(" ")
                }
            })
            .collect();
        said.join(" · ")
    }

    fn agents(&self) -> impl Iterator<Item = &Kept> {
        self.panes.iter().filter(|kept| kept.agent.is_some())
    }

    /// Whether the file says something Glut could have written: one to seven panes on the screen, agents only behind
    /// them and named as `/bg` names them, the first pane in the project's own folder, a share within its bounds,
    /// nothing twice, and plain names.
    fn sound(&self) -> bool {
        let plain = |text: &str| {
            !text.is_empty()
                && text.len() <= MOST_NAME
                && !text.starts_with('-')
                && text.chars().all(|c| c.is_ascii_graphic())
        };
        let names_plain = self.panes.iter().all(|kept| {
            plain(&kept.session)
                && kept
                    .session
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                && [&kept.role, &kept.branch, &kept.into]
                    .into_iter()
                    .flatten()
                    .all(|name| plain(name))
                && kept.agent.as_deref().is_none_or(crate::panes::is_name)
        });
        let shown = self
            .panes
            .iter()
            .take_while(|kept| kept.agent.is_none())
            .count();
        let agents_last = self.agents().count() == self.panes.len() - shown;
        let unique = |field: fn(&Kept) -> Option<&str>| {
            let mut seen: Vec<&str> = self.panes.iter().filter_map(field).collect();
            let all = seen.len();
            seen.sort_unstable();
            seen.dedup();
            seen.len() == all
        };
        names_plain
            && agents_last
            && (1..=MOST_SHOWN).contains(&shown)
            && self.panes.len() - shown <= MOST_AGENTS
            && self.panes[0].branch.is_none()
            && unique(|kept| Some(kept.session.as_str()))
            && unique(|kept| kept.branch.as_deref())
            && unique(|kept| kept.agent.as_deref())
            && (crate::panes::SHARE_LEAST..=crate::panes::SHARE_MOST).contains(&self.share)
    }
}

/// Where the layout of the folder `cwd` is kept, below the owner's `home`.
fn path(home: &Path, cwd: &Path) -> PathBuf {
    home.join(glut::providers::DIR)
        .join(DIR)
        .join(format!("{}.json", glut::project::project_key(cwd)))
}

/// The layout kept for `cwd`, if there is a sound one: read through no link, no more than [`MAX_BYTES`].
pub fn load(home: &Path, cwd: &Path) -> Option<Layout> {
    let file = glut::providers::open_no_follow(&path(home, cwd)).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_BYTES).read_to_string(&mut text).ok()?;
    let layout: Layout = serde_json::from_str(&text).ok()?;
    layout.sound().then_some(layout)
}

/// Keeps `layout` for `cwd`'s next start: into a new file of the owner's alone, never through a link, then moved
/// into place.
///
/// # Errors
///
/// A message when the folder or the file cannot be written.
pub fn save(home: &Path, cwd: &Path, layout: &Layout) -> Result<(), String> {
    let path = path(home, cwd);
    let dir = path.parent().unwrap_or(home);
    glut::providers::make_private_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let text = serde_json::to_string_pretty(layout).map_err(|err| err.to_string())?;
    glut::providers::write_replacing(dir, &path, text.as_bytes(), 0o600)
        .map_err(|err| format!("{}: {err}", path.display()))
}

/// The layout of `cwd` goes: it is not offered again.
pub fn forget(home: &Path, cwd: &Path) {
    let _ = std::fs::remove_file(path(home, cwd));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(session: &str, agent: Option<&str>) -> Kept {
        Kept {
            session: session.into(),
            role: None,
            agent: agent.map(str::to_owned),
            branch: None,
            into: None,
        }
    }

    fn pair() -> Layout {
        Layout {
            panes: vec![kept("s1", None), kept("s2", None)],
            share: 60,
        }
    }

    #[test]
    fn a_layout_comes_back_as_kept_and_says_what_it_holds() {
        let home = std::env::temp_dir().join(format!("glut-layout-{}", std::process::id()));
        let cwd = Path::new("/projects/glut");
        let mut layout = pair();
        layout.panes[1].branch = Some("glut/pane-2".into());
        layout.panes.push(kept("s3", Some("reviewer")));
        layout.panes[2].role = Some("reviewer".into());
        save(&home, cwd, &layout).unwrap();
        assert_eq!(load(&home, cwd), Some(layout.clone()));
        assert_eq!(
            load(&home, Path::new("/projects/other")),
            None,
            "a layout per folder"
        );
        assert_eq!(layout.summary(), "2 panes, 1 agent");
        assert_eq!(
            layout.details(),
            "a pane · on glut/pane-2 · agent reviewer as reviewer"
        );
        forget(&home, cwd);
        assert_eq!(load(&home, cwd), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_odd_layout_is_none() {
        let odd = |change: fn(&mut Layout)| {
            let mut layout = pair();
            change(&mut layout);
            !layout.sound()
        };
        assert!(pair().sound());
        assert!(odd(|layout| layout.share = 95), "a share beyond its bounds");
        assert!(
            odd(|layout| {
                for at in 0..7 {
                    layout.panes.insert(1, kept(&format!("p{at}"), None));
                }
            }),
            "more panes than Glut shows"
        );
        assert!(odd(|layout| layout.panes[1].session = "s2\n".into()));
        assert!(
            odd(|layout| layout.panes[1].session = "s1".into()),
            "one session twice"
        );
        assert!(
            odd(|layout| layout.panes[0].branch = Some("b".into())),
            "the first pane in the folder"
        );
        assert!(
            odd(|layout| layout.panes.insert(0, kept("s0", Some("first")))),
            "agents behind"
        );
        assert!(
            odd(|layout| layout.panes.push(kept("s9", Some("2")))),
            "an agent named like a pane"
        );
        assert!(odd(|layout| layout.panes[1].branch = Some("-D".into())));
        assert!(
            odd(|layout| {
                for at in 0..=MOST_AGENTS {
                    layout
                        .panes
                        .push(kept(&format!("a{at}"), Some(&format!("agent-{at}"))));
                }
            }),
            "more agents than a file holds"
        );
    }

    #[test]
    fn a_layout_from_before_the_grid_comes_back_with_the_main_pane_at_half() {
        let old = r#"{"panes":[{"session":"s1"},{"session":"s2"},{"session":"s3"}],
            "shape":{"split":{"stacked":false,"parts":[{"pane":0},
            {"split":{"stacked":true,"parts":[{"pane":1},{"pane":2}]}}]}}}"#;
        let layout: Layout = serde_json::from_str(old).unwrap();
        assert!(layout.sound());
        assert_eq!((layout.panes.len(), layout.share), (3, crate::panes::SHARE));
    }
}
