//! Code mode (Glut 2, A, G2–G9): the model writes a short JavaScript that calls the run's tools as functions, and only
//! what the script prints or returns comes back — a large result is filtered inside the script instead of riding along
//! in every later step. After Pi's `codemode` (pi.dev/docs/latest/codemode), not a copy of it.
//!
//! * **QuickJS through `rquickjs`** (G2): an async function body, so `await` and `return` work at the top. No Node, no
//!   `fetch`, no `require`, no file system, no timers — the only way out is `tools.<name>(args)`.
//! * **Every call from a script is a call of the run** (`run.rs`): the script's `tools.x(args)` sends a request over a
//!   channel, and the loop answers it on the same path as a direct call — the gate, the hooks, the events, the cap.
//!   Nothing here can call a tool on its own.
//! * **A script that waits for nothing** — a promise no call will ever settle — is ended at once: the loop sees the
//!   script pending with no request open (there are no timers that could still settle it).
//! * **Limits (G9)**: 256 MB per script, two scripts at a time per process, no time or call limit of its own — the run's
//!   deadline and its cancel interrupt a script, a busy loop included.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rquickjs::context::intrinsic;
use rquickjs::prelude::Async;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Promise, Value as JsValue,
};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::history::ResultStore;
use crate::model::ToolSpec;

/// The tool's name.
pub(crate) const NAME: &str = "codemode";

/// Memory one script may use (G9, as Pi).
const MEMORY_LIMIT_BYTES: usize = 256 * 1024 * 1024;
/// Scripts running at once in this process (G9, as Pi): each holds a whole JavaScript runtime.
const MAX_SCRIPTS: usize = 2;
/// What a script's output may hand back, in characters (~10k tokens, G9); beyond it head and tail stay, the whole goes
/// into the run's result store.
pub(crate) const MAX_OUTPUT_CHARS: usize = 40_000;
/// What the tool's description may spend on the tools' declarations, in characters (~3k tokens, as Pi's budget).
const DECLARATION_BUDGET_CHARS: usize = 12_000;
/// Most hits `searchTools` returns.
const MAX_SEARCH_HITS: usize = 20;
/// Up to this many tools behind code are declared in full; more are summed up per server and found by search.
const DECLARE_BEHIND_MAX: usize = 10;
/// `store()`: one value, and all values of a run together, in characters (as Pi).
const MAX_STORED_VALUE_CHARS: usize = 262_144;
const MAX_STORED_TOTAL_CHARS: usize = 1_048_576;
/// `store()`: a key's length and how many keys a run keeps.
const MAX_STORED_KEY_CHARS: usize = 200;
const MAX_STORED_KEYS: usize = 1_000;
/// Largest arguments of one tool call from a script: a call's arguments are text the loop holds while it works.
const MAX_ARGUMENT_BYTES: usize = 1_048_576;

static SCRIPTS: Semaphore = Semaphore::const_new(MAX_SCRIPTS);

/// The JavaScript a script gets: everything QuickJS offers **but regular expressions** (neither `RegExp` nor the
/// compiler regex literals need). The bundled quickjs-ng has an open advisory, GHSA-rxpx-wj25-jhqq (2026-07): compiling
/// a regex after an out-of-memory writes past its buffer — in this process, from a script a prompt can steer. No fixed
/// release exists yet (rquickjs 0.14.0, quickjs-ng snapshot ~0.16.2); bring them back once one does. A script uses the
/// string methods instead.
type Intrinsics = (
    intrinsic::Date,
    intrinsic::Eval,
    intrinsic::Json,
    intrinsic::Proxy,
    intrinsic::MapSet,
    intrinsic::TypedArrays,
    intrinsic::Promise,
    intrinsic::WeakRef,
    intrinsic::DOMException,
);

/// One of the [`MAX_SCRIPTS`] places to run a script, held while it runs. Taken by the caller before [`run`] starts:
/// a script waiting for a place is no script waiting on nothing (the loop's stuck check).
pub(crate) async fn slot() -> Option<tokio::sync::SemaphorePermit<'static>> {
    SCRIPTS.acquire().await.ok()
}

