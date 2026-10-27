//! What the owner sets in Glut itself (T29): `~/.glut/prefs.json`, written by Glut's commands (`/editor`) and readable
//! by hand. Never the owner's hand-written `settings.json`, which a command does not rewrite.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file below the home folder's `.glut`.
const FILE: &str = "prefs.json";
/// A prefs file larger than this is none.
const MAX_BYTES: u64 = 64 * 1024;
/// The editors `/editor` offers when they are on the `PATH`, with how each opens a file at a line.
pub const EDITORS: [(&str, &str); 10] = [
    ("nvim", "nvim +{line} {file}"),
    ("vim", "vim +{line} {file}"),
    ("hx", "hx {file}:{line}"),
    ("micro", "micro {file}:{line}"),
    ("nano", "nano +{line} {file}"),
    ("emacs", "emacs +{line} {file}"),
    // The windowed editors return at once unless told to wait: Glut would read the file before anything was written.
    ("zed", "zed --wait {file}:{line}"),
    ("code", "code --wait -g {file}:{line}"),
    ("cursor", "cursor --wait -g {file}:{line}"),
    ("subl", "subl --wait {file}:{line}"),
];

/// The prefs, with whatever else a later Glut or the owner put in the file kept as it was.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// How a file opens at a line: `nvim +{line} {file}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// How much of the model's thinking the stream shows (T8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<crate::transcript::Thinking>,
    /// Whether the side panel is shown (T28).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<bool>,
    /// Whether the input is edited the vi way (TUI3c, `/vim`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vi: Option<bool>,
    /// How the footer's segments are joined (T39, `/footer`); `None`: guessed from the terminal.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::footer::lenient"
    )]
    pub footer: Option<crate::footer::Separators>,
    /// How a resting session keeps its cache (T36e, `/cache`); `None`: pinging.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::warm::lenient"
    )]
    pub cache: Option<crate::warm::CacheMode>,
    /// Whether the first start's welcome was seen (TUI9c): it goes with the first message or command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcomed: Option<bool>,
    /// How images are drawn (T46, `/images`); `None`: inline where the terminal can, half blocks elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<crate::pictures::Drawing>,
    /// How links are shown (TUI9d, `/links`); `None`: clickable where the terminal speaks OSC 8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<crate::links::Links>,
    /// When the owner is told a turn ended or asks (T53, `/notify`); `None`: `notify.when` of the settings, else on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify: Option<glut::permissions::settings::NotifyWhen>,
    /// How many seconds a turn must have run before its end is told (T53, `/notify after`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_after: Option<u32>,
    /// When Glut last looked for the plans' runs (T60b): the next start tells what ran since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_seen: Option<chrono::DateTime<chrono::Utc>>,
    /// The most rounds a loop runs (T60a, `/loop rounds`); `None`: `loop.rounds` of the settings, else 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_rounds: Option<u32>,
    /// The most a loop's session may cost in USD (T60a, `/loop budget`); `None`: `loop.maxUsd` of the settings, else 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_max_usd: Option<f64>,
    /// Whether tiles show at the window's top right (T69, `/notify tiles`); `None`: they do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiles: Option<bool>,
    /// How much the screen moves (TUI9b, `/motion`); `None`: fully.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<crate::motion::Motion>,
    /// Whether the reader mode is on (TUI9b, `/reader`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader: Option<bool>,
    /// The colours: `glut` or an app theme's name (TUI3c, `/theme`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// The language Glut speaks to the owner in, by its code (T48, `/language`); none: the system's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The folders whose owner said no to moving `CLAUDE.md` into `AGENTS.md` (T35, `/init`): not asked again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep_claude: Vec<String>,
    /// The guides `/init` checked (T42), by path, with the size each had at the end of that turn: a guide too large
    /// is pointed out again only once it grew beyond it.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub guide_checked: std::collections::BTreeMap<String, u64>,
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

impl Prefs {
    /// Whether Glut ever kept a preference here: no file is a first start (TUI9c).
    pub fn kept(home: &Path) -> bool {
        Self::dir(home).join(FILE).exists()
    }

    fn dir(home: &Path) -> PathBuf {
        home.join(glut::providers::DIR)
    }

