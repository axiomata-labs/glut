//! The filters against real outputs of this repo and of small projects (`fixtures/`, paths and names anonymised).

use super::*;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/src/tools/output/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

/// Compacts `fixture` as the output of `command`; panics when it is left whole.
fn compacted(command: &str, name: &str) -> Compacted {
    compact(command, &fixture(name)).unwrap_or_else(|| panic!("{command} on {name} was left whole"))
}

/// Every line of `original` that `keep` matches is in `text`, word for word.
fn keeps(text: &str, original: &str, keep: impl Fn(&str) -> bool) {
    for line in original.lines().filter(|l| keep(l)) {
        assert!(
            text.contains(line.trim_end()),
            "lost: {line:?}\n---\n{text}"
        );
    }
}

#[test]
fn only_a_simple_command_of_a_known_program_is_compacted() {
    let words = |c| simple_command(c).map(|w| w.join(" "));
    assert_eq!(
        words("cd crates/x && cargo test 2>&1").as_deref(),
        Some("cargo test")
    );
    assert_eq!(
        words("RUST_BACKTRACE=1 cargo test").as_deref(),
        Some("cargo test")
    );
    assert_eq!(words("cargo test | tail -20"), None);
    assert_eq!(words("cargo test > out.txt"), None);
    assert_eq!(words("cargo build && cargo test"), None);
    assert_eq!(words("echo $(git status)"), None);
    assert_eq!(
        kind_of(&["git", "-C", "repo", "status"]),
        Some(Kind::GitStatus)
    );
    assert_eq!(kind_of(&["git", "status", "--porcelain"]), None);
    assert_eq!(kind_of(&["git", "log", "--oneline"]), None);
    assert_eq!(kind_of(&["git", "diff", "--stat"]), None);
    assert_eq!(kind_of(&["cargo", "test", "-q"]), None);
    assert_eq!(kind_of(&["cargo", "run"]), None);
    assert_eq!(
        kind_of(&["uv", "run", "--with", "pytest", "pytest", "-v"]),
        Some(Kind::Pytest)
    );
    assert_eq!(kind_of(&["python3", "-m", "pytest"]), Some(Kind::Pytest));
    assert_eq!(kind_of(&["npx", "vitest", "run"]), Some(Kind::JsTests));
    assert_eq!(kind_of(&["pnpm", "install"]), Some(Kind::Install));
    assert_eq!(kind_of(&["cmake", "--build", "build"]), Some(Kind::Make));
    assert_eq!(kind_of(&["rm", "-rf", "x"]), None);
    // A short output is never touched.
    assert_eq!(
        compact(
            "git status",
            "On branch main\nnothing to commit, working tree clean\n"
        ),
        None
    );
}

#[test]
fn git_status_says_what_the_short_form_says() {
    let out = compacted("git status", "git-status.txt");
    assert!(out.text.starts_with("## glut-lean"), "{}", out.text);
    assert!(out.text.contains(
        "R  crates/axiomata-harness/src/history.rs -> crates/axiomata-harness/src/results.rs"
    ));
    assert!(out.text.contains(" M crates/axiomata-harness/src/run.rs"));
    assert!(!out.text.contains("(use \"git"));
}

#[test]
fn git_diff_keeps_every_hunk_line_and_sums_the_files_up() {
    let original = fixture("git-diff.txt");
    let out = compacted("git diff", "git-diff.txt");
    // Every changed line and every header stays.
    keeps(&out.text, &original, |l| {
        (l.starts_with('+') && !l.starts_with("+++ "))
            || (l.starts_with('-') && !l.starts_with("--- "))
            || l.starts_with("@@")
            || l.starts_with("diff --git")
    });
    assert!(!out.text.lines().any(|l| l.starts_with("index ")));
    assert!(
        out.text.lines().next().unwrap().ends_with("files changed:"),
        "{}",
        &out.text[..200]
    );
    // A typical edit — small changes in a file — loses most of its unchanged lines.
    let edit = compacted("git show bceb203", "git-show-edit.txt");
    assert!(
        edit.lines_after * 10 < edit.lines_before * 8,
        "{} → {}",
        edit.lines_before,
        edit.lines_after
    );
}