/// The line that stands for the output dropped past [`MAX_BUFFERED_CHARS`].
const DROPPED: &str = "[… more output dropped: the script printed too much]";

/// What a script's lines may add up to before the rest is dropped, in characters: the model gets at most
/// [`MAX_OUTPUT_CHARS`] anyway, and a loop printing forever must not fill the memory outside QuickJS's limit.
const MAX_BUFFERED_CHARS: usize = 10 * MAX_OUTPUT_CHARS;

/// One call a script makes: the tool, its arguments as JSON text, and where the answer goes.
pub(crate) struct Request {
    pub name: String,
    pub arguments: String,
    pub reply: oneshot::Sender<(bool, String)>,
}

/// What the script can find: every tool the run offers it, with its spec.
#[derive(Debug, Clone, Default)]
pub(crate) struct Catalog {
    pub specs: Vec<ToolSpec>,
}

impl Catalog {
    fn search(&self, query: &str) -> Value {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let hits: Vec<Value> = self
            .specs
            .iter()
            .filter(|spec| {
                let text = format!("{} {}", spec.name, spec.description).to_lowercase();
                words.iter().all(|w| text.contains(w))
            })
            .take(MAX_SEARCH_HITS)
            .map(|spec| json!({ "name": spec.name, "description": first_sentence(&spec.description) }))
            .collect();
        Value::Array(hits)
    }

    fn describe(&self, name: &str) -> Value {
        self.specs
            .iter()
            .find(|spec| spec.name == name)
            .map_or(Value::Null, |spec| {
                json!({ "name": spec.name, "description": spec.description, "parameters": spec.parameters })
            })
    }

    fn names(&self) -> Value {
        Value::Array(
            self.specs
                .iter()
                .map(|s| Value::String(s.name.clone()))
                .collect(),
        )
    }
}

/// The values a run's scripts keep with `store()`; a script's writes count only when it succeeds.
pub(crate) type Kept = Arc<Mutex<HashMap<String, String>>>;

/// What stops a script from outside: the run's stop flag, its cancel, its deadline.
pub(crate) struct Interrupt {
    pub stop: Arc<AtomicBool>,
    pub cancelled: Box<dyn Fn() -> bool + Send + Sync>,
    pub deadline: Instant,
}

/// How a script ended: whether it succeeded, and the text for the model.
pub(crate) struct Ended {
    pub ok: bool,
    pub text: String,
}

/// The spec the model sees: how to write a script, the direct tools as declarations, and the servers whose tools are
/// reached only from a script (G3).
pub(crate) fn spec(direct: &[ToolSpec], deferred: &[ToolSpec]) -> ToolSpec {
    let mut description = String::from(
        "Run JavaScript — the body of an async function: use `await` and `return` — in a sandbox without file \
         system, network or timers. Call tools as `await tools.<name>({...args})`: each resolves to the tool's text \
         output as text — print it as it is, do not JSON.stringify it again; JSON.parse it when it is JSON — and rejects with an Error when the call fails or is refused. Only what \
         you print (`text(...)`, `console.log(...)`) and `return` comes back to you — filter and summarize large \
         results inside the script, and run independent calls together with `Promise.all`/`Promise.allSettled`. \
         `store(key, value)` / `load(key)` keep small JSON values for later scripts of this run. `searchTools(query)` \
         → [{name, description}], `describeTool(name)` → its argument schema, `ALL_TOOLS` → every name. A tool's text \
         that starts with `error: ` counts as its failure. Regular expressions are not available — use the string \
         methods. A failed call stays done; a script cannot start another \
         script or a subagent.",
    );
    let direct_declared = declarations(direct);
    if !direct_declared.is_empty() {
        description.push_str("\n\nTools:\n");
        description.push_str(&direct_declared);
    }
    if deferred.len() <= DECLARE_BEHIND_MAX {
        // A few, as a skill that declares its MCP tools has them: named exactly, no search step (measured 2026-10-20 —
        // a model guessed `tools.calendar_calendars` and spent a turn on searchTools()).
        let declared = declarations(deferred);
        if !declared.is_empty() {
            description.push_str("\n\nTools reached only from a script:\n");
            description.push_str(&declared);
        }
    } else {
        description.push_str(
            "\n\nMore tools, reached only from a script — find them with searchTools():\n",
        );
        description.push_str(&servers(deferred));
    }
    ToolSpec {
        name: NAME.into(),
        description,
        parameters: json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "The JavaScript to run (raw source, no markdown fence)." }
            },
            "required": ["code"]
        }),
    }
}

