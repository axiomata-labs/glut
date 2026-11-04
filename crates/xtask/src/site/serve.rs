//! The site on this machine while it is written: a small HTTP server on `127.0.0.1` alone that builds the pages again
//! when a source changed since the last build, and answers with the build's error instead of an old page.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{build, sources_state};

/// How long a client may take to send its request's head.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// The most of a request's head read, in bytes; a page's request needs a few hundred.
const HEAD_LIMIT: u64 = 16 * 1024;

/// Builds into `out` and serves it on `127.0.0.1:port` until stopped.
///
/// # Errors
///
/// The port cannot be bound.
pub fn serve(repo: &Path, out: &Path, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| format!("cannot listen on 127.0.0.1:{port}: {err}"))?;
    let mut built: Option<(Option<SystemTime>, usize)> = None;
    let mut failure: Option<String> = None;
    println!("site: http://127.0.0.1:{port}/ (Ctrl-C ends it)");
    for stream in listener.incoming().flatten() {
        let now = sources_state(repo);
        if built != Some(now) {
            built = Some(now);
            failure = match build(repo, out) {
                Ok(count) => {
                    println!("site: built {count} pages");
                    None
                }
                Err(err) => {
                    eprintln!("site: {err}");
                    Some(err)
                }
            };
        }
        if let Err(err) = answer(stream, out, failure.as_deref()) {
            eprintln!("site: {err}");
        }
    }
    Ok(())
}

/// Answers one request: the file it names below `out`, the build's error, or 404.
fn answer(mut stream: TcpStream, out: &Path, failure: Option<&str>) -> Result<(), String> {
    // One server loop answers everybody: a client that sends nothing (a browser's pre-connect) or never ends its
    // head must not hold it, nor grow its memory.
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|err| err.to_string())?;
    let mut reader = BufReader::new((&stream).take(HEAD_LIMIT));
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|err| err.to_string())?;
    // The rest of the head is read too: closing on unread bytes may answer with a reset that drops the page.
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|read| read > 0) && header.trim_end() != "" {
        header.clear();
    }
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    if let Some(failure) = failure {
        return respond(
            &mut stream,
            "500 Internal Server Error",
            "text/plain; charset=utf-8",
            failure.as_bytes(),
        );
    }
    match file_for(out, target).and_then(|path| std::fs::read(&path).ok().map(|body| (path, body)))
    {
        Some((path, body)) => respond(&mut stream, "200 OK", content_type(&path), &body),
        None => respond(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
        ),
    }
}

/// The file below `out` a request's path names — a folder's `index.html`, never a path that climbs out.
fn file_for(out: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let mut file = out.to_path_buf();
    for part in Path::new(path.trim_start_matches('/')).components() {
        match part {
            Component::Normal(part) => file.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if file.is_dir() {
        file.push("index.html");
    }
    file.is_file().then_some(file)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: \
         close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reaches_only_files_below_the_site() {
        let out = std::env::temp_dir().join(format!("glut-site-serve-{}", std::process::id()));
        std::fs::create_dir_all(out.join("docs")).unwrap();
        std::fs::write(out.join("index.html"), "i").unwrap();
        std::fs::write(out.join("docs/a.html"), "a").unwrap();
        assert_eq!(file_for(&out, "/"), Some(out.join("index.html")));
        assert_eq!(
            file_for(&out, "/docs/a.html?x#y"),
            Some(out.join("docs/a.html"))
        );
        assert_eq!(file_for(&out, "/../etc/passwd"), None);
        assert_eq!(file_for(&out, "/docs/missing.html"), None);
        std::fs::remove_dir_all(&out).unwrap();
    }
}
