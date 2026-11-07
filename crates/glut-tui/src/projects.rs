//! The projects (T44): the folders Glut was started in, as the sessions kept for them say —
//! each with when it was last worked in and the panes kept for it. `/projects` lists them, the welcome shows the
//! newest; one picked is opened as `glut -c` there would (Glut starts anew in its folder, [`crate::screen`]).
//!
//! The owner's picks — the ones pinned to the top, the ones hidden — are `~/.glut/projects.json`, written by Glut
//! alone. Glut's own worktrees are never a project: their panes belong to the folder they were split from, whose
//! layout keeps them.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::store::Session;

/// The owner's picks, below `~/.glut`.
const FILE: &str = "projects.json";
/// A picks file larger than this is none.
const MAX_BYTES: u64 = 256 * 1024;
/// The most folders pinned or hidden: more is no file Glut wrote.
const MOST_PICKS: usize = 1_000;
/// Where Glut puts its worktrees, below the owner's home (as `crate::worktree` does).
const WORKTREES: &str = ".glut/worktrees";
/// How many projects the welcome shows.
pub const WELCOMED: usize = 5;

/// A folder Glut worked in.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub folder: PathBuf,
    /// When a session of it was last saved; `None` for a pinned folder no session was kept for yet.
    pub last: Option<DateTime<Utc>>,
    /// How many sessions are kept for it.
    pub sessions: usize,
    /// The panes kept for its next start ("2 panes, 1 agent"), if any.
    pub layout: Option<String>,
    pub pinned: bool,
}

impl Project {
    /// Its name: the folder's own.
    pub fn name(&self) -> String {
        self.folder.file_name().map_or_else(
            || self.folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// The owner's picks: folders pinned to the top, and folders hidden — each hidden one until a session of it is saved
/// after it was hidden.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Picks {
    #[serde(default)]
    pub pinned: Vec<PathBuf>,
    #[serde(default)]
    pub hidden: Vec<Hidden>,
}

/// A folder hidden from the list, and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hidden {
    pub folder: PathBuf,
    pub at: DateTime<Utc>,
}

fn path(home: &Path) -> PathBuf {
    home.join(glut::providers::DIR).join(FILE)
}

impl Picks {
    /// The owner's picks; none where the file is missing or not what Glut writes.
    pub fn load(home: &Path) -> Picks {
        Picks::read(home).unwrap_or_default()
    }

    /// The owner's picks, none where there is no file yet.
    ///
    /// # Errors
    ///
    /// A message when there is a file but not one Glut wrote: no plain file, too large, read through a link, or not
    /// its shape — a change of the picks must not write over it.
    pub fn read(home: &Path) -> Result<Picks, String> {
        let file = path(home);
        let odd = || tr!(not_glut_file, path = file.display());
        match std::fs::symlink_metadata(&file) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Picks::default()),
            // A plain file only: a FIFO would hold the list up on its opening.
            Ok(meta) if meta.is_file() && meta.len() <= MAX_BYTES => {}
            _ => return Err(odd()),
        }
        let mut text = String::new();
        glut::providers::open_no_follow(&file)
            .and_then(|opened| opened.take(MAX_BYTES).read_to_string(&mut text))
            .map_err(|_| odd())?;
        serde_json::from_str::<Picks>(&text)
            .ok()
            .filter(|picks| picks.pinned.len() <= MOST_PICKS && picks.hidden.len() <= MOST_PICKS)
            .ok_or_else(odd)
    }

    /// Writes the picks: into a new file of the owner's alone, moved over the old one.
    ///
    /// # Errors
    ///
    /// A message when the file cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), String> {
        let dir = home.join(glut::providers::DIR);
        glut::providers::make_private_dir(&dir)
            .map_err(|err| format!("{}: {err}", dir.display()))?;
        let text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        let file = path(home);
        glut::providers::write_replacing(&dir, &file, text.as_bytes(), 0o600)
            .map_err(|err| format!("{}: {err}", file.display()))
    }

    /// `folder` pinned to the top, or no longer; returns whether it is pinned now. A hidden folder pinned shows again.
    pub fn toggle_pin(&mut self, folder: &Path) -> bool {
        if let Some(at) = self.pinned.iter().position(|pinned| pinned == folder) {
            self.pinned.remove(at);
            return false;
        }
        self.hidden.retain(|hidden| hidden.folder != folder);
        self.pinned.push(folder.to_path_buf());
        true
    }

    /// `folder` hidden from the list from `now` on — unpinned with it.
    pub fn hide(&mut self, folder: &Path, now: DateTime<Utc>) {
        self.pinned.retain(|pinned| pinned != folder);
        self.hidden.retain(|hidden| hidden.folder != folder);
        self.hidden.push(Hidden {
            folder: folder.to_path_buf(),
            at: now,
        });
    }
}