    /// The prefs; a missing file has none.
    ///
    /// # Errors
    ///
    /// A message for a file that cannot be read or is no prefs file — it is never written over then.
    pub fn read(home: &Path) -> Result<Self, String> {
        let path = Self::dir(home).join(FILE);
        let file = match glut::providers::open_no_follow(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(format!("{}: {err}", path.display())),
        };
        let mut text = String::new();
        file.take(MAX_BYTES)
            .read_to_string(&mut text)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The prefs, or none when the file cannot be read (a preference never stops a session).
    pub fn load(home: &Path) -> Self {
        Self::read(home).unwrap_or_default()
    }

    /// Writes the file whole, through a new file renamed into place.
    ///
    /// # Errors
    ///
    /// A message when it cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), String> {
        let dir = Self::dir(home);
        let path = dir.join(FILE);
        let mut text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        text.push('\n');
        glut::providers::make_private_dir(&dir)
            .and_then(|()| glut::providers::write_replacing(&dir, &path, text.as_bytes(), 0o644))
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The editor in use: the one set here, else `$VISUAL`, else `$EDITOR`, else none.
    pub fn editor(&self) -> Option<String> {
        self.editor.clone().or_else(|| {
            ["VISUAL", "EDITOR"].iter().find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
        })
    }
}

/// Sets `command` as the editor and keeps it: a command without `{file}` gets the file at its end.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written, or the command is empty.
pub fn set_editor(home: &Path, command: &str) -> Result<String, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("/editor needs a command".into());
    }
    let command = if command.contains("{file}") {
        command.to_owned()
    } else {
        format!("{command} {{file}}")
    };
    let mut prefs = Prefs::read(home)?;
    prefs.editor = Some(command.clone());
    prefs.save(home)?;
    Ok(command)
}

/// Keeps how the thinking is shown.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_thinking(home: &Path, thinking: crate::transcript::Thinking) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.thinking = Some(thinking);
    prefs.save(home)
}

/// Keeps whether the vi mode is on.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_vi(home: &Path, on: bool) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.vi = Some(on);
    prefs.save(home)
}

/// Keeps how a resting session keeps its cache (`/cache`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_cache(home: &Path, mode: crate::warm::CacheMode) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.cache = Some(mode);
    prefs.save(home)
}

/// Keeps how the footer's segments are joined (`/footer`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_footer(home: &Path, separators: crate::footer::Separators) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.footer = Some(separators);
    prefs.save(home)
}

/// Keeps that the first start's welcome was seen.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_welcomed(home: &Path) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.welcomed = Some(true);
    prefs.save(home)
}

/// Keeps how images are drawn (`/images`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_images(home: &Path, drawing: crate::pictures::Drawing) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.images = Some(drawing);
    prefs.save(home)
}

/// Keeps how links are shown (`/links`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_links(home: &Path, links: crate::links::Links) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.links = Some(links);
    prefs.save(home)
}

/// Keeps when the owner is told (`/notify`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_notify(
    home: &Path,
    when: glut::permissions::settings::NotifyWhen,
) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.notify = Some(when);
    prefs.save(home)
}

/// Keeps after how many seconds a turn's end is told (`/notify after`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_notify_after(home: &Path, secs: u32) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.notify_after = Some(secs);
    prefs.save(home)
}

/// Keeps the most rounds a loop runs (`/loop rounds`, T60a).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_loop_rounds(home: &Path, rounds: u32) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.loop_rounds = Some(rounds);
    prefs.save(home)
}

/// Keeps when Glut last looked for the plans' runs (T60b).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_runs_seen(home: &Path, at: chrono::DateTime<chrono::Utc>) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.runs_seen = Some(at);
    prefs.save(home)
}

/// Keeps the most a loop's session may cost (`/loop budget`, T60a).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_loop_max_usd(home: &Path, usd: f64) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.loop_max_usd = Some(usd);
    prefs.save(home)
}

/// Keeps whether tiles show (`/notify tiles`, T69).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_tiles(home: &Path, on: bool) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.tiles = Some(on);
    prefs.save(home)
}

/// Keeps how much the screen moves (`/motion`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_motion(home: &Path, motion: crate::motion::Motion) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.motion = Some(motion);
    prefs.save(home)
}

/// Keeps whether the reader mode is on (`/reader`).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_reader(home: &Path, on: bool) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.reader = Some(on);
    prefs.save(home)
}

