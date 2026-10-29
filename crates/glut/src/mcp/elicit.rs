//! What a server asks the owner for while one of its tools runs (T67, MCP `elicitation/create`): a form of flat
//! fields, or a page to open. Only the owner answers — never the model — through the run's [`Gate`], whose asker
//! shows it; where nobody is there to answer, the answer is a decline.
//!
//! A form never asks for a secret: the protocol forbids it (that is what the page is for), and a field whose name,
//! title or description reads like one makes Glut decline the whole form and tell the owner why. A value typed into
//! a form goes to the server alone; what the server makes of it in its tool's answer is masked like any result.
//!
//! [`Gate`]: crate::permissions::Gate

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::{Map, Value, json};

/// How long a form waits for the owner before Glut cancels it.
pub const WAIT: Duration = Duration::from_secs(10 * 60);
/// The most fields one form may have: a form is a few questions, not a page of them.
const MAX_FIELDS: usize = 20;
/// The most options one choice may offer.
const MAX_OPTIONS: usize = 100;
/// The longest message, title, description, name or option label kept; a longer one is cut with `…`.
const MAX_TEXT_CHARS: usize = 2_000;
/// The longest address a page request may name.
const MAX_URL_CHARS: usize = 2_000;
/// The longest text a field takes where its schema sets no `maxLength`.
pub const MAX_VALUE_CHARS: usize = 10_000;

/// The words that make a form read like it asks a secret (T67, Q2): a word of its message, or of a field's name, title
/// or description, that is one of these, or holds one of [`SECRET_PARTS`]. A guess, never a barrier: the protocol
/// forbids a form to ask one at all, and the owner reads every field before sending it.
const SECRET_WORDS: [&str; 8] = ["key", "pwd", "pin", "otp", "cvv", "cvc", "tan", "geheimnis"];
/// The parts that make a word read like a secret wherever they stand in it (`accessToken`, `apikey`).
const SECRET_PARTS: [&str; 14] = [
    "password",
    "passwort",
    "passwd",
    "passphrase",
    "passcode",
    "kennwort",
    "secret",
    "token",
    "apikey",
    "privatekey",
    "accesskey",
    "credential",
    "mnemonic",
    "schlüssel",
];
/// The longest name a field may have: it is the key its value goes back under, so it is never cut.
const MAX_NAME_CHARS: usize = 200;

/// A server's question as the owner's asker gets it.
#[derive(Debug, Clone, PartialEq)]
pub struct Asked {
    /// The server, by the name in the owner's `mcp.json` or the project's `.mcp.json`.
    pub server: String,
    /// The subagent whose call the server asks during, by its task as the parent labelled it; `None`: the run itself.
    pub from: Option<String>,
    pub what: What,
}

/// What a server asks.
#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// A form to fill in.
    Form(Form),
    /// A page to open — a sign-in, a payment: the owner opens it or not, nothing typed goes back.
    Page { message: String, url: String },
    /// A form with a field that reads like a secret, `field` by its title or name: Glut declines it without showing
    /// it, and the asker only tells the owner.
    Secret { field: String },
}

/// A form: the server's message and its fields, in the order the server named them.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub message: String,
    pub fields: Vec<Field>,
}

/// One field of a form.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The key its value goes back under.
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: bool,
    pub kind: Kind,
}

/// What a field takes.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Text {
        format: Option<Format>,
        min: Option<usize>,
        max: Option<usize>,
        default: Option<String>,
    },
    Number {
        integer: bool,
        min: Option<f64>,
        max: Option<f64>,
        default: Option<f64>,
    },
    Bool {
        default: Option<bool>,
    },
    /// One of the options.
    Choice {
        options: Vec<Choice>,
        default: Option<usize>,
    },
    /// Some of the options.
    Choices {
        options: Vec<Choice>,
        min: Option<usize>,
        max: Option<usize>,
        default: Vec<usize>,
    },
}

/// The form of a text field's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Email,
    Uri,
    Date,
    DateTime,
}