/// `tools.read_file(args: {path: string, offset?: number}): Promise<string> // first sentence` per tool, within the
/// budget; past it only the names.
fn declarations(specs: &[ToolSpec]) -> String {
    let mut out = String::new();
    let mut rest = Vec::new();
    for spec in specs {
        let line = format!(
            "tools.{}(args: {}): Promise<string> // {}\n",
            spec.name,
            type_of(&spec.parameters),
            first_sentence(&spec.description)
        );
        if out.len() + line.len() > DECLARATION_BUDGET_CHARS {
            rest.push(spec.name.as_str());
        } else {
            out.push_str(&line);
        }
    }
    if !rest.is_empty() {
        out.push_str(&format!(
            "(also: {} — describeTool() gives their arguments)\n",
            rest.join(", ")
        ));
    }
    out
}

/// One line per MCP server: its name, how many tools, a few of them.
fn servers(specs: &[ToolSpec]) -> String {
    let mut by_server: Vec<(String, Vec<&str>)> = Vec::new();
    for spec in specs {
        let server = spec
            .name
            .strip_prefix("mcp__")
            .and_then(|rest| rest.split_once("__"))
            .map_or("other", |(server, _)| server)
            .to_owned();
        match by_server.iter_mut().find(|(s, _)| *s == server) {
            Some((_, names)) => names.push(&spec.name),
            None => by_server.push((server, vec![&spec.name])),
        }
    }
    by_server
        .iter()
        .map(|(server, names)| {
            let shown: Vec<&str> = names.iter().take(4).copied().collect();
            format!(
                "{server}: {} tools (e.g. {})\n",
                names.len(),
                shown.join(", ")
            )
        })
        .collect()
}

/// A JSON schema as a TypeScript-like type, one level deep.
fn type_of(schema: &Value) -> String {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return "{}".into();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    // Sorted: `serde_json` keeps the schema's order or not depending on a feature another crate may switch on, and
    // the description must not change between builds (the prompt's cached head).
    let mut properties: Vec<(&String, &Value)> = properties.iter().collect();
    properties.sort_by(|a, b| a.0.cmp(b.0));
    let fields: Vec<String> = properties
        .into_iter()
        .map(|(name, prop)| {
            let optional = if required.contains(&name.as_str()) {
                ""
            } else {
                "?"
            };
            let kind = match prop.get("type").and_then(Value::as_str) {
                Some("integer") => "number",
                Some("array") => "any[]",
                Some("object") => "object",
                Some(other) => other,
                None => "any",
            };
            format!("{name}{optional}: {kind}")
        })
        .collect();
    format!("{{{}}}", fields.join(", "))
}

fn first_sentence(text: &str) -> &str {
    let end = text.find(". ").map_or(text.len(), |i| i + 1);
    text[..end].trim()
}

/// The script's source from the call's arguments.
pub(crate) fn code_of(arguments: &str) -> Result<String, String> {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(args)) => match args.get("code").and_then(Value::as_str) {
            Some(code) if !code.trim().is_empty() => Ok(strip_fence(code).to_owned()),
            _ => Err("`code` (the JavaScript to run) is required".into()),
        },
        Ok(_) => Err("the arguments must be a JSON object".into()),
        Err(err) => Err(format!("the arguments are not valid JSON: {err}")),
    }
}

