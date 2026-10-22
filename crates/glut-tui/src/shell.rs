//! The owner's own commands from the input (T9): `!cmd` runs a command and hands its output to the model with the
//! next message, `!!cmd` runs it for the owner alone. The owner typed it, so it runs as in the owner's terminal —
//! their shell, their folder, no gate and no sandbox — and never as the model's tool call. It runs in a process group
//! of its own, away from Glut's terminal: a timeout or Glut's end stops all of it, and a password prompt cannot take
//! Glut's keys.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process_group};
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::Command;

/// How long an owner's command may run before Glut stops it.
const TIMEOUT: Duration = Duration::from_secs(120);
/// The most of a command's output kept, each of its two streams: what comes after is read and dropped, so a command
/// that prints without end costs no memory.
const MAX_OUTPUT: usize = 64 * 1024;

/// What a command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// Whether it ended with success.
    pub ok: bool,
    /// What it printed — its errors after its output — cut where it was too long, with how it ended below.
    pub output: String,
}

/// Runs `command` in `cwd` with the owner's shell (`$SHELL -c`, `/bin/sh` without one).
pub async fn run(command: &str, cwd: &Path) -> Ran {
    run_within(command, cwd, TIMEOUT).await
}

async fn run_within(command: &str, cwd: &Path, timeout: Duration) -> Ran {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let spawned = Command::new(shell)
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            return Ran {
                ok: false,
                output: format!("could not start it: {err}"),
            };
        }
    };
    // The whole group goes when Glut stops waiting: a server started with `&` as well as the shell.
    let group = child.id().and_then(|id| Pid::from_raw(id as i32));
    let stop = || {
        if let Some(group) = group {
            let _ = kill_process_group(group, Signal::KILL);
        }
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        stop();
        return Ran {
            ok: false,
            output: "no output streams".into(),
        };
    };
    let reading = async {
        let (out, err) = tokio::join!(bounded(stdout), bounded(stderr));
        let status = child.wait().await;
        (out, err, status)
    };
    match tokio::time::timeout(timeout, reading).await {
        Ok((out, err, status)) => {
            let ok = status.as_ref().is_ok_and(std::process::ExitStatus::success);
            let ending = match status {
                Ok(status) if status.success() => String::new(),
                Ok(status) => format!("({status})"),
                Err(err) => format!("(could not wait for it: {err})"),
            };
            Ran {
                ok,
                output: joined(&out, &err, &ending),
            }
        }
        Err(_) => {
            stop();
            Ran {
                ok: false,
                output: format!("(stopped after {} s)", timeout.as_secs()),
            }
        }
    }
}

/// A stream read to its end, its first [`MAX_OUTPUT`] bytes kept; whether more came.
async fn bounded(mut stream: impl AsyncRead + Unpin) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut more = false;
    let mut chunk = [0_u8; 8192];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = MAX_OUTPUT.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..read.min(room)]);
                more |= read > room;
            }
        }
    }
    (kept, more)
}

/// The output, its errors after it, each said cut where it was, and how the command ended last.
fn joined(out: &(Vec<u8>, bool), err: &(Vec<u8>, bool), ending: &str) -> String {
    let text = |(bytes, more): &(Vec<u8>, bool)| {
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        if *more {
            text.push_str(&format!(
                "\n… more than {} KB, the rest left out\n",
                MAX_OUTPUT / 1024
            ));
        }
        text
    };
    let mut output = text(out);
    let errors = text(err);
    if !errors.trim().is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&errors);
    }
    if !ending.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(ending);
    }
    output
}

/// What the model is told of a command the owner ran with `!`: the command and its output, as the owner's — a key of
/// the owner's providers that the output shows (`!env`) left out, and the output fenced so no line of it closes the
/// fence.
pub fn for_the_model(command: &str, ran: &Ran) -> String {
    let state = if ran.ok { "" } else { " (it failed)" };
    let output = redacted(ran.output.trim_end());
    let longest = output.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("I ran `{command}` myself{state}. Its output:\n{fence}\n{output}\n{fence}")
}

/// `text` with the values of the provider keys in Glut's environment replaced: they never go to a model.
fn redacted(text: &str) -> String {
    let mut names: Vec<String> = glut::providers::catalog::key_envs()
        .map(str::to_owned)
        .collect();
    names.extend(glut::providers::configured_key_envs());
    names.push("AXH_API_KEY".into());
    let mut text = text.to_owned();
    for name in names {
        if let Ok(value) = std::env::var(&name)
            && value.trim().len() >= 8
        {
            text = text.replace(value.trim(), &format!("[{name} left out]"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_command_runs_in_the_folder_and_brings_its_output_and_errors() {
        let cwd = std::env::temp_dir();
        let ran = run("echo out; echo err >&2", &cwd).await;
        assert!(ran.ok);
        assert_eq!(ran.output, "out\nerr\n");
        let failed = run("exit 3", &cwd).await;
        assert!(!failed.ok);
        assert!(failed.output.contains('3'), "{:?}", failed.output);
        assert!(for_the_model("exit 3", &failed).contains("(it failed)"));
        let only_errors = run("echo bad >&2", &cwd).await;
        assert_eq!(only_errors.output, "bad\n");
    }

    #[tokio::test]
    async fn a_command_without_end_is_cut_and_stopped_with_all_it_started() {
        let cwd = std::env::temp_dir();
        let flood = run("yes | head -c 200000", &cwd).await;
        assert!(
            flood.output.len() < MAX_OUTPUT + 200,
            "{}",
            flood.output.len()
        );
        assert!(flood.output.contains("the rest left out"));
        let slow = run_within("sleep 30 & sleep 30", &cwd, Duration::from_millis(300)).await;
        assert!(!slow.ok);
        assert!(slow.output.contains("stopped after"), "{:?}", slow.output);
    }

    #[test]
    fn the_model_gets_the_output_in_a_fence_it_cannot_close() {
        let ran = Ran {
            ok: true,
            output: "before\n```\nafter".into(),
        };
        let told = for_the_model("cat README.md", &ran);
        assert!(told.contains("````\nbefore\n```\nafter\n````"), "{told}");
    }
}