/// One option of a choice: the value that goes back, and what the owner reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// The owner's answer to a server.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// Sent: the form's values (none for a page the owner opened).
    Accept(Map<String, Value>),
    /// Refused: the server should not ask again for this.
    Decline,
    /// Closed without an answer, or not answered in time.
    Cancel,
}

impl Reply {
    /// The `result` of the `elicitation/create` response.
    pub fn result(&self) -> Value {
        match self {
            Reply::Accept(content) if content.is_empty() => json!({"action": "accept"}),
            Reply::Accept(content) => json!({"action": "accept", "content": content}),
            Reply::Decline => json!({"action": "decline"}),
            Reply::Cancel => json!({"action": "cancel"}),
        }
    }
}

/// What an asker's [`crate::permissions::Asker::fill`] returns.
pub type ReplyFuture<'a> = Pin<Box<dyn Future<Output = Reply> + Send + 'a>>;

/// What the owner put into one field so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A text or a number, as typed.
    Text(String),
    Bool(bool),
    /// The option picked, if any.
    Choice(Option<usize>),
    /// Each option, picked or not.
    Choices(Vec<bool>),
}

/// Why a field's value cannot be sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    Required,
    TooShort(usize),
    TooLong(usize),
    NotEmail,
    NotUri,
    NotDate,
    NotDateTime,
    NotNumber,
    NotInteger,
    Below(f64),
    Above(f64),
    TooFew(usize),
    TooMany(usize),
}

/// The params of an `elicitation/create` request as the owner may be asked them.
///
/// # Errors
///
/// What makes it no question Glut asks: a form whose schema is not the protocol's flat one, holds a field of a kind it
/// does not know or more than it shows, a page whose address is not of the web. The server then hears a decline.
pub fn parse(params: &Value) -> Result<What, String> {
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .map(bounded)
        .unwrap_or_default();
    match params.get("mode").and_then(Value::as_str).unwrap_or("form") {
        "url" => {
            let url = params
                .get("url")
                .and_then(Value::as_str)
                .ok_or("a page request without a url")?;
            if url.chars().count() > MAX_URL_CHARS {
                return Err(format!("an address longer than {MAX_URL_CHARS} characters"));
            }
            let parsed = reqwest::Url::parse(url).map_err(|_| "an address that is none")?;
            if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
                return Err("an address not of the web".into());
            }
            Ok(What::Page {
                message,
                url: parsed.to_string(),
            })
        }
        "form" => {
            let schema = params
                .get("requestedSchema")
                .ok_or("a form without a requestedSchema")?;
            let form = form(message, schema)?;
            if let Some(field) = marked_secret(schema) {
                return Ok(What::Secret { field });
            }
            if names_secret(&form.message) {
                return Ok(What::Secret {
                    field: form
                        .fields
                        .first()
                        .map_or_else(String::new, |field| field.label().to_owned()),
                });
            }
            Ok(match form.fields.iter().find(|field| field.secret()) {
                Some(field) => What::Secret {
                    field: field.label().to_owned(),
                },
                None => What::Form(form),
            })
        }
        other => Err(format!("a mode {other:?} Glut does not know")),
    }
}

/// The host of a page's address, as the owner should read it before opening it (an international name in its
/// `xn--` form, which cannot pass for another).
pub fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default()
}