/// A model that wraps the code in a markdown fence anyway is forgiven.
fn strip_fence(code: &str) -> &str {
    let trimmed = code.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return code;
    };
    let body = rest.split_once('\n').map_or("", |(_, body)| body);
    body.trim_end()
        .strip_suffix("```")
        .unwrap_or(body)
        .trim_end()
}

/// The JavaScript every script runs behind: the globals, built on the few Rust functions below.
const PRELUDE: &str = r#"
globalThis.__str = (v) => typeof v === "string" ? v : (v === undefined ? "undefined" : (JSON.stringify(v, null, 2) ?? String(v)));
globalThis.text = (...a) => __emit(a.map(__str).join(" "));
globalThis.console = Object.freeze({
  log: (...a) => __emit(a.map(__str).join(" ")),
  info: (...a) => __emit(a.map(__str).join(" ")),
  warn: (...a) => __emit("warn: " + a.map(__str).join(" ")),
  error: (...a) => __emit("error: " + a.map(__str).join(" ")),
});
globalThis.tools = new Proxy({}, {
  get(_, name) {
    if (typeof name !== "string") return undefined;
    return async (args) => {
      const [ok, out] = JSON.parse(await __call(name, JSON.stringify(args ?? {})));
      if (!ok) throw new Error(out);
      return out;
    };
  },
});
globalThis.searchTools = (q) => JSON.parse(__search(String(q ?? "")));
globalThis.describeTool = (n) => JSON.parse(__describe(String(n)));
globalThis.ALL_TOOLS = Object.freeze(JSON.parse(__names()));
globalThis.store = (k, v) => { __store(String(k), JSON.stringify(v ?? null)); };
globalThis.load = (k) => { const s = __load(String(k)); return s == null ? undefined : JSON.parse(s); };
"#;

/// Runs `code`; every tool call goes out through `requests` and is answered by the caller. Returns how it ended.
pub(crate) async fn run(
    code: String,
    catalog: Arc<Catalog>,
    kept: Kept,
    store: ResultStore,
    interrupt: Interrupt,
    requests: mpsc::Sender<Request>,
) -> Ended {
    let runtime = match AsyncRuntime::new() {
        Ok(runtime) => runtime,
        Err(err) => return failed(format!("the script runtime could not start: {err}")),
    };
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES).await;
    runtime
        .set_interrupt_handler(Some(Box::new(move || {
            interrupt.stop.load(Ordering::SeqCst)
                || (interrupt.cancelled)()
                || Instant::now() >= interrupt.deadline
        })))
        .await;
    let context = match AsyncContext::custom::<Intrinsics>(&runtime).await {
        Ok(context) => context,
        Err(err) => return failed(format!("the script runtime could not start: {err}")),
    };
    let output: Arc<Mutex<Vec<String>>> = Arc::default();
    let staged: Kept = Arc::new(Mutex::new(
        kept.lock().unwrap_or_else(|p| p.into_inner()).clone(),
    ));
    let result = {
        let output = Arc::clone(&output);
        let staged = Arc::clone(&staged);
        context
            .async_with(async move |ctx| {
                evaluate(ctx, code, catalog, output, staged, requests).await
            })
            .await
    };
    let mut text = output.lock().unwrap_or_else(|p| p.into_inner()).join("\n");
    let ok = match result {
        Ok(Some(returned)) => {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&returned);
            true
        }
        Ok(None) => true,
        Err(error) => {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!("Script error: {error}"));
            false
        }
    };
    if ok {
        *kept.lock().unwrap_or_else(|p| p.into_inner()) =
            staged.lock().unwrap_or_else(|p| p.into_inner()).clone();
    }
    if text.trim().is_empty() {
        text = "(the script printed and returned nothing)".into();
    }
    Ended {
        ok,
        text: bounded(text, &store),
    }
}

fn failed(reason: String) -> Ended {
    Ended {
        ok: false,
        text: format!("error: {reason}"),
    }
}

