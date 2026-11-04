//! Folders a run works in beside its own (T60c): the owner's word for one session or run — `glut run --folder`,
//! `/folder add` — never a skill's frontmatter or a project's `.glut/`. Inside such a folder everything holds as in
//! the working folder: the file tools reach it by its full path (`~/Vault/notes.md`), commands write there through the
//! gate and the sandbox, a snapshot records it before every change, and the masks apply.
//!
//! One [`Folders`] is shared by everything of a run that needs it — the file tools, the shell, the gate, the
//! snapshots — so a folder added while a session runs counts everywhere at its next call.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Folders below the home folder no folder beside the run lies in: what runs as the owner outside every sandbox —
/// at the next login (`Library/LaunchAgents`), as an app loads (`Library`), as the shell or the editor starts
/// (`.config`, `.oh-my-zsh`), as a program on the `PATH` (`bin`, `.local/bin`), or as an app (`Applications`).
const HOME_RUNS: [&str; 13] = [
    "Library",
    ".config",
    ".oh-my-zsh",
    "bin",
    ".local/bin",
    "Applications",
    // The language managers' programs, on the `PATH` of a shell even where Glut was started without it.
    ".cargo/bin",
    ".bun",
    ".deno",
    ".volta",
    ".nvm",
    ".pyenv",
    ".rbenv",
];

/// Where below [`HOME_RUNS`] a folder beside the run may lie all the same: the cloud drives' own folders (iCloud
/// Drive, where an Obsidian vault often is, and the file providers'), which hold documents, never what runs.
const HOME_DOCUMENTS: [&str; 2] = ["Library/Mobile Documents", "Library/CloudStorage"];

/// Folders no folder beside the run lies in anywhere: the system's and the programs' own.
const SYSTEM_RUNS: [&str; 10] = [
    "/System",
    "/Library",
    "/Applications",
    "/usr",
    "/bin",
    "/sbin",
    "/opt",
    "/etc",
    "/private/etc",
    "/cores",
];

/// One folder beside the run's own — made only by [`check`] outside this crate, so every one of them was checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// The folder, resolved.
    pub(crate) real: PathBuf,
    /// How the model, the rules and the owner see it: `~/…` below the home folder, else the full path.
    pub(crate) shown: String,
    /// Its repository's git folders, which a command may write except what makes git run code (as the working
    /// folder's, [`crate::sandbox::Policy`]).
    pub(crate) git_dirs: Vec<PathBuf>,
}

impl Folder {
    /// The folder, resolved.
    pub fn real(&self) -> &Path {
        &self.real
    }

    /// How the model, the rules and the owner see it.
    pub fn shown(&self) -> &str {
        &self.shown
    }
}

/// Why [`Folders::add`] did not add a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAdded {
    /// It is one of them already.
    Known,
    /// It lies in one of them or holds one (shown): a command in the outer one could put a link where the inner one
    /// is, and the file tools would follow it.
    Overlaps(String),
}

/// The folders beside a run's own, shared by its tools (see the module).
#[derive(Debug, Clone, Default)]
pub struct Folders(Arc<RwLock<Vec<Folder>>>);