fn form(message: String, schema: &Value) -> Result<Form, String> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err("a schema that is no object".into());
    }
    let properties = match schema.get("properties") {
        Some(Value::Object(properties)) => properties,
        None => {
            return Ok(Form {
                message,
                fields: Vec::new(),
            });
        }
        Some(_) => return Err("properties that are no object".into()),
    };
    if properties.len() > MAX_FIELDS {
        return Err(format!("more than {MAX_FIELDS} fields"));
    }
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(name) = properties
        .keys()
        .find(|name| name.chars().count() > MAX_NAME_CHARS)
    {
        return Err(format!(
            "a field name of {} characters",
            name.chars().count()
        ));
    }
    let fields = properties
        .iter()
        .map(|(name, property)| {
            Ok(Field {
                name: name.clone(),
                title: text_of(property, "title"),
                description: text_of(property, "description"),
                required: required.contains(&name.as_str()),
                kind: kind(property).map_err(|reason| format!("the field {name:?}: {reason}"))?,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Form { message, fields })
}

/// The first field the schema itself marks as a secret's — `format: password`, or `writeOnly`, a value never to be
/// shown back —, by its title or name.
fn marked_secret(schema: &Value) -> Option<String> {
    schema
        .get("properties")?
        .as_object()?
        .iter()
        .find(|(_, property)| {
            property.get("format").and_then(Value::as_str) == Some("password")
                || property.get("writeOnly") == Some(&Value::Bool(true))
        })
        .map(|(name, property)| text_of(property, "title").unwrap_or_else(|| name.clone()))
}

fn kind(property: &Value) -> Result<Kind, String> {
    let kind = property
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "string" if property.get("enum").is_some() || property.get("oneOf").is_some() => {
            let options = options(property)?;
            let default = property
                .get("default")
                .and_then(Value::as_str)
                .and_then(|value| options.iter().position(|option| option.value == value));
            Ok(Kind::Choice { options, default })
        }
        "string" => Ok(Kind::Text {
            format: match property.get("format").and_then(Value::as_str) {
                None => None,
                Some("email") => Some(Format::Email),
                Some("uri") => Some(Format::Uri),
                Some("date") => Some(Format::Date),
                Some("date-time") => Some(Format::DateTime),
                // A password format is a secret's: the field reads like one and the form is declined.
                Some("password") => None,
                Some(other) => return Err(format!("a format {other:?} Glut does not know")),
            },
            min: count(property, "minLength"),
            max: count(property, "maxLength"),
            default: text_of(property, "default"),
        }),
        "number" | "integer" => Ok(Kind::Number {
            integer: kind == "integer",
            min: property.get("minimum").and_then(Value::as_f64),
            max: property.get("maximum").and_then(Value::as_f64),
            default: property.get("default").and_then(Value::as_f64),
        }),
        "boolean" => Ok(Kind::Bool {
            default: property.get("default").and_then(Value::as_bool),
        }),
        "array" => {
            let items = property.get("items").ok_or("a list without items")?;
            let options = options(items)?;
            let default = property
                .get("default")
                .and_then(Value::as_array)
                .map(|picked| {
                    picked
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|value| options.iter().position(|option| option.value == value))
                        .collect()
                })
                .unwrap_or_default();
            Ok(Kind::Choices {
                options,
                min: count(property, "minItems"),
                max: count(property, "maxItems"),
                default,
            })
        }
        other => Err(format!("a type {other:?} Glut does not show")),
    }
}

/// The options of a choice: `enum` (with the legacy `enumNames` as their labels), `oneOf` or `anyOf` of
/// `{const, title}`.
fn options(property: &Value) -> Result<Vec<Choice>, String> {
    let options: Vec<Choice> = if let Some(values) = property.get("enum").and_then(Value::as_array)
    {
        let names = property.get("enumNames").and_then(Value::as_array);
        values
            .iter()
            .enumerate()
            .map(|(at, value)| {
                let value = value.as_str().ok_or("an option that is no text")?;
                let label = names
                    .and_then(|names| names.get(at))
                    .and_then(Value::as_str)
                    .unwrap_or(value);
                Ok(Choice {
                    value: value.to_owned(),
                    label: bounded(label),
                })
            })
            .collect::<Result<_, String>>()?
    } else if let Some(values) = property
        .get("oneOf")
        .or_else(|| property.get("anyOf"))
        .and_then(Value::as_array)
    {
        values
            .iter()
            .map(|option| {
                let value = option
                    .get("const")
                    .and_then(Value::as_str)
                    .ok_or("an option without a const")?;
                Ok(Choice {
                    value: value.to_owned(),
                    label: bounded(option.get("title").and_then(Value::as_str).unwrap_or(value)),
                })
            })
            .collect::<Result<_, String>>()?
    } else {
        return Err("a choice without options".into());
    };
    if options.is_empty() || options.len() > MAX_OPTIONS {
        return Err(format!("a choice of no or more than {MAX_OPTIONS} options"));
    }
    Ok(options)
}