/// Keeps the theme chosen with `/theme`.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_theme(home: &Path, name: &str) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.theme = Some(name.to_owned());
    prefs.save(home)
}

/// Keeps the language Glut speaks to the owner in (T48).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_language(home: &Path, lang: glut::lang::Lang) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.language = Some(lang.code().to_owned());
    prefs.save(home)
}

/// Keeps whether `folder` keeps its `CLAUDE.md` as its guide (T35): `true` adds it to the folders not asked again,
/// `false` (`/init --migrate`) takes it out.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_keep_claude(home: &Path, folder: &str, keep: bool) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.keep_claude.retain(|kept| kept != folder);
    if keep {
        prefs.keep_claude.push(folder.to_owned());
    }
    prefs.save(home)
}

/// Keeps the size the guide at `path` had when `/init` checked it (T42).
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_guide_checked(home: &Path, path: &str, bytes: u64) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.guide_checked.insert(path.to_owned(), bytes);
    prefs.save(home)
}

/// Forgets the sizes kept for `paths` (T42): shortened below the limit, they count afresh.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn forget_guides_checked(home: &Path, paths: &[String]) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.guide_checked.retain(|path, _| !paths.contains(path));
    prefs.save(home)
}

/// Keeps whether the side panel is shown.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written.
pub fn set_panel(home: &Path, shown: bool) -> Result<(), String> {
    let mut prefs = Prefs::read(home)?;
    prefs.panel = Some(shown);
    prefs.save(home)
}

/// The editor's command for `file` at `line`: `{file}` and `{line}` put in — the file at the end when the template
/// names none (`$EDITOR=nvim`) — split into the program and its arguments at spaces. No shell and no quoting: the
/// path stays one argument whatever it holds, but a program path with spaces needs a template without them.
pub fn editor_command(template: &str, file: &Path, line: usize) -> Option<(String, Vec<String>)> {
    if template.trim().is_empty() {
        return None;
    }
    let template = if template.contains("{file}") {
        template.to_owned()
    } else {
        format!("{template} {{file}}")
    };
    let mut parts = template.split_whitespace().map(|part| {
        part.replace("{line}", &line.to_string())
            .replace("{file}", &file.to_string_lossy())
    });
    let program = parts.next()?;
    Some((program, parts.collect()))
}

/// The editors of [`EDITORS`] that are on the `PATH`.
pub fn installed_editors() -> Vec<(&'static str, &'static str)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
    EDITORS
        .into_iter()
        .filter(|(program, _)| folders.iter().any(|folder| folder.join(program).is_file()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "glut-prefs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|at| at.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[test]
    fn the_editor_is_kept_with_what_else_the_file_holds_and_a_broken_file_is_never_written_over() {
        let home = home("keep");
        assert_eq!(
            set_editor(&home, "nvim").unwrap(),
            "nvim {file}",
            "the file goes at the end"
        );
        assert_eq!(Prefs::load(&home).editor.as_deref(), Some("nvim {file}"));
        let path = home.join(".glut").join(FILE);
        std::fs::write(&path, r#"{"editor": "vi {file}", "theme": "forest"}"#).unwrap();
        set_editor(&home, "zed {file}:{line}").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("forest") && text.contains("zed {file}:{line}"),
            "{text}"
        );
        std::fs::write(&path, "{ broken").unwrap();
        assert!(set_editor(&home, "hx").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
        assert!(set_editor(&home, "  ").is_err());
    }

    #[test]
    fn an_editor_command_takes_the_file_and_line_and_keeps_a_spaced_path_whole() {
        let file = Path::new("/tmp/my notes.md");
        assert_eq!(
            editor_command("nvim +{line} {file}", file, 3),
            Some(("nvim".into(), vec!["+3".into(), "/tmp/my notes.md".into()]))
        );
        assert_eq!(
            editor_command("code -g {file}:{line}", file, 1),
            Some((
                "code".into(),
                vec!["-g".into(), "/tmp/my notes.md:1".into()]
            ))
        );
        assert_eq!(editor_command("   ", file, 1), None);
        assert_eq!(
            editor_command("nvim", file, 1),
            Some(("nvim".into(), vec!["/tmp/my notes.md".into()])),
            "an editor from $EDITOR gets the file"
        );
    }
}