#[test]
fn git_log_is_one_line_a_commit() {
    let out = compacted("git log -8", "git-log.txt");
    assert_eq!(out.text.lines().count(), 8, "{}", out.text);
    assert!(
        out.text
            .starts_with("66cf5443f Tue Oct 20 18:18:28 2026 +0200 Owner: docs(plans): Glut lean")
    );
}

#[test]
fn cargo_keeps_warnings_failures_and_the_summary_and_drops_progress() {
    // A test run that compiles first, as most do.
    let fail = fixture("cargo-build.txt") + &fixture("cargo-test-fail.txt");
    let out = compact("cargo test", &fail).expect("compacted");
    keeps(&out.text, &fail, |l| {
        l.contains("FAILED")
            || l.contains("panicked")
            || l.contains("left:")
            || l.starts_with("warning")
    });
    assert!(!out.text.contains("test tests::adds ... ok"));
    assert!(!out.text.contains("   Compiling"));

    let ok = compacted("cargo test -p axiomata-tasks", "cargo-test-ok.txt");
    assert!(
        ok.text.contains("test result: ok. 25 passed"),
        "{}",
        ok.text
    );
    assert!(!ok.text.contains("running 0 tests"), "{}", ok.text);

    let build = compacted("cargo build -p axiomata-cli", "cargo-build.txt");
    assert!(build.text.contains("Finished"), "{}", build.text);
    assert!(build.lines_after < 5, "{}", build.text);
}

#[test]
fn pytest_drops_its_header_and_passing_tests_and_keeps_the_failure() {
    let original = fixture("pytest-fail.txt");
    let out = compacted(
        "uv run --no-project --with pytest pytest -v",
        "pytest-fail.txt",
    );
    keeps(&out.text, &original, |l| {
        l.starts_with('E') || l.contains("FAILED") || l.contains("failed")
    });
    assert!(
        !out.text.contains("PASSED") && !out.text.contains("rootdir:"),
        "{}",
        out.text
    );
}

#[test]
fn swift_drops_progress_colours_and_the_compilers_invocation() {
    let out = compacted("swift test", "swift-compile-fail.txt");
    assert!(!out.text.contains("[24 / 96]") && !out.text.contains('\u{1b}'));
    assert!(
        !out.text.contains("swift-frontend -frontend"),
        "the invocation is gone"
    );
    assert!(
        out.text
            .contains("error: use of 'add' refers to instance method"),
        "{}",
        out.text
    );
    assert!(out.text.contains("error: Build failed"));

    let tests = compacted("swift test", "swift-test-fail.txt");
    assert!(tests.text.contains("testBad]' failed"), "{}", tests.text);
    assert!(tests.text.contains("✘ Test addsWrong() recorded an issue"));
    assert!(
        !tests.text.contains("testOk]' passed") && !tests.text.contains("✔ Test adds() passed")
    );
}

#[test]
fn a_long_listing_is_grouped_by_folder() {
    let out = compacted("find crates -name '*.rs'", "find.txt");
    assert!(out.lines_after < out.lines_before / 2, "{}", out.text);
    assert!(
        out.text.lines().any(|l| l.contains(" more")),
        "{}",
        out.text
    );
}

#[test]
fn an_output_that_would_hardly_shrink_is_left_whole() {
    // The vitest run of this repo prints only its summary already.
    assert_eq!(compact("npx vitest run", &fixture("vitest-ok.txt")), None);
    let lines: String = (0..30)
        .map(|n| format!("src/a{n}.rs:1: error: something\n"))
        .collect();
    assert_eq!(compact("cargo build", &lines), None);
}