impl Folders {
    /// The folders as they stand now.
    pub fn list(&self) -> Vec<Folder> {
        self.0
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Whether the run has no folder beside its own.
    pub fn is_empty(&self) -> bool {
        self.0
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_empty()
    }

    /// Adds `folder`.
    ///
    /// # Errors
    ///
    /// [`NotAdded`]: the folder is there already, or lies in one of them or holds one.
    pub fn add(&self, folder: Folder) -> Result<(), NotAdded> {
        let mut folders = self.0.write().unwrap_or_else(|poison| poison.into_inner());
        if folders.iter().any(|known| known.real == folder.real) {
            return Err(NotAdded::Known);
        }
        if let Some(known) = folders.iter().find(|known| {
            known.real.starts_with(&folder.real) || folder.real.starts_with(&known.real)
        }) {
            return Err(NotAdded::Overlaps(known.shown.clone()));
        }
        folders.push(folder);
        Ok(())
    }

    /// Takes away the folder `named` — as shown, by its full path, or as `/folder add` took it (`~/…`, or relative
    /// to `cwd`); the one taken, if there was one.
    pub fn remove(&self, named: &str, cwd: &Path, home: &Path) -> Option<Folder> {
        let named = named.trim().trim_end_matches('/');
        let home = &crate::permissions::settings::user_home_beside(home);
        let real = typed_path(named, cwd, home).canonicalize().ok();
        let mut folders = self.0.write().unwrap_or_else(|poison| poison.into_inner());
        let at = folders.iter().position(|folder| {
            folder.shown == named
                || folder.real.to_string_lossy() == named
                || real.as_ref().is_some_and(|real| *real == folder.real)
        })?;
        Some(folders.remove(at))
    }

    /// The folder `path` lies in and the path inside it (empty for the folder itself), when `path` names one of
    /// them by its shown form or its full path — read by the names of its parts, `..` included; a link inside is
    /// left to the folder's guard. `None` for any other path.
    pub fn place(&self, path: &str) -> Option<(Folder, String)> {
        let folders = self.0.read().unwrap_or_else(|poison| poison.into_inner());
        place_in(&folders, path)
    }

    /// The path a command or a rule names, as the rules see it when it lies in one of the folders (`~/Vault/a.md`),
    /// else `None`.
    pub fn rule_path(&self, path: &str) -> Option<String> {
        self.place(path)
            .map(|(folder, inside)| joined(&folder.shown, &inside))
    }

    /// Where `path` (as the rules see it) is on disk, when it lies in one of the folders.
    pub fn full_path(&self, path: &str) -> Option<PathBuf> {
        self.place(path).map(|(folder, inside)| {
            if inside.is_empty() {
                folder.real
            } else {
                folder.real.join(inside)
            }
        })
    }

    /// The lines a prompt tells the model about the folders; empty without any.
    pub fn prompt(&self) -> String {
        let folders = self.list();
        if folders.is_empty() {
            return String::new();
        }
        let names: Vec<&str> = folders.iter().map(|folder| folder.shown.as_str()).collect();
        format!(
            "Extra folders (the owner gave them to this session; the file tools reach them by their full path as \
             written here, commands may write there too): {}",
            names.join(", ")
        )
    }
}

/// `inside` below `shown`, or `shown` alone.
fn joined(shown: &str, inside: &str) -> String {
    if inside.is_empty() {
        shown.to_owned()
    } else {
        format!("{shown}/{inside}")
    }
}

fn place_in(folders: &[Folder], path: &str) -> Option<(Folder, String)> {
    let path = path.trim();
    if !path.starts_with(['/', '~']) {
        return None;
    }
    let parts = lexical_parts(path)?;
    folders.iter().find_map(|folder| {
        let real = folder.real.to_string_lossy();
        [folder.shown.as_str(), real.as_ref()]
            .into_iter()
            .find_map(|form| {
                let base = lexical_parts(form)?;
                let rest = parts.strip_prefix(base.as_slice())?;
                Some((folder.clone(), rest.join("/")))
            })
    })
}

/// The parts of a full path (`/` or `~` first) by their names, `.` and `..` resolved; `None` when it climbs above
/// its start.
fn lexical_parts(path: &str) -> Option<Vec<String>> {
    let mut parts: Vec<String> = Vec::new();
    let (lead, rest) = match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => ("~", rest),
        Some(_) => return None,
        None => ("/", path),
    };
    parts.push(lead.to_owned());
    for component in Path::new(rest).components() {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            Component::ParentDir => {
                if parts.len() == 1 {
                    return None;
                }
                parts.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Some(parts)
}

/// What the owner typed as a path: `~` the home folder, a relative path below `cwd`.
fn typed_path(typed: &str, cwd: &Path, home: &Path) -> PathBuf {
    match typed.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => cwd.join(typed),
    }
}

/// `typed` as a folder beside the run in `cwd`, or why it cannot be one: it must exist and be a folder; refused are
/// `/`, the home folder and every folder above it, a folder holding the working folder or inside it, one inside or
/// holding a place no command reads ([`crate::sandbox::secrets`] and the owner's `deny_read`), one inside a protected
/// folder (`.git`, `.glut`, …), one reaching a place whose files run as programs ([`HOME_RUNS`] but the cloud drives,
/// [`SYSTEM_RUNS`], the `PATH`), and a name with a control character. `~/…` is below `home`, a relative path below
/// `cwd`.
///
/// # Errors
///
/// Why the folder is refused, for the owner to read.
pub fn check(
    typed: &str,
    cwd: &Path,
    home: &Path,
    deny_read: &[PathBuf],
) -> Result<Folder, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("name a folder".into());
    }
    // `~` and the places whose files run are the user's, also where Glut's own home is moved.
    let home = &crate::permissions::settings::user_home_beside(home);
    let real = typed_path(typed, cwd, home)
        .canonicalize()
        .map_err(|err| format!("{typed}: {err}"))?;
    if !real.is_dir() {
        return Err(format!("{typed} is no folder"));
    }
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if real == Path::new("/") || home.starts_with(&real) {
        return Err(format!(
            "{typed} holds the home folder: name a folder inside it"
        ));
    }
    if cwd.starts_with(&real) {
        return Err(format!("{typed} holds the working folder"));
    }
    // Said with where the run already reaches it: a Glut started in the home folder has every folder of it.
    if let Ok(inside) = real.strip_prefix(&cwd) {
        let working = match cwd.strip_prefix(&home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => cwd.display().to_string(),
        };
        return Err(format!(
            "{typed} lies in the working folder {working} already: the run reaches it as {}/…",
            inside.display()
        ));
    }
    refuse_reached(typed, &real, &home, deny_read)?;
    let shown = match real.strip_prefix(&home) {
        Ok(inside) => format!("~/{}", inside.to_string_lossy()),
        Err(_) => real.to_string_lossy().into_owned(),
    };
    // Its name goes into the model's prompt and onto the screen as it is.
    let hidden = |c: char| {
        c.is_control()
            || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
    };
    if shown.chars().any(hidden) {
        return Err(format!(
            "{typed}: a folder whose name holds a control, bidi or zero-width character"
        ));
    }
    Ok(Folder {
        git_dirs: crate::sandbox::git_dirs(&real),
        real,
        shown,
    })
}

