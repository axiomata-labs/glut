//! The Linux sandbox (T59) as a command meets it: bubblewrap with this build's `glut` as the helper, each rule tried
//! from inside. Linux only, and only where the sandbox can run (a container needs the right to make namespaces).

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Output;

use glut::sandbox::Policy;
use glut::sandbox::scan::{Watch, search};

/// A project and a home of the test's own, removed when it ends.
struct Place {
    top: PathBuf,
    root: PathBuf,
    home: PathBuf,
}

impl Place {
    fn new(name: &str) -> Option<Self> {
        glut::sandbox::linux::use_helper(PathBuf::from(env!("CARGO_BIN_EXE_glut")));
        if let Some(why) = glut::sandbox::unavailable() {
            // Where the sandbox is meant to be there (the Docker runs of T59), its absence is a failure.
            assert!(
                std::env::var_os("GLUT_EXPECT_SANDBOX").is_none(),
                "no sandbox: {why}"
            );
            eprintln!("skipped, no sandbox here: {why}");
            return None;
        }
        // Not below the temporary folder: commands may write it, and the project's own rules are what is tried.
        let top = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("linux-sandbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let (root, home) = (top.join("project"), top.join("home"));
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/id_ed25519"), "PRIVATE").unwrap();
        std::fs::write(home.join("notes.txt"), "the owner's").unwrap();
        std::fs::write(root.join(".env"), "TOKEN=secret").unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app/.env.local"), "TOKEN=deeper").unwrap();
        Some(Place {
            root: root.canonicalize().unwrap(),
            home: home.canonicalize().unwrap(),
            top,
        })
    }

    fn policy(&self) -> Policy {
        let mut policy = Policy::new(&self.root, &self.home, &[], &[]);
        policy.git_dirs = vec![self.root.join(".git")];
        policy
    }

    /// Runs `script` with `sh -c` in the sandbox of `policy`.
    fn run_in(&self, policy: &Policy, script: &str, relay: Option<&Path>) -> Output {
        let found = search(&policy.root, &policy.beside, &policy.git_dirs);
        policy
            .command(Path::new("/bin/sh"), &self.root, &found, relay)
            .unwrap()
            .arg("-c")
            .arg(script)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn run(&self, script: &str) -> Output {
        self.run_in(&self.policy(), script, None)
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.top);
    }
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_command_writes_its_folder_and_nothing_else() {
    let Some(place) = Place::new("write") else {
        return;
    };
    let out = place.run("echo made > made.txt && echo changed > ../home/notes.txt");
    assert!(!out.status.success(), "{}", said(&out));
    assert_eq!(
        std::fs::read_to_string(place.root.join("made.txt")).unwrap(),
        "made\n"
    );
    assert_eq!(
        std::fs::read_to_string(place.home.join("notes.txt")).unwrap(),
        "the owner's"
    );
}

#[test]
fn secrets_and_env_files_read_as_nothing() {
    let Some(place) = Place::new("read") else {
        return;
    };
    let out = place
        .run("cat ../home/.ssh/id_ed25519; echo; cat .env app/.env.local; cat ../home/notes.txt");
    let text = said(&out);
    assert!(!text.contains("PRIVATE"), "{text}");
    assert!(
        !text.contains("secret") && !text.contains("deeper"),
        "{text}"
    );
    assert!(text.contains("the owner's"), "the rest reads: {text}");
    // Refused, as on macOS — not an empty file a model would take for one to write.
    assert_eq!(text.matches("Permission denied").count(), 3, "{text}");
    let out =
        place.run("ls ../home/.ssh || echo LISTING-REFUSED; echo x >> .env || echo WRITE-REFUSED");
    let text = said(&out);
    assert!(
        text.contains("LISTING-REFUSED") && text.contains("WRITE-REFUSED"),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(place.root.join(".env")).unwrap(),
        "TOKEN=secret"
    );
}

#[test]
fn git_hooks_and_config_stay_and_the_git_folder_stays_put() {
    let Some(place) = Place::new("git") else {
        return;
    };
    let out = place.run(
        "echo run > .git/hooks/pre-commit; echo '[x]' >> .git/config; mv .git moved; \
         echo object > .git/written; echo done",
    );
    let text = said(&out);
    assert!(text.contains("done"), "{text}");
    assert!(!place.root.join(".git/hooks/pre-commit").exists(), "{text}");
    assert_eq!(
        std::fs::read_to_string(place.root.join(".git/config")).unwrap(),
        "[core]\n"
    );
    assert!(!place.root.join("moved").exists(), "{text}");
    assert!(
        place.root.join(".git/written").exists(),
        "the rest of the git folder is written: {text}"
    );
}

#[test]
fn what_a_command_makes_that_macos_refuses_goes_to_the_quarantine() {
    let Some(place) = Place::new("quarantine") else {
        return;
    };
    let policy = place.policy();
    let before = search(&policy.root, &policy.beside, &policy.git_dirs);
    let out = place.run_in(
        &policy,
        "echo 'export X=1' > .envrc && mkdir -p sub/.git/hooks",
        None,
    );
    assert!(out.status.success(), "{}", said(&out));
    let note = Watch::new(&policy, before).settle().expect("a note");
    assert!(
        note.contains(".envrc") && note.contains("sub/.git"),
        "{note}"
    );
    assert!(!place.root.join(".envrc").exists());
    assert!(!place.root.join("sub/.git").exists());
    assert!(place.home.join(".glut/quarantine").is_dir());
}

#[test]
fn no_unix_socket_no_namespace_no_signal_to_others() {
    let Some(place) = Place::new("seccomp") else {
        return;
    };
    let mut outside = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = outside.id();
    let out = place.run(&format!(
        "perl -e 'use Socket; socket(my $s, PF_UNIX, SOCK_STREAM, 0) or print \"unix: $!\\n\"'; \
         perl -e 'socket(my $s, 40, 1, 0) or print \"vsock: $!\\n\"'; \
         unshare -U true || echo 'unshare refused'; \
         kill {pid} || echo 'kill refused'"
    ));
    let text = said(&out);
    let _ = outside.kill();
    let _ = outside.wait();
    assert!(text.contains("unix: Operation not permitted"), "{text}");
    // A VM's way to its host (`AF_VSOCK`): no family but the network's own.
    assert!(text.contains("vsock: Operation not permitted"), "{text}");
    assert!(text.contains("unshare refused"), "{text}");
    assert!(text.contains("kill refused"), "{text}");
}

#[test]
fn what_the_command_makes_that_macos_refuses_is_gone_while_it_still_runs() {
    let Some(place) = Place::new("watch") else {
        return;
    };
    let policy = place.policy();
    let before = search(&policy.root, &policy.beside, &policy.git_dirs);
    let mut command = policy
        .command(Path::new("/bin/sh"), &place.root, &before, None)
        .unwrap();
    let watch = Watch::new(&policy, before);
    let mut child = command
        .arg("-c")
        .arg("echo /elsewhere > .git/commondir; mkdir -p deep/sub/.git/hooks; mkdir -p .cargo; sleep 3")
        .current_dir(&place.root)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let running = child.try_wait().unwrap().is_none();
    let left: Vec<&str> = [".git/commondir", "deep/sub/.git", ".cargo"]
        .into_iter()
        .filter(|made| place.root.join(made).exists())
        .collect();
    let _ = child.kill();
    let _ = child.wait();
    let note = watch.settle().unwrap_or_default();
    assert!(running, "the command was still to run");
    assert!(
        left.is_empty(),
        "still there while it ran: {left:?}; {note}"
    );
    assert!(note.contains("at once"), "{note}");
}

#[test]
fn what_the_owner_changes_meanwhile_stays_where_it_is() {
    let Some(place) = Place::new("owners") else {
        return;
    };
    let policy = place.policy();
    let before = search(&policy.root, &policy.beside, &policy.git_dirs);
    let mut command = policy
        .command(Path::new("/bin/sh"), &place.root, &before, None)
        .unwrap();
    let watch = Watch::new(&policy, before);
    let mut child = command
        .arg("-c")
        .arg("sleep 2")
        .current_dir(&place.root)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    // As the owner's `git config` writes it: a new file, renamed over the old one.
    let git = place.root.join(".git");
    std::fs::write(git.join("config.lock"), "[core]\n[remote \"origin\"]\n").unwrap();
    std::fs::rename(git.join("config.lock"), git.join("config")).unwrap();
    let _ = child.wait();
    let note = watch.settle().unwrap_or_default();
    assert_eq!(
        std::fs::read_to_string(git.join("config")).unwrap(),
        "[core]\n[remote \"origin\"]\n",
        "{note}"
    );
    assert!(!note.contains(".git/config"), "{note}");
}

#[test]
fn nothing_outside_the_filter_can_be_reached_into() {
    let Some(place) = Place::new("helper") else {
        return;
    };
    // The helper is the first process and the command's parent: neither its memory nor a trace of it.
    let out = place.run(
        "for p in 1 $PPID; do perl -e 'open(my $f, \"+<\", \"/proc/$ARGV[0]/mem\") \
         and print \"WRITABLE $ARGV[0]\\n\" or print \"refused $ARGV[0]\\n\"' $p; done; \
         grep -c . /proc/1/environ 2>/dev/null || echo 'environ refused'",
    );
    let text = said(&out);
    assert!(!text.contains("WRITABLE"), "{text}");
    assert!(text.contains("refused 1"), "{text}");
    assert!(text.contains("environ refused"), "{text}");
}

#[test]
fn a_command_cannot_hand_a_job_to_the_system() {
    let Some(place) = Place::new("launchers") else {
        return;
    };
    let present: Vec<&str> = [
        "/usr/bin/systemctl",
        "/usr/bin/systemd-run",
        "/usr/bin/loginctl",
        "/usr/bin/crontab",
        "/usr/bin/at",
    ]
    .into_iter()
    .filter(|program| Path::new(program).exists())
    .collect();
    for program in present {
        // Not run at all: the shell cannot start it (126), whatever the program would have said.
        let out = place.run(&format!("{program} </dev/null >/dev/null 2>&1; echo rc=$?"));
        assert!(said(&out).contains("rc=126"), "{program}: {}", said(&out));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_network_is_the_proxy_through_the_relay_and_nothing_else() {
    let Some(place) = Place::new("network") else {
        return;
    };
    let proxy = glut::sandbox::proxy::Proxy::start(None).await.unwrap();
    let socket = place.top.join("proxy.sock");
    let _bridge = glut::sandbox::proxy::Bridge::start(&socket, proxy.port()).unwrap();
    let mut policy = place.policy();
    policy.proxy_port = Some(proxy.port());
    let port = proxy.port();
    let script = format!(
        "exec 3<>/dev/tcp/127.0.0.1/{port} && printf 'CONNECT 127.0.0.1:22 HTTP/1.1\\r\\n\\r\\n' >&3 && \
         head -c 12 <&3; echo; (exec 4<>/dev/tcp/127.0.0.1/22) 2>/dev/null || echo 'other port refused'"
    );
    let out = tokio::task::spawn_blocking(move || {
        let found = search(&policy.root, &policy.beside, &policy.git_dirs);
        policy
            .command(Path::new("/bin/bash"), &policy.root, &found, Some(&socket))
            .unwrap()
            .arg("-c")
            .arg(script)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let text = said(&out);
    // The proxy answered (it refuses this machine's own address), and only it was reached.
    assert!(text.contains("HTTP/1.1 502"), "{text}");
    assert!(text.contains("other port refused"), "{text}");
}