/// Whether `folder` is one of Glut's own worktrees, below the owner's `home`.
fn is_worktree(home: &Path, folder: &Path) -> bool {
    folder.starts_with(home.join(WORKTREES))
}

/// The projects: the folders sessions were kept for that still are folders — no worktree of Glut's, none the owner
/// hid since its last session — and the pinned ones; pinned first, then the newest.
pub fn list(home: &Path) -> Vec<Project> {
    let picks = Picks::load(home);
    let mut projects: Vec<Project> = Vec::new();
    for head in Session::heads(home) {
        match projects
            .iter_mut()
            .find(|project| project.folder == head.cwd)
        {
            Some(project) => {
                project.sessions += 1;
                project.last = project.last.max(Some(head.updated));
            }
            None => projects.push(Project {
                folder: head.cwd,
                last: Some(head.updated),
                sessions: 1,
                layout: None,
                pinned: false,
            }),
        }
    }
    for pinned in &picks.pinned {
        if !projects.iter().any(|project| &project.folder == pinned) {
            projects.push(Project {
                folder: pinned.clone(),
                last: None,
                sessions: 0,
                layout: None,
                pinned: true,
            });
        }
    }
    // Folders as Glut writes them — whole paths: a start there names the folder by it.
    projects.retain(|project| {
        project.folder.is_absolute()
            && !is_worktree(home, &project.folder)
            && project.folder.is_dir()
            && !picks.hidden.iter().any(|hidden| {
                hidden.folder == project.folder && project.last.is_none_or(|last| last <= hidden.at)
            })
    });
    for project in &mut projects {
        project.pinned = picks.pinned.contains(&project.folder);
        project.layout = crate::layout::load(home, &project.folder).map(|layout| layout.summary());
    }
    projects.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last.cmp(&a.last)));
    projects
}

/// The folder `typed` names, as `/projects <path>` opens it: `~` the owner's `home`, a relative path from `cwd`. It
/// must be a folder, and none of Glut's own worktrees.
///
/// # Errors
///
/// A message naming what `typed` is instead.
pub fn folder_of(typed: &str, home: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err(tr!(projects_no_folder).into());
    }
    let named = match typed.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => cwd.join(typed),
    };
    let shown = crate::transcript::clean(typed);
    let folder = named
        .canonicalize()
        .map_err(|_| tr!(no_such_folder, folder = shown))?;
    if !folder.is_dir() {
        return Err(tr!(not_a_folder, folder = shown));
    }
    if is_worktree(home, &folder) {
        return Err(tr!(is_glut_worktree, folder = shown));
    }
    Ok(folder)
}

/// `folder` as the list writes it: the owner's home as `~`.
pub fn shown_path(folder: &Path, home: &Path) -> String {
    match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => folder.display().to_string(),
    }
}