fn text_of(property: &Value, key: &str) -> Option<String> {
    property.get(key).and_then(Value::as_str).map(bounded)
}

fn count(property: &Value, key: &str) -> Option<usize> {
    property
        .get(key)
        .and_then(Value::as_u64)
        .map(|count| usize::try_from(count).unwrap_or(usize::MAX))
}

/// A server's text held to [`MAX_TEXT_CHARS`].
fn bounded(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT_CHARS {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX_TEXT_CHARS).collect();
    cut.push('…');
    cut
}

/// The words of `text`, lower case: split at white space and where a lower-case letter meets an upper-case one
/// (`accessToken`). Every other character that is no letter or digit is left out rather than split at — `pass-word`,
/// `pass_word` and a word with an invisible character inside read as the word they show.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut lower_before = false;
    for c in text.chars() {
        if !c.is_alphanumeric() && !c.is_whitespace() {
            continue;
        }
        if c.is_whitespace() {
            words.push(std::mem::take(&mut word));
            lower_before = false;
            continue;
        }
        if c.is_uppercase() && lower_before {
            words.push(std::mem::take(&mut word));
        }
        lower_before = c.is_lowercase() || c.is_ascii_digit();
        word.extend(c.to_lowercase());
    }
    words.push(word);
    words.retain(|word| !word.is_empty());
    words
}

/// Whether `text` reads like it names a secret.
fn names_secret(text: &str) -> bool {
    words(text).iter().any(|word| {
        SECRET_WORDS.contains(&word.as_str()) || SECRET_PARTS.iter().any(|part| word.contains(part))
    })
}

impl Field {
    /// What the owner reads the field as: its title, else its name.
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }

    /// Whether the field reads like it asks for a secret (T67, Q2).
    fn secret(&self) -> bool {
        [
            Some(&self.name),
            self.title.as_ref(),
            self.description.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|text| names_secret(text))
    }

    /// What the field holds before the owner touches it: the server's default, else nothing.
    pub fn start(&self) -> Input {
        match &self.kind {
            Kind::Text { default, .. } => Input::Text(default.clone().unwrap_or_default()),
            Kind::Number { default, .. } => {
                Input::Text(default.map(|number| number.to_string()).unwrap_or_default())
            }
            Kind::Bool { default } => Input::Bool(default.unwrap_or(false)),
            Kind::Choice { default, .. } => Input::Choice(*default),
            Kind::Choices {
                options, default, ..
            } => Input::Choices((0..options.len()).map(|at| default.contains(&at)).collect()),
        }
    }

    /// The value `input` sends, `None` for a field left empty, or why it cannot be sent.
    ///
    /// # Errors
    ///
    /// The [`Problem`] with it: a required field left empty, a text of the wrong form or length, a number out of
    /// bounds, too few or too many options picked.
    pub fn check(&self, input: &Input) -> Result<Option<Value>, Problem> {
        let value = match (&self.kind, input) {
            (
                Kind::Text {
                    format, min, max, ..
                },
                Input::Text(text),
            ) => check_text(text, *format, *min, *max)?,
            (
                Kind::Number {
                    integer, min, max, ..
                },
                Input::Text(text),
            ) => check_number(text, *integer, *min, *max)?,
            (Kind::Bool { .. }, Input::Bool(value)) => Some(Value::Bool(*value)),
            (Kind::Choice { options, .. }, Input::Choice(picked)) => picked
                .and_then(|at| options.get(at))
                .map(|option| Value::String(option.value.clone())),
            (
                Kind::Choices {
                    options, min, max, ..
                },
                Input::Choices(picked),
            ) => {
                let values: Vec<Value> = options
                    .iter()
                    .zip(picked)
                    .filter(|(_, picked)| **picked)
                    .map(|(option, _)| Value::String(option.value.clone()))
                    .collect();
                if values.is_empty() {
                    None
                } else if min.is_some_and(|min| values.len() < min) {
                    return Err(Problem::TooFew(min.unwrap_or_default()));
                } else if max.is_some_and(|max| values.len() > max) {
                    return Err(Problem::TooMany(max.unwrap_or_default()));
                } else {
                    Some(Value::Array(values))
                }
            }
            _ => None,
        };
        match value {
            None if self.required => Err(Problem::Required),
            value => Ok(value),
        }
    }
}

