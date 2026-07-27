//! `roadmark` — CLI for the `.roadmap/` source-of-truth pipeline.
//!
//! Subcommands:
//! - `generate`: render `ROADMAP.md` to stdout, or atomically to `--output`
//! - `validate`: schema, slug uniqueness, anchor drift
//! - `add`: scaffold a new feature file
//! - `import`: bootstrap `.roadmap/` from a hand-written ROADMAP.md
//! - `rename`: rename a slug, moving the file and rewriting cross-links

use anyhow::{Context, Result};
use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "roadmark",
    version,
    about = "ROADMAP.md generator from .roadmap/ frontmatter source"
)]
struct Cli {
    /// Path to the `.roadmap/` directory. Defaults to `./.roadmap`.
    #[arg(long, global = true, default_value = ".roadmap")]
    root: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new feature file from a template.
    Add {
        /// Slug for the new feature (matches the filename without `.md`).
        /// Must be `f-<kebab-name>`. The legacy `f<digits>` form is
        /// rejected unless `--allow-legacy-numeric` is set.
        slug: String,
        /// Allow the legacy `f<digits>` slug shape (e.g. `f139`).
        /// Migration-only — emits a deprecation warning.
        #[arg(long)]
        allow_legacy_numeric: bool,
    },
    /// Generate ROADMAP.md from `.roadmap/` source. Writes to stdout
    /// unless `--output` is given.
    Generate {
        /// Write to this file instead of stdout, via a temp file and a
        /// rename — a failed run leaves the previous file untouched.
        /// Prefer this over `generate > ROADMAP.md`, which has the shell
        /// empty the destination before roadmark runs.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Bootstrap `.roadmap/` from an existing hand-written ROADMAP.md.
    Import {
        /// The hand-written roadmap to read.
        source: PathBuf,
        /// Report what would be written and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Map one of roadmark's fields onto a source column header,
        /// e.g. `--map area=Direction`. Repeatable. Without it, headers
        /// are matched by name and by a short alias list.
        #[arg(long, value_name = "FIELD=HEADER")]
        map: Vec<String>,
    },
    /// Validate the `.roadmap/` source: schema, slug uniqueness, anchor drift.
    Validate {
        /// Path to the on-disk `ROADMAP.md` to diff anchors against.
        #[arg(long, default_value = "ROADMAP.md")]
        roadmap_md: PathBuf,
        /// Treat anchor drift as a warning instead of a failure.
        /// Schema errors and slug collisions still fail the run.
        #[arg(long)]
        accept_drift: bool,
    },
    /// Rename a feature slug: move the file, update its id, and rewrite
    /// cross-references in every feature body.
    Rename {
        /// Current slug (matches the filename without `.md`).
        from: String,
        /// New slug. Must be `f-<kebab-name>` unless `--allow-legacy-numeric`.
        to: String,
        /// Allow the legacy `f<digits>` slug shape as the target.
        /// Migration-only — emits a deprecation warning.
        #[arg(long)]
        allow_legacy_numeric: bool,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        },
    }
}

fn run() -> Result<ExitCode> {
    // Parse via `ArgMatches` rather than `Cli::parse()` so `validate` can
    // ask whether `--root` was *typed* or defaulted — the derived struct
    // holds the value but not its provenance, and the two mean opposite
    // things when the tree is missing (see `validate::validate`).
    let matches = Cli::command().get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };
    let root_explicit = matches.value_source("root") == Some(ValueSource::CommandLine);
    match cli.command {
        Command::Generate { output } => {
            generate(&cli.root, output.as_deref())?;
            Ok(ExitCode::SUCCESS)
        },
        Command::Validate {
            roadmap_md,
            accept_drift,
        } => validate_cmd(&cli.root, &roadmap_md, accept_drift, root_explicit),
        Command::Add {
            slug,
            allow_legacy_numeric,
        } => add_cmd(&cli.root, &slug, allow_legacy_numeric),
        Command::Import {
            source,
            dry_run,
            map,
        } => import_cmd(&cli.root, &source, dry_run, &map),
        Command::Rename {
            from,
            to,
            allow_legacy_numeric,
        } => rename_cmd(&cli.root, &from, &to, allow_legacy_numeric),
    }
}

