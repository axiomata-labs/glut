//! The site onto its web server (glut-ai.com, Plesk at CHIPWORK on netcup): built and checked here, packed whole into
//! one archive before anything is sent, unpacked beside the web root and moved in only once it arrived whole. The
//! server has no Rust, so nothing is built there; it has no rsync either, so the archive replaces the root's contents
//! but what the server keeps of its own (`.htaccess` — the preview's password —, `.well-known` — Let's Encrypt's
//! challenges).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::check;

/// The web space's system user at the server. Not a secret: what lets a deploy in is the key.
const TARGET: &str = "glut-ai.com_ve4c7fnxgwi@185.243.11.81";
/// The server's host keys, from the repository's top: a deploy talks to this server or to none.
const KNOWN_HOSTS: &str = "site/deploy/known_hosts";
/// The key used where none is named: the one on the owner's Mac.
const DEFAULT_KEY: &str = ".config/glut-ai/deploy_ed25519";
/// Seconds to reach the server, and between keep-alives: a hung server ends the deploy instead of holding it.
const CONNECT_SECONDS: u32 = 20;
const ALIVE_SECONDS: u32 = 15;

/// Run by the server's shell (chrooted to the web space) with the archive on its input. The new files arrive in a
/// folder of their own first: a broken or cut stream leaves the site as it was. From the swap on, a dropped connection
/// no longer stops it (`HUP`, `TERM` ignored), so the root is never left half emptied; what a killed deploy left beside
/// the root goes after an hour.
const REMOTE: &str = r#"set -eu
umask 022
root=glut-ai.com/httpdocs
find glut-ai.com -mindepth 1 -maxdepth 1 -name '.deploy.*' -mmin +60 -exec rm -rf -- {} +
new=$(mktemp -d glut-ai.com/.deploy.XXXXXX)
trap 'rm -rf -- "$new"' EXIT
tar -xzf - -C "$new" --no-same-owner --no-same-permissions
test -f "$new/index.html" || { echo "no index.html arrived: the site was left as it was" >&2; exit 1; }
for own in .htaccess .well-known; do
  test ! -e "$new/$own" || { echo "the site holds the server's $own: the site was left as it was" >&2; exit 1; }
done
chmod -R u=rwX,go=rX -- "$new"
trap '' HUP TERM
find "$root" -mindepth 1 -maxdepth 1 ! -name .htaccess ! -name .well-known -exec rm -rf -- {} +
find "$new" -mindepth 1 -maxdepth 1 -exec mv -t "$root" -- {} +
echo "site: $(find "$root" -type f | wc -l) files on the server""#;

/// Builds and checks into `out`, then replaces the server's site with it, over SSH with `key` (the owner's key on this
/// machine where none is named).
///
/// # Errors
///
/// What [`check`] refuses — a site with a broken link is never sent —, no key, `tar` failing (nothing is sent
/// then) or `ssh` failing.
pub fn deploy(repo: &Path, out: &Path, key: Option<PathBuf>) -> Result<(), String> {
    let key = match key {
        Some(key) => key,
        None => PathBuf::from(std::env::var_os("HOME").ok_or("no HOME to find the deploy key in")?)
            .join(DEFAULT_KEY),
    };
    if !key.is_file() {
        return Err(format!("no deploy key at {}", key.display()));
    }
    check(repo, out)?;
    // #159: what Glut's update notice reads — only a release GitHub holds whole.
    let latest = latest_release()?;
    std::fs::write(out.join("latest.txt"), format!("{latest}\n"))
        .map_err(|err| format!("cannot write latest.txt: {err}"))?;
    println!("site: latest.txt says {latest}");
    // Packed whole before the server hears of it: a tar that dies halfway must never reach the server as a site cut
    // short that still unpacks.
    let archive = std::env::temp_dir().join(format!("glut-site-{}.tar.gz", std::process::id()));
    let packed = Command::new("tar")
        .args(tar_args(&archive, out))
        // macOS's tar would add `._` files for extended attributes.
        .env("COPYFILE_DISABLE", "1")
        .status()
        .map_err(|err| format!("cannot start tar: {err}"));
    let sent = packed.and_then(|packed| {
        if !packed.success() {
            return Err(format!("tar failed ({packed}): nothing was sent"));
        }
        let stream = File::open(&archive)
            .map_err(|err| format!("cannot read {}: {err}", archive.display()))?;
        Command::new("ssh")
            .args(ssh_args(&key, &repo.join(KNOWN_HOSTS)))
            .stdin(stream)
            .status()
            .map_err(|err| format!("cannot start ssh: {err}"))
    });
    let _ = std::fs::remove_file(&archive);
    let sent = sent?;
    if !sent.success() {
        return Err(format!(
            "the deploy failed on the way or on the server ({sent})"
        ));
    }
    Ok(())
}

/// The repository whose releases the binaries come from.
const RELEASES: &str = "axiomata-labs/glut";
/// What a release must hold before Glut may name it: every binary `install.sh` fetches, the script and the sums.
const RELEASE_ASSETS: [&str; 6] = [
    "glut-aarch64-apple-darwin.tar.gz",
    "glut-x86_64-apple-darwin.tar.gz",
    "glut-aarch64-unknown-linux-musl.tar.gz",
    "glut-x86_64-unknown-linux-musl.tar.gz",
    "install.sh",
    "SHA256SUMS",
];

