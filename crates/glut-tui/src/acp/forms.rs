//! A server's question in the editor (T64b, T67): an MCP `elicitation/create` goes on to the editor as ACP's own
//! `elicitation/create` where it said it shows forms or pages. Glut rebuilds the form from what it read of it — never
//! the server's raw schema — and checks the editor's answer by the form's own rules, as the TUI's card does. A form
//! that reads like it asks a secret is declined unshown here too, and nothing reaches the model.

use glut::mcp::elicit::{Form, Format, Input, Kind, Reply, What};
use serde_json::{Map, Value, json};

/// A form as the editor gets it: the server's message and a flat schema of Glut's own making.
pub fn request(form: &Form) -> Value {
    let mut properties = Map::new();
    for field in &form.fields {
        let mut schema = match &field.kind {
            Kind::Text {
                format,
                min,
                max,
                default,
            } => {
                let mut schema = json!({"type": "string"});
                if let Some(format) = format {
                    schema["format"] = json!(match format {
                        Format::Email => "email",
                        Format::Uri => "uri",
                        Format::Date => "date",
                        Format::DateTime => "date-time",
                    });
                }
                put(&mut schema, "minLength", min.map(Value::from));
                put(&mut schema, "maxLength", max.map(Value::from));
                put(&mut schema, "default", default.clone().map(Value::from));
                schema
            }
            Kind::Number {
                integer,
                min,
                max,
                default,
            } => {
                let mut schema = json!({"type": if *integer { "integer" } else { "number" }});
                // An integer field's bounds are whole numbers: `5`, never `5.0`, for an editor that checks.
                let number = |value: f64| {
                    if *integer && value.fract() == 0.0 && value.abs() < 9.0e15 {
                        Value::from(value as i64)
                    } else {
                        Value::from(value)
                    }
                };
                put(&mut schema, "minimum", min.map(number));
                put(&mut schema, "maximum", max.map(number));
                put(&mut schema, "default", default.map(number));
                schema
            }
            Kind::Bool { default } => {
                let mut schema = json!({"type": "boolean"});
                put(&mut schema, "default", default.map(Value::from));
                schema
            }
            Kind::Choice { options, default } => {
                let one_of: Vec<Value> = options
                    .iter()
                    .map(|option| json!({"const": option.value, "title": shown(&option.label)}))
                    .collect();
                let mut schema = json!({"type": "string", "oneOf": one_of});
                let default = default
                    .and_then(|at| options.get(at))
                    .map(|option| Value::from(option.value.clone()));
                put(&mut schema, "default", default);
                schema
            }
            Kind::Choices {
                options,
                min,
                max,
                default,
            } => {
                let any_of: Vec<Value> = options
                    .iter()
                    .map(|option| json!({"const": option.value, "title": shown(&option.label)}))
                    .collect();
                let mut schema = json!({"type": "array", "items": {"anyOf": any_of}});
                put(&mut schema, "minItems", min.map(Value::from));
                put(&mut schema, "maxItems", max.map(Value::from));
                if !default.is_empty() {
                    let picked: Vec<Value> = default
                        .iter()
                        .filter_map(|at| options.get(*at))
                        .map(|option| Value::from(option.value.clone()))
                        .collect();
                    schema["default"] = Value::Array(picked);
                }
                schema
            }
        };
        put(&mut schema, "title", field.title.clone().map(Value::from));
        put(
            &mut schema,
            "description",
            field.description.clone().map(Value::from),
        );
        properties.insert(field.name.clone(), schema);
    }
    let required: Vec<&str> = form
        .fields
        .iter()
        .filter(|field| field.required)
        .map(|field| field.name.as_str())
        .collect();
    json!({"type": "object", "properties": properties, "required": required})
}

/// A server's text as the editor shows it: control, bidi and invisible characters out.
fn shown(text: &str) -> Value {
    Value::from(crate::transcript::clean(text).to_string())
}

/// A server's message on one line, cut: a long one cannot push the server's name out of view.
pub fn one_line(text: &str) -> String {
    let line = text.replace(['\n', '\r'], " ");
    crate::transcript::clean(&crate::transcript::cut(&line, MESSAGE_CHARS)).to_string()
}

/// How much of a server's message the editor shows.
const MESSAGE_CHARS: usize = 300;

fn put(schema: &mut Value, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        schema[key] = value;
    }
}