fn check_text(
    text: &str,
    format: Option<Format>,
    min: Option<usize>,
    max: Option<usize>,
) -> Result<Option<Value>, Problem> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let length = text.chars().count();
    if let Some(min) = min.filter(|min| length < *min) {
        return Err(Problem::TooShort(min));
    }
    let most = max.unwrap_or(MAX_VALUE_CHARS).min(MAX_VALUE_CHARS);
    if length > most {
        return Err(Problem::TooLong(most));
    }
    match format {
        Some(Format::Email) => {
            let fits = text.split_once('@').is_some_and(|(name, host)| {
                !name.is_empty() && host.contains('.') && !host.contains('@')
            }) && !text.contains(char::is_whitespace);
            if !fits {
                return Err(Problem::NotEmail);
            }
        }
        Some(Format::Uri) => {
            reqwest::Url::parse(text).map_err(|_| Problem::NotUri)?;
        }
        Some(Format::Date) => {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| Problem::NotDate)?;
        }
        Some(Format::DateTime) => {
            chrono::DateTime::parse_from_rfc3339(text).map_err(|_| Problem::NotDateTime)?;
        }
        None => {}
    }
    Ok(Some(Value::String(text.to_owned())))
}

fn check_number(
    text: &str,
    integer: bool,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<Option<Value>, Problem> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let number: f64 = text.parse().map_err(|_| Problem::NotNumber)?;
    if !number.is_finite() {
        return Err(Problem::NotNumber);
    }
    if let Some(min) = min.filter(|min| number < *min) {
        return Err(Problem::Below(min));
    }
    if let Some(max) = max.filter(|max| number > *max) {
        return Err(Problem::Above(max));
    }
    if integer {
        // `42.0` and `1e3` are whole numbers too; one past what an i64 holds is none the server could take.
        if number.fract() != 0.0 || number.abs() >= 9.0e15 {
            return Err(Problem::NotInteger);
        }
        return Ok(Some(Value::from(number as i64)));
    }
    Ok(serde_json::Number::from_f64(number).map(Value::Number))
}