/// Render the roadmap, then emit it — to `output` when given, else stdout.
///
/// Everything that can fail (config, feature parsing) happens *before* the
/// first byte is written, and the file path goes through an atomic replace,
/// so a failing run never destroys the roadmap it was asked to regenerate.
fn generate(root: &std::path::Path, output: Option<&std::path::Path>) -> Result<()> {
    // Name the resolved root, not just the file: with a mistyped `--root`
    // the failure is "that tree isn't there", which is the same story
    // `validate` now tells about the same mistake (#31).
    let config = roadmark::load_config(root)
        .with_context(|| format!("reading roadmap source at {}", root.display()))?;
    let mut features = roadmark::load_features(root, &config)
        .with_context(|| format!("reading roadmap source at {}", root.display()))?;
    let sections = roadmark::load_sections(root, &config)
        .with_context(|| format!("reading roadmap source at {}", root.display()))?;
    roadmark::sort_features(&mut features, &config);
    let rendered = roadmark::render(&features, &config, &sections);
    match output {
        Some(path) => roadmark::write_atomic(path, &rendered)
            .with_context(|| format!("writing {}", path.display()))?,
        None => print!("{rendered}"),
    }
    Ok(())
}

/// Deprecation warning shared by `add` and `rename` when a legacy
/// `f<digits>` slug is accepted under `--allow-legacy-numeric`. `noun`
/// is the caller's subject ("features" / "slugs").
fn warn_legacy_numeric(slug: &str, noun: &str) {
    eprintln!(
        "warning: `{slug}` uses the legacy `f<digits>` slug shape — \
         deprecated, only intended for one-shot migration. New \
         {noun} should use `f-<kebab-name>`."
    );
}

fn add_cmd(root: &std::path::Path, slug: &str, allow_legacy_numeric: bool) -> Result<ExitCode> {
    let outcome = roadmark::add::add(root, slug, allow_legacy_numeric)?;
    if outcome.legacy_numeric_warning {
        warn_legacy_numeric(slug, "features");
    }
    println!("created {}", outcome.path.display());
    Ok(ExitCode::SUCCESS)
}

/// `import` reports rather than narrates: counts, then the paths, then
/// what a human still owes the tree. The closing hint is the point of the
/// command — the import lands you on a *failing* `validate`, and saying so
/// is what stops that failure reading as a bug.
fn import_cmd(
    root: &std::path::Path,
    source: &std::path::Path,
    dry_run: bool,
    map: &[String],
) -> Result<ExitCode> {
    let mut options = roadmark::import::ImportOptions::default();
    for spec in map {
        options.add_mapping(spec)?;
    }
    let outcome = roadmark::import::import(root, source, &options, dry_run)?;
    let verb = if outcome.dry_run {
        "would create"
    } else {
        "created"
    };
    println!("{verb} {} feature file(s)", outcome.created.len());
    for path in &outcome.created {
        println!("  {}", path.display());
    }
    if !outcome.skipped.is_empty() {
        println!("skipped {} existing file(s):", outcome.skipped.len());
        for path in &outcome.skipped {
            println!("  {}", path.display());
        }
    }
    for (label, path) in [
        ("config", &outcome.config_written),
        ("leftover prose", &outcome.leftovers_written),
    ] {
        if let Some(path) = path {
            println!("{verb} {label}: {}", path.display());
        }
    }
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    if !outcome.dry_run {
        // Say what actually happens next. The imported tree *generates* —
        // that is deliberate, so the adopter can see their roadmap
        // immediately — and `validate` names what is still owed rather
        // than refusing the tree over it.
        eprintln!(
            "hint: the tree generates as-is. `roadmark validate` will name every \
             `<TODO>` left to decide; uncomment a `[fields.*]` block in config.toml \
             (with its `required_when`) to turn those into a gate."
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn rename_cmd(
    root: &std::path::Path,
    from: &str,
    to: &str,
    allow_legacy_numeric: bool,
) -> Result<ExitCode> {
    let outcome = roadmark::rename::rename(root, from, to, allow_legacy_numeric)?;
    if outcome.legacy_numeric_warning {
        warn_legacy_numeric(to, "slugs");
    }
    println!(
        "renamed {} -> {}",
        outcome.old_path.display(),
        outcome.new_path.display()
    );
    println!("rewrote {} file(s)", outcome.rewritten.len());
    eprintln!("hint: regenerate the roadmap (`roadmark generate -o ROADMAP.md`)");
    Ok(ExitCode::SUCCESS)
}

fn validate_cmd(
    root: &std::path::Path,
    roadmap_md: &std::path::Path,
    accept_drift: bool,
    root_explicit: bool,
) -> Result<ExitCode> {
    let report = roadmark::validate::validate(root, roadmap_md, root_explicit)?;
    print!("{}", report.to_text());
    if report.has_hard_errors() {
        return Ok(ExitCode::FAILURE);
    }
    if report.has_drift() && !accept_drift {
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}