#[test]
fn glut_s_own_notes_survive_every_filter() {
    let mut text = fixture("cargo-build.txt") + &fixture("cargo-test-fail.txt");
    text.push_str("\n[exit code 101]");
    let out = compact("cargo test", &text).unwrap();
    assert!(out.text.ends_with("[exit code 101]"), "{}", out.text);
}

#[test]
fn the_other_filters_drop_their_noise_and_keep_what_matters() {
    let repeat = |line: &str, n: usize| line.repeat(n);

    let pip = repeat(
        "Collecting requests\n  Downloading requests-2.32.3-py3-none-any.whl (64 kB)\n",
        10,
    ) + "ERROR: Could not find a version that satisfies the requirement nope\n";
    let out = compact("pip install -r requirements.txt", &pip).unwrap();
    assert_eq!(
        out.text,
        "ERROR: Could not find a version that satisfies the requirement nope"
    );

    let pnpm = repeat(
        "Progress: resolved 120, reused 118, downloaded 2, added 0\n",
        12,
    ) + "++++++++++++\n WARN  deprecated glob@7.2.3\nDone in 2.1s\n";
    let out = compact("pnpm install", &pnpm).unwrap();
    assert_eq!(out.text, " WARN  deprecated glob@7.2.3\nDone in 2.1s");

    let check = "Loading svelte-check in workspace: /w\nGetting Svelte diagnostics...\n\n"
        .to_owned()
        + &repeat(
            "/w/src/a.svelte:3:5\nError: Type 'string' is not assignable to type 'number'.\n",
            6,
        );
    // Almost all of it is diagnostics: dropping two progress lines does not pay, it stays whole.
    assert_eq!(compact("npx svelte-check", &check), None);

    let cmake = repeat(
        "[ 42%] Building C object src/CMakeFiles/core.dir/a.c.o\n",
        20,
    ) + "/w/src/b.c:7:3: error: use of undeclared identifier 'x'\nmake[2]: *** [src/b.c.o] Error 1\n";
    let out = compact("cmake --build build", &cmake).unwrap();
    assert_eq!(
        out.text,
        "/w/src/b.c:7:3: error: use of undeclared identifier 'x'\nmake[2]: *** [src/b.c.o] Error 1"
    );

    let xcode = repeat(
        "CompileSwift normal arm64 /w/App/A.swift (in target 'App' from project 'App')\n    cd /w\n    \
         /Applications/Xcode.app/Contents/Developer/usr/bin/swiftc -c A.swift\n\n",
        6,
    ) + "/w/App/B.swift:3:9: error: cannot find 'foo' in scope\n** BUILD FAILED **\n";
    let out = compact("xcodebuild -scheme App build", &xcode).unwrap();
    assert_eq!(
        out.text,
        "/w/App/B.swift:3:9: error: cannot find 'foo' in scope\n** BUILD FAILED **"
    );

    let mut tree = String::from(".\n├── src\n");
    for n in 0..80 {
        tree.push_str(&format!("│   ├── file{n}.rs\n"));
    }
    tree.push_str("└── Cargo.toml\n\n2 directories, 81 files\n");
    let out = compact("tree", &tree).unwrap();
    assert!(
        out.text.contains("file7.rs") && !out.text.contains("file8.rs"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("… 72 more") && out.text.contains("Cargo.toml"),
        "{}",
        out.text
    );

    let mut ls = String::from("total 4096\n");
    for n in 0..70 {
        ls.push_str(&format!(
            "-rw-r--r--  1 owner  staff  {} Oct 20 18:00 note{n}.md\n",
            100 + n
        ));
    }
    let out = compact("ls -la", &ls).unwrap();
    assert!(
        out.text.contains("note0.md (100 B)") && out.text.contains("62 more"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("-rw-r--r--"));
}

#[test]
fn git_show_keeps_its_commit_and_a_file_print_or_the_models_own_format_is_left_alone() {
    let out = compacted("git show bceb203", "git-show-edit.txt");
    assert!(out.text.contains("commit bceb203"), "{}", out.text);
    assert!(out.text.contains("Author: Owner"), "{}", out.text);
    assert_eq!(kind_of(&["git", "show", "HEAD:src/a.rs"]), None);
    assert_eq!(kind_of(&["git", "diff", "-U10"]), None);
    assert_eq!(kind_of(&["git", "diff", "--unified=5"]), None);
    assert_eq!(kind_of(&["git", "diff", "-W"]), None);
    assert_eq!(kind_of(&["git", "log", "--numstat"]), None);
    assert_eq!(kind_of(&["git", "status", "-sb"]), None);
    assert_eq!(kind_of(&["git", "status", "--ignored"]), None);
    // A "diff" that is none — a file printed — is not filtered.
    let file: String = (0..40).map(|n| format!("line {n}\n")).collect();
    assert_eq!(compact("git diff", &file), None);
}

#[test]
fn a_failed_commands_prefix_and_glut_s_notes_survive_the_git_filters() {
    let diff = format!("error: {}\n[exit code 1]", fixture("git-diff.txt"));
    let out = compact("git diff --exit-code", &diff).unwrap();
    assert!(out.text.starts_with("error: "), "{}", &out.text[..120]);
    assert!(out.text.ends_with("[exit code 1]"), "{}", out.text);
    assert!(!out.text.lines().any(|l| l.starts_with("index ")));
    let log = format!(
        "{}\n[… 4096 bytes of output in the middle were not kept …]",
        fixture("git-log.txt")
    );
    let out = compact("git log -8", &log).unwrap();
    assert!(out.text.ends_with("not kept …]"), "{}", out.text);
}

#[test]
fn find_with_an_action_that_prints_something_else_is_no_listing() {
    assert_eq!(
        kind_of(&["find", "src", "-name", "*.c", "-exec", "cat", "{}", "+"]),
        None
    );
    assert_eq!(kind_of(&["fd", "-e", "rs", "-x", "wc", "-l"]), None);
    assert_eq!(kind_of(&["find", "src", "-print0"]), None);
    assert_eq!(
        kind_of(&["find", "src", "-name", "*.rs"]),
        Some(Kind::Listing)
    );
}

#[test]
fn a_diffs_trailing_spaces_stay() {
    let mut diff = String::from(
        "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,20 +1,20 @@\n",
    );
    for n in 0..20 {
        diff.push_str(&format!(" same {n}\n"));
    }
    diff.push_str("-foo\n+foo   \n");
    let out = compact("git diff", &diff).unwrap();
    assert!(
        out.text.contains("\n+foo   \n") || out.text.ends_with("\n+foo   "),
        "{:?}",
        out.text
    );
}

#[test]
fn a_chain_is_split_outside_quotes_and_only_of_simple_commands() {
    let parts = |command| chain(command).map(|chain| (chain.parts, chain.ops));
    assert_eq!(
        parts("cargo fmt && cargo test"),
        Some((vec!["cargo fmt", "cargo test"], vec!["&&"]))
    );
    assert_eq!(
        parts("cd crates/a && cargo build 2>&1; git status || true"),
        Some((
            vec!["cd crates/a", "cargo build 2>&1", "git status", "true"],
            vec!["&&", ";", "||"]
        ))
    );
    // An operator inside quotes or escaped belongs to the part — which is then no simple command.
    assert_eq!(
        split_chain(r#"echo "a && b"; ls"#).unwrap().0,
        [r#"echo "a && b""#, "ls"]
    );
    assert_eq!(
        split_chain(r"echo 'x;y' && ls").unwrap().0,
        ["echo 'x;y'", "ls"]
    );
    assert_eq!(
        split_chain(r"echo a\;b; ls").unwrap().0,
        [r"echo a\;b", "ls"]
    );
    assert_eq!(parts(r#"echo "a && b"; ls"#), None);
    // One command, nothing Glut knows, a pipe, a redirection, a background part, a `;;`, an open quote: no chain.
    for command in [
        "cargo test",
        "cd a && cargo test",
        "echo a && echo b",
        "cargo build | tail && git status",
        "cargo build > log && git status",
        "sleep 1 & git status",
        "git status;; ls",
        "git status && echo 'open",
        "git status && $(rm x)",
        "git status &&",
        "&& git status",
    ] {
        assert_eq!(parts(command), None, "{command}");
    }
}

#[test]
fn marks_come_out_again_and_say_where_each_part_began() {
    let mark = "axh-mark-00ff";
    let chain = chain("cargo fmt && cargo test").unwrap();
    let marked = marked(&chain, mark, "builtin ");
    assert_eq!(
        marked,
        "{ builtin printf '%s-%s\\n' 'axh-mark-00ff' '0'\ncargo fmt\n} && \
         { builtin printf '%s-%s\\n' 'axh-mark-00ff' '1'\ncargo test\n}"
    );
    // The command text holds no mark: a part that prints it (`env` shows `AXH_COMMAND`) loses nothing.
    assert!(!marked.contains("axh-mark-00ff-"));
    let printed = "[note]\naxh-mark-00ff-0\naxh-mark-00ff-1\nrunning 2 tests\nok\n";
    let (text, ranges) = unmark(printed, mark);
    assert_eq!(text, "[note]\nrunning 2 tests\nok\n");
    assert_eq!(ranges, Some(vec![(0, 0..7), (1, 7..text.len())]));
    // A part without a closing line break still ends where the next mark begins.
    let (text, ranges) = unmark("axh-mark-00ff-0\nhalfaxh-mark-00ff-1\nrest", mark);
    assert_eq!(text, "halfrest");
    assert_eq!(ranges, Some(vec![(0, 0..4), (1, 4..8)]));
    // `a || b; c` with `a` succeeding: `b` has no mark and no range.
    let (text, ranges) = unmark("axh-mark-00ff-0\na\naxh-mark-00ff-2\nc\n", mark);
    assert_eq!(text, "a\nc\n");
    assert_eq!(ranges, Some(vec![(0, 0..2), (2, 2..4)]));
    // Marks out of order (a part printed one) or a lost middle leave no parts, but every mark is taken out.
    let (text, ranges) = unmark("axh-mark-00ff-1\na\naxh-mark-00ff-0\nb\n", mark);
    assert_eq!((text.as_str(), ranges), ("a\nb\n", None));
    let cut = format!("axh-mark-00ff-0\na\n[… 9 {DROPPED_MIDDLE} …]\nz\n");
    assert_eq!(unmark(&cut, mark).1, None);
    assert_eq!(
        unmark("no marks at all\n", mark),
        ("no marks at all\n".into(), None)
    );
}

#[test]
fn a_chain_the_marks_would_read_otherwise_is_left_alone() {
    // A comment (whose text the marks would turn into commands), `$?` and `$_` (which would see the mark's `printf`),
    // keywords, groups, subshells and backslashes (which the braces would break): no chain, the output stays whole.
    for command in [
        "git status # note && rm -rf build",
        "cargo test; echo \"exit=$?\"",
        "cargo build; echo $_",
        "cargo build; if [ -f x ]; then echo y; fi",
        "cargo build; for f in a b; do echo $f; done",
        "git status && (cd sub; cargo test)",
        "git status && { cargo test; }",
        "cargo build && echo a \\",
        "cargo build && echo $'a\\'b'",
        "! cargo build; git status",
    ] {
        assert_eq!(chain(command), None, "{command}");
    }
}

#[test]
fn a_known_command_in_front_of_a_pipe_is_named_for_the_count() {
    assert_eq!(piped("cargo test 2>&1 | tail -30"), Some("cargo"));
    assert_eq!(piped("git diff | head"), Some("git diff"));
    assert_eq!(piped("cat x | grep y"), None);
    assert_eq!(piped("cargo test || true"), None);
    assert_eq!(piped("cargo test"), None);
}