/// The script, run: `Ok(Some(text))` for a returned value, `Ok(None)` for none, `Err` with the error's message.
async fn evaluate<'js>(
    ctx: Ctx<'js>,
    code: String,
    catalog: Arc<Catalog>,
    output: Arc<Mutex<Vec<String>>>,
    kept: Kept,
    requests: mpsc::Sender<Request>,
) -> Result<Option<String>, String> {
    install(&ctx, catalog, output, kept, requests)
        .map_err(|err| format!("the script runtime could not start: {err}"))?;
    let source = format!("(async () => {{\n{code}\n}})()");
    let promise: Promise = ctx
        .eval(source)
        .catch(&ctx)
        .map_err(|err| err.to_string())?;
    let value: JsValue = promise
        .into_future()
        .await
        .catch(&ctx)
        .map_err(|err| err.to_string())?;
    if value.is_undefined() {
        return Ok(None);
    }
    let shown: Function = ctx.globals().get("__str").map_err(|err| err.to_string())?;
    let text: String = shown
        .call((value,))
        .catch(&ctx)
        .map_err(|err| err.to_string())?;
    Ok(Some(text))
}

/// The Rust side of the globals: emit, call, search, describe, names, store, load — then the prelude on top.
fn install<'js>(
    ctx: &Ctx<'js>,
    catalog: Arc<Catalog>,
    output: Arc<Mutex<Vec<String>>>,
    kept: Kept,
    requests: mpsc::Sender<Request>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    globals.set(
        "__emit",
        Function::new(ctx.clone(), move |line: String| {
            let mut lines = output.lock().unwrap_or_else(|p| p.into_inner());
            let buffered: usize = lines.iter().map(String::len).sum();
            if buffered + line.len() <= MAX_BUFFERED_CHARS {
                lines.push(line);
            } else if lines.last().is_none_or(|last| last != DROPPED) {
                lines.push(DROPPED.to_owned());
            }
        })?,
    )?;
    globals.set(
        "__call",
        Function::new(
            ctx.clone(),
            Async(move |name: String, arguments: String| {
                let requests = requests.clone();
                async move {
                    if arguments.len() > MAX_ARGUMENT_BYTES {
                        let refused =
                            format!("the arguments are larger than {MAX_ARGUMENT_BYTES} bytes");
                        return Ok::<String, rquickjs::Error>(json!([false, refused]).to_string());
                    }
                    let (reply, answer) = oneshot::channel();
                    let answered = match requests
                        .send(Request {
                            name,
                            arguments,
                            reply,
                        })
                        .await
                    {
                        Ok(()) => answer.await.unwrap_or((false, "the run ended".into())),
                        Err(_) => (false, "the run ended".into()),
                    };
                    Ok::<String, rquickjs::Error>(json!([answered.0, answered.1]).to_string())
                }
            }),
        )?,
    )?;
    let search = Arc::clone(&catalog);
    globals.set(
        "__search",
        Function::new(ctx.clone(), move |query: String| {
            search.search(&query).to_string()
        })?,
    )?;
    let describe = Arc::clone(&catalog);
    globals.set(
        "__describe",
        Function::new(ctx.clone(), move |name: String| {
            describe.describe(&name).to_string()
        })?,
    )?;
    globals.set(
        "__names",
        Function::new(ctx.clone(), move || catalog.names().to_string())?,
    )?;
    let writes = Arc::clone(&kept);
    globals.set(
        "__store",
        Function::new(
            ctx.clone(),
            move |key: String, value: String| -> rquickjs::Result<()> {
                let mut map = writes.lock().unwrap_or_else(|p| p.into_inner());
                if value.chars().count() > MAX_STORED_VALUE_CHARS {
                    return Err(rquickjs::Error::new_from_js_message(
                        "value",
                        "store",
                        "the value is too large",
                    ));
                }
                let new_key = !map.contains_key(&key);
                if key.chars().count() > MAX_STORED_KEY_CHARS
                    || (new_key && map.len() >= MAX_STORED_KEYS)
                {
                    return Err(rquickjs::Error::new_from_js_message(
                        "key",
                        "store",
                        "too long a key, or too many keys",
                    ));
                }
                // Keys count too: many large keys with empty values would grow the map as well.
                let total: usize = map
                    .iter()
                    .filter(|(k, _)| **k != key)
                    .map(|(k, v)| k.chars().count() + v.chars().count())
                    .sum();
                if total + key.chars().count() + value.chars().count() > MAX_STORED_TOTAL_CHARS {
                    return Err(rquickjs::Error::new_from_js_message(
                        "value",
                        "store",
                        "the store is full",
                    ));
                }
                map.insert(key, value);
                Ok(())
            },
        )?,
    )?;
    globals.set(
        "__load",
        Function::new(ctx.clone(), move |key: String| -> Option<String> {
            kept.lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&key)
                .cloned()
        })?,
    )?;
    ctx.eval::<(), _>(PRELUDE)
}