impl Form {
    /// What the form sends for `inputs`, one per field in its order, or the first field that cannot be sent and why.
    ///
    /// # Errors
    ///
    /// The field's index and its [`Problem`].
    pub fn answer(&self, inputs: &[Input]) -> Result<Map<String, Value>, (usize, Problem)> {
        let mut content = Map::new();
        for (at, (field, input)) in self.fields.iter().zip(inputs).enumerate() {
            if let Some(value) = field.check(input).map_err(|problem| (at, problem))? {
                content.insert(field.name.clone(), value);
            }
        }
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form_of(schema: Value) -> Form {
        match parse(&json!({"message": "Tell me", "requestedSchema": schema})).unwrap() {
            What::Form(form) => form,
            other => panic!("not a form: {other:?}"),
        }
    }

    #[test]
    fn every_kind_of_field_the_protocol_names_is_read() {
        let form = form_of(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "title": "Name", "minLength": 2},
                "mail": {"type": "string", "format": "email"},
                "age": {"type": "integer", "minimum": 0, "maximum": 150, "default": 30},
                "ok": {"type": "boolean", "default": true},
                "size": {"type": "string", "enum": ["s", "m"], "enumNames": ["Small", "Medium"], "default": "m"},
                "tier": {"type": "string", "oneOf": [{"const": "a", "title": "Alpha"}]},
                "tags": {"type": "array", "items": {"anyOf": [{"const": "x", "title": "X"}, {"const": "y"}]},
                         "maxItems": 1, "default": ["y"]},
            },
            "required": ["name", "tags"],
        }));
        assert_eq!(form.message, "Tell me");
        assert_eq!(form.fields.len(), 7);
        let field = |name: &str| form.fields.iter().find(|field| field.name == name).unwrap();
        assert!(field("name").required && !field("mail").required);
        assert_eq!(field("name").label(), "Name");
        assert_eq!(field("age").start(), Input::Text("30".into()));
        assert_eq!(field("ok").start(), Input::Bool(true));
        assert_eq!(field("size").start(), Input::Choice(Some(1)));
        assert!(
            matches!(&field("size").kind, Kind::Choice { options, .. } if options[0].label == "Small")
        );
        assert!(
            matches!(&field("tier").kind, Kind::Choice { options, .. } if options[0].label == "Alpha")
        );
        assert_eq!(field("tags").start(), Input::Choices(vec![false, true]));
        assert_eq!(
            field("tags").check(&Input::Choices(vec![false, false])),
            Err(Problem::Required),
            "a required choice of several is not sent empty"
        );
    }

    #[test]
    fn a_form_glut_cannot_show_is_refused() {
        let refused =
            |schema: Value| parse(&json!({"message": "m", "requestedSchema": schema})).is_err();
        assert!(refused(json!({"type": "array"})));
        assert!(refused(
            json!({"type": "object", "properties": {"x": {"type": "object"}}})
        ));
        assert!(refused(
            json!({"type": "object", "properties": {"x": {"type": "string", "format": "ipv4"}}})
        ));
        assert!(refused(
            json!({"type": "object", "properties": {"x": {"type": "string", "enum": []}}})
        ));
        let many: Map<String, Value> = (0..=MAX_FIELDS)
            .map(|at| (format!("f{at}"), json!({"type": "boolean"})))
            .collect();
        assert!(refused(json!({"type": "object", "properties": many})));
        assert!(parse(&json!({"mode": "telepathy"})).is_err());
    }

    #[test]
    fn a_field_that_reads_like_a_secret_declines_the_form() {
        let secret = |property: Value| {
            matches!(
                parse(
                    &json!({"message": "m", "requestedSchema": {"type": "object", "properties": {"x": property}}})
                ),
                Ok(What::Secret { .. })
            )
        };
        assert!(secret(json!({"type": "string", "title": "Password"})));
        assert!(secret(json!({"type": "string", "title": "API key"})));
        assert!(secret(
            json!({"type": "string", "description": "Your accessToken for the service"})
        ));
        assert!(secret(json!({"type": "string", "title": "Client secret"})));
        assert!(secret(json!({"type": "string", "title": "Passwort"})));
        assert!(secret(json!({"type": "string", "format": "password"})));
        assert!(secret(
            json!({"type": "string", "writeOnly": true, "title": "Code"})
        ));
        let named = parse(&json!({"message": "m", "requestedSchema": {
            "type": "object", "properties": {"apiKey": {"type": "string"}}}}));
        assert_eq!(
            named,
            Ok(What::Secret {
                field: "apiKey".into()
            })
        );
        assert!(secret(json!({"type": "string", "title": "Pass-word"})));
        assert!(secret(
            json!({"type": "string", "title": "pass\u{200b}word"})
        ));
        assert!(secret(json!({"type": "string", "title": "SSH privateKey"})));
        let in_message = parse(
            &json!({"message": "Paste your API token", "requestedSchema": {
            "type": "object", "properties": {"value": {"type": "string"}}}}),
        );
        assert!(
            matches!(in_message, Ok(What::Secret { .. })),
            "{in_message:?}"
        );
        assert!(!secret(json!({"type": "string", "title": "Keyword"})));
        assert!(!secret(json!({"type": "string", "title": "Monkey name"})));
        assert!(!secret(json!({"type": "string", "title": "Your name"})));
    }

    #[test]
    fn a_page_must_be_of_the_web() {
        let page = parse(
            &json!({"mode": "url", "message": "Sign in", "url": "https://example.com/login?x=1",
                                 "elicitationId": "e1"}),
        );
        assert_eq!(
            page,
            Ok(What::Page {
                message: "Sign in".into(),
                url: "https://example.com/login?x=1".into()
            })
        );
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "not a url",
            "https://",
        ] {
            assert!(
                parse(&json!({"mode": "url", "message": "m", "url": url})).is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn a_value_is_checked_before_it_is_sent() {
        let form = form_of(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 2, "maxLength": 5},
                "mail": {"type": "string", "format": "email"},
                "day": {"type": "string", "format": "date"},
                "age": {"type": "integer", "minimum": 0, "maximum": 150},
                "rate": {"type": "number"},
                "tags": {"type": "array", "items": {"enum": ["a", "b", "c"]}, "minItems": 1, "maxItems": 2},
            },
            "required": ["name"],
        }));
        let field = |name: &str| form.fields.iter().find(|field| field.name == name).unwrap();
        let text = |text: &str| Input::Text(text.into());
        assert_eq!(field("name").check(&text("")), Err(Problem::Required));
        assert_eq!(field("name").check(&text("a")), Err(Problem::TooShort(2)));
        assert_eq!(
            field("name").check(&text("abcdef")),
            Err(Problem::TooLong(5))
        );
        assert_eq!(field("name").check(&text(" ab ")), Ok(Some(json!("ab"))));
        assert_eq!(
            field("mail").check(&text("")),
            Ok(None),
            "an optional field may stay empty"
        );
        assert_eq!(field("mail").check(&text("a@b")), Err(Problem::NotEmail));
        assert_eq!(
            field("mail").check(&text("a@b.de")),
            Ok(Some(json!("a@b.de")))
        );
        assert_eq!(
            field("day").check(&text("2026-13-01")),
            Err(Problem::NotDate)
        );
        assert_eq!(
            field("day").check(&text("2026-10-29")),
            Ok(Some(json!("2026-10-29")))
        );
        assert_eq!(field("age").check(&text("1.5")), Err(Problem::NotInteger));
        assert_eq!(field("age").check(&text("42.0")), Ok(Some(json!(42))));
        assert_eq!(field("age").check(&text("-1")), Err(Problem::Below(0.0)));
        assert_eq!(field("age").check(&text("42")), Ok(Some(json!(42))));
        assert_eq!(field("rate").check(&text("NaN")), Err(Problem::NotNumber));
        assert_eq!(field("rate").check(&text("0.5")), Ok(Some(json!(0.5))));
        assert_eq!(
            field("tags").check(&Input::Choices(vec![false; 3])),
            Ok(None)
        );
        assert_eq!(
            field("tags").check(&Input::Choices(vec![true, true, true])),
            Err(Problem::TooMany(2))
        );
        assert_eq!(
            field("tags").check(&Input::Choices(vec![false, true, false])),
            Ok(Some(json!(["b"])))
        );
        let inputs: Vec<Input> = form.fields.iter().map(Field::start).collect();
        let name_at = form
            .fields
            .iter()
            .position(|field| field.name == "name")
            .unwrap();
        assert_eq!(form.answer(&inputs), Err((name_at, Problem::Required)));
    }

    #[test]
    fn a_reply_is_the_protocols_result() {
        let mut content = Map::new();
        content.insert("name".into(), json!("Ada"));
        assert_eq!(
            Reply::Accept(content).result(),
            json!({"action": "accept", "content": {"name": "Ada"}})
        );
        assert_eq!(
            Reply::Accept(Map::new()).result(),
            json!({"action": "accept"})
        );
        assert_eq!(Reply::Decline.result(), json!({"action": "decline"}));
        assert_eq!(Reply::Cancel.result(), json!({"action": "cancel"}));
    }
}
