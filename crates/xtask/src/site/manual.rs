//! A page of the terminal UI's manual (`crates/glut-tui/manual/<lang>/<page>.md`) as HTML. The pages are written for
//! the help box, line by line: the first line says what the command is for, a line `/cmd … — what it does` is one
//! of its forms, a line starting with `/` alone is an example, the rest are paragraphs.

/// One page, its name being the command's.
pub fn render(name: &str, text: &str, commands: &[&str]) -> String {
    let mut html = String::new();
    let mut forms = Vec::new();
    let mut lines = text.lines().map(str::trim_end).peekable();
    if let Some(lead) = lines.next() {
        html.push_str(&format!(
            "<p class=\"lead\">{}</p>\n",
            linked(lead, name, commands)
        ));
    }
    for line in lines {
        if let Some((form, what)) = line.split_once(" — ").filter(|_| line.starts_with('/')) {
            forms.push((form.to_owned(), what.to_owned()));
            continue;
        }
        flush(&mut html, &mut forms, name, commands);
        if line.is_empty() {
            continue;
        }
        if line.starts_with('/') {
            html.push_str(&format!(
                "<pre class=\"example\"><code>{}</code></pre>\n",
                escape(line)
            ));
        } else {
            html.push_str(&format!("<p>{}</p>\n", linked(line, name, commands)));
        }
    }
    flush(&mut html, &mut forms, name, commands);
    html
}

/// The forms gathered so far, as one list.
fn flush(html: &mut String, forms: &mut Vec<(String, String)>, name: &str, commands: &[&str]) {
    if forms.is_empty() {
        return;
    }
    html.push_str("<dl class=\"forms\">\n");
    for (form, what) in forms.drain(..) {
        html.push_str(&format!(
            "<dt><code>{}</code></dt><dd>{}</dd>\n",
            escape(&form),
            linked(&what, name, commands)
        ));
    }
    html.push_str("</dl>\n");
}

/// `text` escaped, every `/command` of another page a link to it — only where it stands as a word of its own (after
/// a space, a bracket or at the start), so a path like `~/.glut/arena` stays text.
fn linked(text: &str, own: &str, commands: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut before = ' ';
    while let Some(at) = rest.find('/') {
        let (head, tail) = rest.split_at(at);
        out.push_str(&escape(head));
        before = head.chars().last().unwrap_or(before);
        let word: String = tail[1..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase())
            .collect();
        let alone = matches!(before, ' ' | '(' | '"' | '“' | '„' | '«');
        let after = &tail[1 + word.len()..];
        let mut next = after.chars();
        let ends = match next.next() {
            None => true,
            // A sentence's full stop ends the word; `/help.md` is a file.
            Some('.') => next.next().is_none_or(|c| !c.is_alphanumeric()),
            Some(c) => !(c.is_alphanumeric() || matches!(c, '/' | '-' | '_')),
        };
        if alone && ends && word != own && commands.contains(&word.as_str()) {
            out.push_str(&format!("<a href=\"{word}.html\"><code>/{word}</code></a>"));
        } else {
            out.push('/');
            out.push_str(&escape(&word));
        }
        before = word.chars().last().unwrap_or('/');
        rest = &tail[1 + word.len()..];
    }
    out.push_str(&escape(rest));
    out
}

/// `text` safe inside an element or a quoted attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_reads_as_lead_forms_paragraphs_and_examples() {
        let text = "What it is for.\n\n/arena — the models\n/arena take <n> — model n's changes\n\nSee /help and \
                    ~/.glut/arena for more.\n\n/arena sonnet, gpt-5: fix it\n";
        let html = render("arena", text, &["arena", "help"]);
        assert!(
            html.starts_with("<p class=\"lead\">What it is for.</p>"),
            "{html}"
        );
        assert!(
            html.contains("<dt><code>/arena take &lt;n&gt;</code></dt><dd>model n's changes</dd>"),
            "{html}"
        );
        assert!(
            html.contains(
                "See <a href=\"help.html\"><code>/help</code></a> and ~/.glut/arena for more."
            ),
            "{html}"
        );
        assert!(
            html.contains("<pre class=\"example\"><code>/arena sonnet, gpt-5: fix it</code></pre>"),
            "{html}"
        );
    }

    #[test]
    fn only_a_command_of_another_page_is_linked() {
        let commands = ["help", "loop"];
        assert_eq!(linked("/loop stops", "loop", &commands), "/loop stops");
        assert_eq!(
            linked("a /help, b", "x", &commands),
            "a <a href=\"help.html\"><code>/help</code></a>, b"
        );
        assert_eq!(
            linked("in ~/.glut/help.md", "x", &commands),
            "in ~/.glut/help.md"
        );
        assert_eq!(linked("/nothing", "x", &commands), "/nothing");
    }
}