/// The output as the model gets it: whole up to [`MAX_OUTPUT_CHARS`]; beyond, head and tail and the whole in `store`.
pub(crate) fn bounded(text: String, store: &ResultStore) -> String {
    let total = text.chars().count();
    if total <= MAX_OUTPUT_CHARS {
        return text;
    }
    let half = MAX_OUTPUT_CHARS / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(total - half).collect();
    let id = store.put(text);
    format!(
        "{head}\n\n[… {} characters left out of the middle; `{}` with id \"{id}\" gives the whole output …]\n\n{tail}",
        total - 2 * half,
        crate::history::NAME
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_of(name: &str, description: &str, parameters: Value) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    fn interrupt() -> Interrupt {
        Interrupt {
            stop: Arc::default(),
            cancelled: Box::new(|| false),
            deadline: Instant::now() + std::time::Duration::from_secs(30),
        }
    }

    /// Runs `code` with the tool `echo` answered here, as the loop would answer it.
    async fn script(code: &str, kept: Kept) -> Ended {
        let (tx, mut rx) = mpsc::channel::<Request>(8);
        let catalog = Arc::new(Catalog {
            specs: vec![spec_of(
                "mcp__srv__echo",
                "Echoes text. More words.",
                json!({"type": "object"}),
            )],
        });
        let answering = tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                let args: Value = serde_json::from_str(&request.arguments).unwrap();
                let reply = match request.name.as_str() {
                    "echo" => (
                        true,
                        format!("echo:{}", args["text"].as_str().unwrap_or("")),
                    ),
                    other => (false, format!("not permitted: {other}")),
                };
                let _ = request.reply.send(reply);
            }
        });
        let ended = run(
            code.into(),
            catalog,
            kept,
            ResultStore::new("t"),
            interrupt(),
            tx,
        )
        .await;
        answering.abort();
        ended
    }

    #[tokio::test]
    async fn tools_are_awaited_in_parallel_and_only_the_output_comes_back() {
        let ended = script(
            r#"const [a, b] = await Promise.all([tools.echo({text: "x"}), tools.echo({text: "y"})]);
               console.log("got", a);
               text(b);
               return {n: 2};"#,
            Kept::default(),
        )
        .await;
        assert!(ended.ok, "{}", ended.text);
        assert_eq!(ended.text, "got echo:x\necho:y\n{\n  \"n\": 2\n}");
    }

    #[tokio::test]
    async fn a_refused_call_rejects_and_a_failed_script_keeps_its_partial_output() {
        let ended = script(
            r#"text("before");
               const settled = await Promise.allSettled([tools.nope({})]);
               text(settled[0].status, settled[0].reason.message);
               throw new Error("boom");"#,
            Kept::default(),
        )
        .await;
        assert!(!ended.ok);
        assert!(
            ended
                .text
                .starts_with("before\nrejected not permitted: nope"),
            "{}",
            ended.text
        );
        assert!(
            ended.text.contains("Script error: Error: boom"),
            "{}",
            ended.text
        );
    }

    #[tokio::test]
    async fn the_sandbox_has_no_way_out_but_the_tools() {
        let ended = script(
            "return [typeof fetch, typeof require, typeof setTimeout, typeof process, ALL_TOOLS.length].join(',')",
            Kept::default(),
        )
        .await;
        assert_eq!(ended.text, "undefined,undefined,undefined,undefined,1");
        let found = script(
            "return searchTools('ECHO text')[0].description",
            Kept::default(),
        )
        .await;
        assert_eq!(found.text, "Echoes text.");
    }

    #[tokio::test]
    async fn regular_expressions_are_not_there_but_the_string_methods_are() {
        let literal = script("return /a/.test('a')", Kept::default()).await;
        assert!(!literal.ok, "{}", literal.text);
        let constructed = script("return new RegExp('a').test('a')", Kept::default()).await;
        assert!(
            !constructed.ok && constructed.text.contains("RegExp"),
            "{}",
            constructed.text
        );
        let strings = script(
            "return ['a,b'.split(',').length, 'xay'.includes('a'), 'a-b'.replaceAll('-', '+')].join(' ')",
            Kept::default(),
        )
        .await;
        assert_eq!(strings.text, "2 true a+b");
    }

    #[tokio::test]
    async fn store_keeps_values_only_from_scripts_that_succeed() {
        let kept = Kept::default();
        assert!(
            script("store('a', {v: 1}); return load('a').v", Arc::clone(&kept))
                .await
                .ok
        );
        let failed = script("store('b', 2); throw new Error('no')", Arc::clone(&kept)).await;
        assert!(!failed.ok);
        let read = script("return [load('a').v, load('b')].join(',')", kept).await;
        assert_eq!(read.text, "1,");
    }

    #[tokio::test]
    async fn a_busy_loop_ends_at_the_deadline() {
        let (tx, _rx) = mpsc::channel(8);
        let ended = run(
            "while (true) {}".into(),
            Arc::default(),
            Kept::default(),
            ResultStore::new("t"),
            Interrupt {
                stop: Arc::default(),
                cancelled: Box::new(|| false),
                deadline: Instant::now() + std::time::Duration::from_millis(200),
            },
            tx,
        )
        .await;
        assert!(
            !ended.ok && ended.text.contains("interrupted"),
            "{}",
            ended.text
        );
    }

    #[test]
    fn long_output_keeps_head_and_tail_and_the_whole_in_the_store() {
        let store = ResultStore::new("s");
        let text = format!("{}{}", "h".repeat(MAX_OUTPUT_CHARS), "t".repeat(100));
        let shown = bounded(text.clone(), &store);
        assert!(shown.starts_with('h') && shown.ends_with('t') && shown.contains("id \"s-1\""));
        assert!(store.read("s-1", 0).unwrap().starts_with(&"h".repeat(100)));
    }

    #[test]
    fn the_description_declares_direct_tools_and_names_the_servers_behind_code() {
        let direct = [spec_of(
            "read_file",
            "Read a file. Long ones in parts.",
            json!({"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer"}},"required":["path"]}),
        )];
        let deferred = [
            spec_of("mcp__apple_mail__search", "Search mail.", json!({})),
            spec_of("mcp__apple_mail__move", "Move mail.", json!({})),
        ];
        let spec = spec(&direct, &deferred);
        assert!(spec.description.contains("tools.read_file(args: {offset?: number, path: string}): Promise<string> // Read a file."));
        // Two tools behind code are declared in full; twelve are summed up per server.
        assert!(spec.description.contains(
            "Tools reached only from a script:\ntools.mcp__apple_mail__search(args: {}): Promise<string> // Search mail."
        ));
        let many: Vec<ToolSpec> = (0..12)
            .map(|i| spec_of(&format!("mcp__apple_mail__t{i}"), "T.", json!({})))
            .collect();
        assert!(
            super::spec(&direct, &many)
                .description
                .contains("apple_mail: 12 tools (e.g. mcp__apple_mail__t0")
        );
        assert_eq!(
            code_of(r#"{"code":"```js\nreturn 1\n```"}"#).unwrap(),
            "return 1"
        );
        assert!(code_of(r#"{"code":"  "}"#).is_err());
    }
}