/// The newest release on GitHub (no draft, no pre-release) that holds every one of [`RELEASE_ASSETS`], as a bare
/// version (`0.1.1`), through `gh`. A deploy that cannot tell stops: the site never names a version nobody can
/// install, and never goes without the file the update notice reads.
fn latest_release() -> Result<String, String> {
    let out = Command::new("gh")
        .args([
            "api",
            &format!("repos/{RELEASES}/releases/latest"),
            "--jq",
            "[.tag_name, (.assets | map(.name) | join(\" \"))] | join(\"\\n\")",
        ])
        .output()
        .map_err(|err| format!("cannot start gh to read the latest release: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh could not read the latest release: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (tag, assets) = text.trim().split_once('\n').unwrap_or((text.trim(), ""));
    released(tag, &assets.split(' ').collect::<Vec<_>>())
}

/// The version of release `tag` holding `assets`, when it holds every one of [`RELEASE_ASSETS`].
fn released(tag: &str, assets: &[&str]) -> Result<String, String> {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.parse::<u64>().is_err()) {
        return Err(format!("the latest release's tag {tag:?} is no version"));
    }
    let missing: Vec<&str> = RELEASE_ASSETS
        .iter()
        .filter(|asset| !assets.contains(asset))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "release {tag} lacks {}: not deployed — wait until its build finished",
            missing.join(", ")
        ));
    }
    Ok(version.to_owned())
}

/// `tar` packing `out`'s contents into `archive`, without the build's own marker.
fn tar_args(archive: &Path, out: &Path) -> Vec<String> {
    vec![
        "-czf".into(),
        archive.display().to_string(),
        "--exclude=.glut-site".into(),
        // macOS's extended attributes (quarantine, provenance) mean nothing to the server's tar.
        "--no-xattrs".into(),
        "-C".into(),
        out.display().to_string(),
        ".".into(),
    ]
}

/// `ssh` with the key alone (no agent, no password, no config of the machine's) and the pinned host keys alone.
fn ssh_args(key: &Path, known_hosts: &Path) -> Vec<String> {
    vec![
        "-F".into(),
        "/dev/null".into(),
        "-i".into(),
        key.display().to_string(),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
        "-o".into(),
        "GlobalKnownHostsFile=/dev/null".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_SECONDS}"),
        "-o".into(),
        format!("ServerAliveInterval={ALIVE_SECONDS}"),
        TARGET.into(),
        REMOTE.into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_release_holding_every_binary_is_named() {
        assert_eq!(released("v0.1.1", &RELEASE_ASSETS).unwrap(), "0.1.1");
        let err = released("v0.1.1", &RELEASE_ASSETS[..4]).unwrap_err();
        assert!(
            err.contains("install.sh") && err.contains("SHA256SUMS"),
            "{err}"
        );
        assert!(released("nightly", &RELEASE_ASSETS).is_err());
        assert!(released("v0.2.0-rc1", &RELEASE_ASSETS).is_err());
    }

    #[test]
    fn the_server_is_reached_with_the_key_and_the_pinned_host_keys_alone() {
        let args = ssh_args(Path::new("/k"), Path::new("/repo/site/deploy/known_hosts"));
        let options: Vec<&str> = args
            .windows(2)
            .filter(|pair| pair[0] == "-o")
            .map(|pair| pair[1].as_str())
            .collect();
        assert!(options.contains(&"IdentitiesOnly=yes"));
        assert!(
            options.contains(&"BatchMode=yes"),
            "never a password prompt"
        );
        assert!(
            options.contains(&"StrictHostKeyChecking=yes"),
            "an unknown host key is refused"
        );
        assert!(options.contains(&"UserKnownHostsFile=/repo/site/deploy/known_hosts"));
        assert_eq!(
            args[..4],
            ["-F", "/dev/null", "-i", "/k"],
            "no config of the machine's overrides these"
        );
        assert_eq!(args[args.len() - 2], TARGET);
    }

    #[test]
    fn the_server_keeps_its_own_files_and_takes_the_new_ones_only_once_they_arrived_whole() {
        let swap = REMOTE.find("find \"$root\"").unwrap();
        assert!(
            REMOTE.find("tar -xzf").unwrap() < swap,
            "unpacked before anything is removed"
        );
        assert!(REMOTE.find("test -f \"$new/index.html\"").unwrap() < swap);
        assert!(REMOTE.contains("! -name .htaccess ! -name .well-known"));
        assert!(
            REMOTE.find("trap '' HUP TERM").unwrap() < swap,
            "a dropped connection cannot stop the swap"
        );
        assert!(
            REMOTE.find("for own in .htaccess .well-known").unwrap() < swap,
            "the server's own are never replaced"
        );
        assert!(
            REMOTE.starts_with("set -eu"),
            "any failing step stops the rest"
        );
        assert!(
            tar_args(Path::new("a.tgz"), Path::new("out"))
                .contains(&"--exclude=.glut-site".to_owned())
        );
    }

    #[test]
    fn the_pinned_host_keys_are_in_the_repository() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let known = std::fs::read_to_string(repo.join(KNOWN_HOSTS)).unwrap();
        let host = TARGET.split('@').nth(1).unwrap();
        assert!(
            known
                .lines()
                .any(|line| line.starts_with(&format!("{host} ssh-ed25519 ")))
        );
    }
}