/// `typed` as the folder an unattended run works in (T60b, a plan's own): it must exist and be a folder; refused are
/// `/`, the home folder and every folder above it, and — as for a folder beside a run — one inside or holding a place
/// no command reads, one inside a protected folder, and one reaching a place whose files run as programs: what its
/// commands write there would run later outside the sandbox, with nobody there to see it.
///
/// # Errors
///
/// Why the folder is refused, for the owner to read.
pub fn check_working(typed: &Path, home: &Path, deny_read: &[PathBuf]) -> Result<PathBuf, String> {
    let shown = typed.display().to_string();
    let home = &crate::permissions::settings::user_home_beside(home);
    let real = typed
        .canonicalize()
        .map_err(|err| format!("{shown}: {err}"))?;
    if !real.is_dir() {
        return Err(format!("{shown} is no folder"));
    }
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if real == Path::new("/") || home.starts_with(&real) {
        return Err(format!(
            "{shown} holds the home folder: name a folder inside it"
        ));
    }
    refuse_reached(&shown, &real, &home, deny_read)?;
    Ok(real)
}

/// Why `real` (typed as `typed`) reaches a place no run's folder may: a secret, a place whose files run as programs, a
/// protected folder.
fn refuse_reached(
    typed: &str,
    real: &Path,
    home: &Path,
    deny_read: &[PathBuf],
) -> Result<(), String> {
    let secrets = crate::sandbox::secrets(home, deny_read);
    if let Some(secret) = secrets
        .iter()
        .find(|secret| real.starts_with(secret) || secret.starts_with(real))
    {
        return Err(format!(
            "{typed} reaches {}, which no command reads",
            secret.display()
        ));
    }
    let resolved = |path: PathBuf| path.canonicalize().unwrap_or(path);
    // The places whose files run, below the user's home and below Glut's where that is moved: a plan's launchd and
    // systemd files lie in Glut's.
    let homes: Vec<PathBuf> = std::iter::once(home.to_path_buf())
        .chain(crate::sandbox::moved_glut_home(home))
        .collect();
    let runs: Vec<PathBuf> = homes
        .iter()
        .filter(|home| {
            !HOME_DOCUMENTS
                .iter()
                .any(|rel| real.starts_with(resolved(home.join(rel))))
        })
        .flat_map(|home| HOME_RUNS.iter().map(move |rel| home.join(rel)))
        .collect();
    let runs = runs
        .into_iter()
        .chain(SYSTEM_RUNS.iter().map(PathBuf::from))
        .map(resolved);
    // A folder on the `PATH`, or one holding a folder on it: what is written there runs as a command.
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|dir| dir.is_absolute())
        .map(resolved)
        .filter(|dir| real.starts_with(dir) || dir.starts_with(real));
    if let Some(place) = runs
        .filter(|place| real.starts_with(place) || place.starts_with(real))
        .chain(on_path)
        .next()
    {
        return Err(format!(
            "{typed} reaches {}, whose files run as programs: never a folder beside the run",
            place.display()
        ));
    }
    let protected = real.components().any(|component| match component {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            crate::tools::files::PROTECTED_DIRS
                .iter()
                .any(|dir| name.eq_ignore_ascii_case(dir))
        }
        _ => false,
    });
    if protected {
        return Err(format!(
            "{typed} lies in a protected folder (.git, .glut, …), which the run never writes"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for a test, resolved.
    fn temp(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "glut-folders-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn a_folder_is_checked_against_the_places_it_must_not_reach() {
        let home = temp("home");
        let cwd = home.join("work");
        let vault = home.join("Vault");
        for dir in [&cwd, &vault, &home.join(".ssh"), &home.join("Vault/.git")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(cwd.join("inner")).unwrap();
        for dir in [
            "Library/LaunchAgents",
            "Library/Mobile Documents/iCloud~md~obsidian/Notes",
            ".config/nvim",
            "Projects/app",
            "Vault/.glut",
        ] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        let folder = check("~/Vault", &cwd, &home, &[]).unwrap();
        assert_eq!(folder.shown, "~/Vault");
        assert_eq!(folder.real, vault);
        assert_eq!(check("../Vault/", &cwd, &home, &[]).unwrap().real, vault);
        // An iCloud Drive folder holds documents, the rest of the Library what runs.
        assert!(
            check(
                "~/Library/Mobile Documents/iCloud~md~obsidian/Notes",
                &cwd,
                &home,
                &[]
            )
            .is_ok()
        );
        for refused in [
            "/",
            "~",
            "~/.ssh",
            "~/Vault/.git",
            "~/Library/LaunchAgents",
            "~/Library",
            "~/.config/nvim",
            "~/Vault/.glut",
            "..",
            "/usr/local",
            "/Library",
            "/private/etc",
            "inner",
            "~/missing",
            "",
        ] {
            assert!(check(refused, &cwd, &home, &[]).is_err(), "{refused}");
        }
        // The owner's own secrets count like Glut's: inside one, or one inside.
        assert!(check("~/Vault", &cwd, &home, &[vault.join("keys")]).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_folder_is_taken_away_as_it_was_added() {
        let home = temp("away");
        let cwd = home.join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(home.join("Vault")).unwrap();
        let folders = Folders::default();
        folders
            .add(check("../Vault", &cwd, &home, &[]).unwrap())
            .unwrap();
        assert!(folders.remove("../Vault", &cwd, &home).is_some());
        assert!(folders.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_path_is_placed_by_its_shown_form_or_its_full_path() {
        let folders = Folders::default();
        let folder = |real: &str| Folder {
            real: PathBuf::from(real),
            shown: real.replace("/Users/o", "~"),
            git_dirs: Vec::new(),
        };
        assert_eq!(folders.add(folder("/Users/o/Vault")), Ok(()));
        assert_eq!(folders.add(folder("/Users/o/Vault")), Err(NotAdded::Known));
        for nested in ["/Users/o/Vault/sub", "/Users/o"] {
            assert_eq!(
                folders.add(folder(nested)),
                Err(NotAdded::Overlaps("~/Vault".into())),
                "{nested}"
            );
        }
        let place = |path: &str| folders.place(path).map(|(_, inside)| inside);
        assert_eq!(place("~/Vault/notes/a.md").as_deref(), Some("notes/a.md"));
        assert_eq!(place("/Users/o/Vault/./a.md").as_deref(), Some("a.md"));
        assert_eq!(place("~/Vault").as_deref(), Some(""));
        assert_eq!(
            folders.rule_path("/Users/o/Vault/b").as_deref(),
            Some("~/Vault/b")
        );
        for outside in [
            "~/Vault/../.ssh/id",
            "~/Vaults/a",
            "Vault/a",
            "~x/Vault",
            "/Users/o",
        ] {
            assert_eq!(place(outside), None, "{outside}");
        }
        assert!(folders.prompt().contains("~/Vault"));
        assert_eq!(
            folders
                .remove("~/Vault/", Path::new("/w"), Path::new("/Users/o"))
                .map(|folder| folder.shown)
                .as_deref(),
            Some("~/Vault")
        );
        assert!(folders.is_empty() && folders.prompt().is_empty());
    }
}