/// How long ago `then` was, in a few words.
pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - then).num_minutes().max(0);
    let texts = crate::texts::t();
    match minutes {
        0 => tr!(just_now).into(),
        1..=59 => tr!(minutes_ago, n = minutes),
        60..=1439 => tr!(hours_ago, n = minutes / 60),
        1440..=2879 => tr!(yesterday).into(),
        _ => texts.days_ago.of((minutes / 1440) as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("glut-projects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home.canonicalize().unwrap()
    }

    /// A session of `cwd` saved `minutes_ago` (negative: later than now), written as `save` writes it.
    fn kept(home: &Path, cwd: &Path, minutes_ago: i64) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let mut session = Session::new(cwd, "p", "m");
        session.id = format!("t{}", NEXT.fetch_add(1, Ordering::Relaxed));
        session.updated = Utc::now() - chrono::Duration::minutes(minutes_ago);
        let dir = home.join(".glut/sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{}.json", session.id)),
            serde_json::to_string(&session).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn the_folders_of_the_sessions_newest_first_pinned_on_top_hidden_until_worked_in_again() {
        let home = home("list");
        let (a, b, gone) = (home.join("a"), home.join("b"), home.join("gone"));
        let tree = home.join(WORKTREES).join("a-glut-pane-2");
        for folder in [&a, &b, &tree] {
            std::fs::create_dir_all(folder).unwrap();
        }
        kept(&home, &a, 60);
        kept(&home, &a, 5);
        kept(&home, &b, 30);
        kept(&home, &gone, 1);
        kept(&home, &tree, 1);
        let listed = list(&home);
        let names: Vec<String> = listed.iter().map(Project::name).collect();
        // Newest first; a folder that is gone and a worktree of Glut's are no project.
        assert_eq!(names, ["a", "b"]);
        assert_eq!(listed[0].sessions, 2);
        let mut picks = Picks::default();
        assert!(picks.toggle_pin(&b));
        picks.save(&home).unwrap();
        assert_eq!(list(&home)[0].name(), "b");
        assert!(list(&home)[0].pinned);
        // Hidden — and unpinned — until a session of it is saved after.
        picks.hide(&b, Utc::now());
        picks.save(&home).unwrap();
        assert_eq!(Picks::load(&home).pinned, Vec::<PathBuf>::new());
        assert_eq!(list(&home).len(), 1);
        kept(&home, &b, -1);
        assert_eq!(list(&home).len(), 2);
        // A file Glut did not write is left as it is: no change of the picks writes over it.
        std::fs::write(home.join(".glut").join(FILE), "not json").unwrap();
        assert!(Picks::read(&home).unwrap_err().contains("left as it is"));
        assert_eq!(Picks::load(&home), Picks::default());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_typed_folder_is_a_folder_and_no_worktree() {
        let home = home("typed");
        let project = home.join("Development").join("Glut");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        std::fs::create_dir_all(home.join(WORKTREES).join("x")).unwrap();
        std::fs::write(home.join("file.txt"), "").unwrap();
        assert_eq!(
            folder_of("~/Development/Glut", &home, &home),
            Ok(project.clone())
        );
        assert_eq!(
            folder_of("Glut", &home.join("Development"), &home.join("Development")),
            Ok(project)
        );
        assert!(
            folder_of("~/nowhere", &home, &home)
                .unwrap_err()
                .contains("no such folder")
        );
        assert!(
            folder_of("~/file.txt", &home, &home)
                .unwrap_err()
                .contains("not a folder")
        );
        assert!(
            folder_of("~/.glut/worktrees/x", &home, &home)
                .unwrap_err()
                .contains("worktrees")
        );
        assert_eq!(shown_path(&home.join("a/b"), &home), "~/a/b");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn how_long_ago_in_a_few_words() {
        let now = Utc::now();
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now - chrono::Duration::minutes(5), now), "5 min ago");
        assert_eq!(ago(now - chrono::Duration::hours(3), now), "3 h ago");
        assert_eq!(ago(now - chrono::Duration::hours(30), now), "yesterday");
        assert_eq!(ago(now - chrono::Duration::days(4), now), "4 days ago");
    }
}