/// The editor's answer to a form, checked by the form's rules: only its own fields, each value as the TUI's card
/// would send it; an answer that breaks a rule is no answer (`Cancel`).
pub fn reply(form: &Form, result: Option<&Value>) -> Reply {
    let Some(result) = result else {
        return Reply::Cancel;
    };
    match result.get("action").and_then(Value::as_str) {
        Some("accept") => {}
        Some("decline") => return Reply::Decline,
        _ => return Reply::Cancel,
    }
    let content = result.get("content").and_then(Value::as_object);
    let inputs: Vec<Input> = form
        .fields
        .iter()
        .map(|field| {
            let given = content.and_then(|content| content.get(&field.name));
            match (&field.kind, given) {
                (_, None) => field.start(),
                (Kind::Text { .. } | Kind::Number { .. }, Some(Value::String(text))) => {
                    Input::Text(text.clone())
                }
                (Kind::Number { .. }, Some(Value::Number(number))) => {
                    Input::Text(number.to_string())
                }
                (Kind::Bool { .. }, Some(Value::Bool(value))) => Input::Bool(*value),
                (Kind::Choice { options, .. }, Some(Value::String(value))) => {
                    Input::Choice(options.iter().position(|option| option.value == *value))
                }
                (Kind::Choices { options, .. }, Some(Value::Array(values))) => Input::Choices(
                    options
                        .iter()
                        .map(|option| {
                            values
                                .iter()
                                .any(|value| value.as_str() == Some(&option.value))
                        })
                        .collect(),
                ),
                // A value of the wrong kind sends nothing for the field: a required one then fails its check.
                (Kind::Text { .. } | Kind::Number { .. }, Some(_)) => Input::Text(String::new()),
                (Kind::Bool { .. }, Some(_)) => field.start(),
                (Kind::Choice { .. }, Some(_)) => Input::Choice(None),
                (Kind::Choices { options, .. }, Some(_)) => {
                    Input::Choices(vec![false; options.len()])
                }
            }
        })
        .collect();
    match form.answer(&inputs) {
        Ok(content) => Reply::Accept(content),
        Err(_) => Reply::Cancel,
    }
}

/// A page the server wants opened, as the editor gets it — the server, the host it leads to and whether it is
/// unencrypted said before the server's words, as the TUI's card says them: opened or not, nothing typed goes back.
pub fn page_request(server: &str, what: &What, id: &str) -> Option<Value> {
    let What::Page { message, url } = what else {
        return None;
    };
    let host = glut::mcp::elicit::host_of(url);
    let plain = if url.starts_with("http://") {
        " ⚠ http"
    } else {
        ""
    };
    let said = format!("{server} → {host}{plain}: {}", one_line(message));
    Some(json!({"mode": "url", "message": said, "url": url, "elicitationId": id}))
}

/// The editor's answer to a page: opened is an accept without content, anything else no answer.
pub fn page_reply(result: Option<&Value>) -> Reply {
    match result
        .and_then(|result| result.get("action"))
        .and_then(Value::as_str)
    {
        Some("accept") => Reply::Accept(Map::new()),
        Some("decline") => Reply::Decline,
        _ => Reply::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        let parsed = glut::mcp::elicit::parse(&json!({
            "message": "Pick",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "minLength": 2},
                    "age": {"type": "integer", "maximum": 150},
                    "size": {"type": "string", "enum": ["s", "m"]},
                    "tags": {"type": "array", "items": {"anyOf": [{"const": "x"}, {"const": "y"}]}},
                },
                "required": ["name"],
            },
        }))
        .unwrap();
        match parsed {
            What::Form(form) => form,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_form_goes_to_the_editor_as_glut_read_it() {
        let schema = request(&form());
        assert_eq!(schema["required"], json!(["name"]));
        assert_eq!(schema["properties"]["age"]["type"], "integer");
        assert_eq!(schema["properties"]["size"]["oneOf"][1]["const"], "m");
        assert_eq!(schema["properties"]["tags"]["type"], "array");
    }

    #[test]
    fn the_editors_answer_is_checked_by_the_forms_own_rules() {
        let form = form();
        let accepted = reply(
            &form,
            Some(&json!({"action": "accept", "content": {
                "name": "Ada", "age": 36, "size": "m", "tags": ["y", "z"], "extra": "dropped",
            }})),
        );
        let Reply::Accept(content) = accepted else {
            panic!("{accepted:?}")
        };
        assert_eq!(content.get("age"), Some(&json!(36)));
        assert_eq!(content.get("tags"), Some(&json!(["y"])));
        assert!(
            content.get("extra").is_none(),
            "a field the form has not is not sent"
        );
        // A rule broken (too short, out of bounds, the required one missing) is no answer.
        for content in [
            json!({"name": "A"}),
            json!({"name": "Ada", "age": 999}),
            json!({"age": 3}),
        ] {
            let said = reply(
                &form,
                Some(&json!({"action": "accept", "content": content})),
            );
            assert_eq!(said, Reply::Cancel, "{content}");
        }
        assert_eq!(
            reply(&form, Some(&json!({"action": "decline"}))),
            Reply::Decline
        );
        assert_eq!(reply(&form, None), Reply::Cancel);
    }

    #[test]
    fn a_page_says_its_server_and_host_and_hides_no_name_before_the_host() {
        let page = glut::mcp::elicit::parse(
            &json!({"mode": "url", "message": "Sign\nin", "url": "http://example.com/x"}),
        )
        .unwrap();
        let sent = page_request("srv", &page, "s-page-0").unwrap();
        assert_eq!(sent["message"], "srv → example.com ⚠ http: Sign in");
        assert!(
            glut::mcp::elicit::parse(&json!({"mode": "url", "message": "m", "url": "https://known.example@evil.example/"}))
                .is_err()
        );
    }
}
