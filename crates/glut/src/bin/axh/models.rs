//! `axh models` (`docs/plans/glut-providers.md`, PV2, P6): the models each configured provider offers, from its kept
//! list (`~/.axh/models/<id>.json`), fetched again when it is older than a day or on `refresh`.

use std::path::Path;
use std::process::ExitCode;

use clap::Subcommand;
use glut::providers::models::{self, ModelInfo, ModelList};
use glut::providers::{self, Providers};

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// The models of one provider or of all, with context window, price per million tokens and whether they reason.
    List {
        /// Only this provider's.
        #[arg(long)]
        provider: Option<String>,
        /// Fetch the lists again first, however fresh they are.
        #[arg(long)]
        refresh: bool,
        /// Only models whose id or name holds every word (`deepseek flash`).
        query: Vec<String>,
    },
    /// Fetches the lists again: of one provider, or of all.
    Refresh {
        #[arg(long)]
        provider: Option<String>,
    },
}

/// Runs one `axh models` command.
pub async fn run(command: ModelsCommand) -> ExitCode {
    let home = match glut::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    let outcome = match command {
        ModelsCommand::List {
            provider,
            refresh,
            query,
        } => list(&home, provider.as_deref(), refresh, &query.join(" ")).await,
        ModelsCommand::Refresh { provider } => refresh(&home, provider.as_deref()).await,
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("axh models: {message}");
    ExitCode::from(1)
}

/// The providers a command covers: the one named, or every configured one.
fn chosen(providers: &Providers, id: Option<&str>) -> Result<Vec<String>, String> {
    match id {
        Some(id) => providers.pick(Some(id)).map(|(id, _)| vec![id.to_owned()]),
        None if providers.providers.is_empty() => Err(
            "no provider yet: axh providers add <kind> (axh providers catalog lists them)".into(),
        ),
        None => Ok(providers.providers.keys().cloned().collect()),
    }
}

/// The list of provider `id`: fresh from the provider when `again`, else the kept one while it is fresh.
async fn list_of(
    home: &Path,
    providers: &Providers,
    id: &str,
    again: bool,
) -> Result<ModelList, String> {
    let store = providers.store(home).map_err(|err| err.to_string())?;
    // The model only fills the endpoint: a list is the provider's, not a model's.
    let resolved = providers::resolve(providers, store.as_ref(), Some(id), Some("-"), |name| {
        std::env::var(name).ok()
    })?;
    if again {
        let list = models::fetch(&resolved.endpoint, resolved.protocol).await?;
        models::keep(home, id, &list).map_err(|err| err.to_string())?;
        return Ok(list);
    }
    models::current(home, id, &resolved.endpoint, resolved.protocol).await
}

async fn list(home: &Path, provider: Option<&str>, again: bool, query: &str) -> Result<(), String> {
    let providers = Providers::load(home).map_err(|err| err.to_string())?;
    let ids = chosen(&providers, provider)?;
    let mut failed = Vec::new();
    for id in &ids {
        match list_of(home, &providers, id, again).await {
            Ok(list) => {
                let shown: Vec<&ModelInfo> = list.matching(query).collect();
                println!(
                    "{id} — {} of {} models (fetched {})",
                    shown.len(),
                    list.models.len(),
                    list.fetched_at
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                );
                for model in shown {
                    println!("  {}", row(model));
                }
            }
            Err(reason) => failed.push(format!("{id}: {reason}")),
        }
    }
    report(failed, ids.len())
}

async fn refresh(home: &Path, provider: Option<&str>) -> Result<(), String> {
    let providers = Providers::load(home).map_err(|err| err.to_string())?;
    let ids = chosen(&providers, provider)?;
    let mut failed = Vec::new();
    for id in &ids {
        match list_of(home, &providers, id, true).await {
            Ok(list) => println!("{id}: {} models", list.models.len()),
            Err(reason) => failed.push(format!("{id}: {reason}")),
        }
    }
    report(failed, ids.len())
}

/// One provider that failed among several is said and the rest still shown; only when all failed is it an error.
fn report(failed: Vec<String>, of: usize) -> Result<(), String> {
    for line in &failed {
        eprintln!("axh models: {line}");
    }
    if !failed.is_empty() && failed.len() == of {
        return Err("no list could be read".into());
    }
    Ok(())
}

/// A context window as people say it, in binary thousands and millions: `160k`, `1M`, `1.9M`.
fn window(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        let millions = f64::from(tokens) / 1_048_576.0;
        let text = format!("{millions:.1}");
        format!("{}M", text.trim_end_matches(".0"))
    } else {
        format!("{}k", tokens / 1024)
    }
}

/// A model as one line: id, window, prices, reasoning.
fn row(model: &ModelInfo) -> String {
    let window = model
        .context_window
        .map(window)
        .unwrap_or_else(|| "?".into());
    let prices = model
        .prices
        .map(|prices| format!("${:.2} / ${:.2}", prices.input_per_m, prices.output_per_m))
        .unwrap_or_default();
    let reasons = if model.reasoning == Some(true) {
        "reasons"
    } else {
        ""
    };
    format!(
        "{:<48} {window:>6}  {prices:<18} {reasons}",
        models::printable(&model.id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_what_the_list_knows_and_a_mark_for_what_it_does_not() {
        let mut model = ModelInfo {
            id: "deepseek/deepseek-v4.1-flash".into(),
            name: None,
            context_window: Some(163_840),
            prices: Some(glut::Prices {
                input_per_m: 0.1,
                cached_input_per_m: None,
                output_per_m: 0.4,
            }),
            reasoning: Some(true),
            created: None,
            api: None,
        };
        assert_eq!(
            (window(1_048_576), window(2_000_000), window(131_072)),
            ("1M".to_owned(), "1.9M".to_owned(), "128k".to_owned())
        );
        let line = row(&model);
        assert!(
            line.contains("160k") && line.contains("$0.10 / $0.40") && line.ends_with("reasons"),
            "{line}"
        );
        model.context_window = None;
        model.prices = None;
        model.reasoning = None;
        assert!(row(&model).contains("?"));
        assert!(
            report(vec!["a: down".into()], 2).is_ok(),
            "one of two failed"
        );
        assert!(report(vec!["a: down".into()], 1).is_err());
    }
}
