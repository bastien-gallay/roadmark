//! `import` subcommand: bootstrap a `.roadmap/` tree from an existing
//! hand-written `ROADMAP.md`.
//!
//! Every candidate adopter already has a roadmap — that is the premise of
//! the pitch. Asking them to retype seventy rows is the adoption cost, and
//! hand-transcription of a *nearly* mechanical task is exactly where
//! silent mistakes come from.
//!
//! The split is deliberate and honest about its limits. What a table can
//! tell us — id, status, horizon, the summary prose, the areas, and the
//! bucket when the document is organised by headings — is written.
//!
//! What it cannot divides again, along a line the schema draws rather than
//! this module. `class` and `effort` are omissible, so they are written
//! **commented out** with their value set inline: a decision, not a
//! lookup. `type`, `area` and `target` are *mandatory* frontmatter, so a
//! comment would produce a file that doesn't parse — and `generate`
//! failing outright is a far worse landing than a roadmap with a visible
//! gap in it. Those get a `<TODO>` placeholder, which parses, generates,
//! and is named by `validate` as a warning.
//!
//! So the imported tree runs on arrival and tells you what it doesn't
//! know, which is what turns "seventy files to write" into "seventy fields
//! to decide", reviewable in one pass.
//!
//! Layered like the rest of the crate: [`plan_import`] is string-in,
//! struct-out and never touches the filesystem, so the whole mapping is
//! unit- and snapshot-testable; [`import`] is the thin I/O half.

use crate::add::{classify_slug, derive_id};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every field a `--map` may name: the ones read from a table column, plus
/// `summary`, which becomes the body rather than a frontmatter key.
///
/// `class` and `effort` are here even though they are written commented
/// out — a source table that *does* carry them should be believed.
const MAPPABLE: &[&str] = &[
    "id", "type", "status", "horizon", "target", "area", "class", "effort", "summary",
];

/// The axes a table almost never carries, emitted **commented out** with a
/// suggested value set so uncommenting is a decision, not a lookup.
///
/// Only the genuinely omissible ones are here. `type` and `area` are
/// structurally mandatory — commenting those out would produce a file that
/// doesn't *parse*, so `generate` would fail before the adopter ever saw
/// the roadmap. They are written with a guess instead, and the guess is
/// reported as a warning.
///
/// Paired with the `[fields.*]` suggestions written into `config.toml`, so
/// uncommenting one side has something to validate against.
const UNDECIDABLE: &[(&str, &str)] = &[
    (
        "class",
        r#"class = "enabler"       # differentiator | enabler | table-stakes | polish | bet"#,
    ),
    ("effort", r#"effort = "M"            # S | M | L"#),
];

/// Placeholder for a mandatory field the source could not supply. Not a
/// plausible value on purpose: it parses, so `generate` runs, and it fails
/// membership the moment the axis is declared.
const TODO_VALUE: &str = "<TODO>";

/// How a source table's headers map onto roadmark's fields.
///
/// No two hand-written roadmaps use the same headers, so the defaults are
/// a best guess ([`guess_field`]) and `--map <field>=<header>` overrides
/// any of them.
#[derive(Debug, Default, Clone)]
pub struct ImportOptions {
    /// field name → source header, lowercased. Overrides the guess.
    pub mapping: BTreeMap<String, String>,
}

impl ImportOptions {
    /// Parse a `field=Header` override as typed on the command line.
    pub fn add_mapping(&mut self, spec: &str) -> Result<()> {
        let (field, header) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("`--map` takes `field=Header`, got `{spec}`"))?;
        let field = field.trim().to_lowercase();
        if !MAPPABLE.contains(&field.as_str()) {
            bail!(
                "`--map` field `{field}` is not importable (known: {})",
                MAPPABLE.join(", ")
            );
        }
        self.mapping.insert(field, header.trim().to_lowercase());
        Ok(())
    }
}

/// One feature file the import would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFeature {
    pub slug: String,
    pub id: String,
    /// Full file contents, frontmatter included.
    pub contents: String,
}

/// What an import would produce, computed without touching the disk.
#[derive(Debug, Default, Clone)]
pub struct ImportPlan {
    pub features: Vec<PlannedFeature>,
    /// Section headings that became `target` buckets, in document order —
    /// the `versions` list a bootstrapped `config.toml` needs.
    pub buckets: Vec<String>,
    /// Horizon values seen, in first-appearance order. Declared in the
    /// bootstrapped config: `validate` requires `[fields.horizon]` once a
    /// feature carries one, and failing on a fact the import *derived*
    /// would be noise in the middle of the signal.
    pub horizons: Vec<String>,
    /// Prose that belonged to no table row. Kept rather than dropped: it
    /// is usually the reasoning a roadmap is read for.
    pub leftovers: String,
    /// Things a human should look at — never fatal, because a partial
    /// import a human then fixes beats no import at all.
    pub warnings: Vec<String>,
}

/// Outcome of a real (or dry) run. Paths only — `main.rs` owns the words.
#[derive(Debug)]
pub struct ImportOutcome {
    pub created: Vec<PathBuf>,
    /// Feature files that already existed. Never overwritten: an import
    /// re-run must not clobber edits made since the first one.
    pub skipped: Vec<PathBuf>,
    pub config_written: Option<PathBuf>,
    pub leftovers_written: Option<PathBuf>,
    pub warnings: Vec<String>,
    pub dry_run: bool,
}

/// Read `source`, plan the import, and write the tree.
///
/// Nothing is overwritten. A `config.toml` is written only when absent —
/// without one `generate` cannot run at all, so an import that skipped it
/// would land the adopter on a crash rather than on the failing `validate`
/// that is the whole point.
pub fn import(
    root: &Path,
    source: &Path,
    options: &ImportOptions,
    dry_run: bool,
) -> Result<ImportOutcome> {
    let markdown =
        std::fs::read_to_string(source).with_context(|| format!("reading {}", source.display()))?;
    let plan = plan_import(&markdown, options)?;

    let features_dir = root.join("features");
    let mut created = Vec::new();
    let mut skipped = Vec::new();
    for feature in &plan.features {
        let path = features_dir.join(format!("{}.md", feature.slug));
        if path.exists() {
            skipped.push(path);
        } else {
            created.push(path);
        }
    }
    let config_path = root.join("config.toml");
    let config_written = (!config_path.exists()).then(|| config_path.clone());
    let leftovers_path = root.join("import-leftovers.md");
    let leftovers_written = (!plan.leftovers.trim().is_empty() && !leftovers_path.exists())
        .then(|| leftovers_path.clone());

    if !dry_run {
        std::fs::create_dir_all(&features_dir)
            .with_context(|| format!("creating {}", features_dir.display()))?;
        for (feature, path) in plan.features.iter().zip(
            plan.features
                .iter()
                .map(|f| features_dir.join(format!("{}.md", f.slug))),
        ) {
            if path.exists() {
                continue;
            }
            std::fs::write(&path, &feature.contents)
                .with_context(|| format!("writing {}", path.display()))?;
        }
        if let Some(path) = &config_written {
            std::fs::write(path, render_config(&plan.buckets, &plan.horizons))
                .with_context(|| format!("writing {}", path.display()))?;
        }
        if let Some(path) = &leftovers_written {
            std::fs::write(path, &plan.leftovers)
                .with_context(|| format!("writing {}", path.display()))?;
        }
    }

    Ok(ImportOutcome {
        created,
        skipped,
        config_written,
        leftovers_written,
        warnings: plan.warnings,
        dry_run,
    })
}

/// Turn a hand-written roadmap into a plan. Pure: no filesystem, so the
/// mapping is testable on a string.
pub fn plan_import(markdown: &str, options: &ImportOptions) -> Result<ImportPlan> {
    let mut plan = ImportPlan::default();
    let mut leftovers = String::new();
    let mut heading: Option<String> = None;
    let mut seen_slugs: BTreeMap<String, usize> = BTreeMap::new();

    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(title) = heading_text(line) {
            heading = Some(title);
            leftovers.push_str(line);
            leftovers.push('\n');
            i += 1;
            continue;
        }
        match take_table(&lines, i) {
            Some((table, next)) => {
                let consumed = import_table(
                    &table,
                    heading.as_deref(),
                    options,
                    &mut seen_slugs,
                    &mut plan,
                );
                if !consumed {
                    // A legend, a projection matrix, a key — not a feature
                    // table. It goes to leftovers verbatim rather than
                    // vanishing: silently dropping content is the one
                    // unrecoverable thing an import could do.
                    for line in &lines[i..next] {
                        leftovers.push_str(line);
                        leftovers.push('\n');
                    }
                }
                i = next;
            },
            None => {
                leftovers.push_str(line);
                leftovers.push('\n');
                i += 1;
            },
        }
    }

    if plan.features.is_empty() {
        bail!(
            "no importable table found — expected a markdown table with a header row \
             and at least one of an `ID` or `Summary` column (use `--map field=Header` \
             if yours are named differently)"
        );
    }
    plan.leftovers = tidy_leftovers(&leftovers);
    Ok(plan)
}

/// `## Foo` → `Foo`. Only `##`/`###`, because `#` is the document title
/// and never a bucket.
fn heading_text(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("###")
        .or_else(|| trimmed.strip_prefix("##"))?;
    let title = rest.trim_start_matches('#').trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// A markdown table starting at `start`: header row, delimiter row, then
/// body rows. Returns the rows and the index just past the table.
///
/// Hand-rolled, like every other scan in this crate — the shape is fixed
/// and narrow, and a markdown AST would round-trip the prose badly.
fn take_table(lines: &[&str], start: usize) -> Option<(Vec<Vec<String>>, usize)> {
    let header = split_row(lines.get(start)?)?;
    let delim = split_row(lines.get(start + 1)?)?;
    if delim.is_empty() || !delim.iter().all(|c| is_delimiter_cell(c)) {
        return None;
    }
    let mut rows = vec![header];
    let mut i = start + 2;
    while let Some(row) = lines.get(i).and_then(|l| split_row(l)) {
        rows.push(row);
        i += 1;
    }
    (rows.len() > 1).then_some((rows, i))
}

/// `| a | b |` → `["a", "b"]`, or `None` when the line isn't a table row.
/// An escaped `\|` is a literal pipe in a cell, not a separator — the
/// renderer writes it that way, so the importer must read it that way.
fn split_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let inner = trimmed
        .strip_prefix('|')?
        .strip_suffix('|')
        .unwrap_or(&trimmed[1..]);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            if ch != '|' {
                cell.push('\\');
            }
            cell.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            cells.push(cell.trim().to_string());
            cell = String::new();
        } else {
            cell.push(ch);
        }
    }
    if escaped {
        cell.push('\\');
    }
    cells.push(cell.trim().to_string());
    Some(cells)
}

fn is_delimiter_cell(cell: &str) -> bool {
    let body = cell.trim().trim_start_matches(':').trim_end_matches(':');
    !body.is_empty() && body.chars().all(|c| c == '-')
}

/// Which source header feeds `field`, if any: an explicit `--map` first,
/// then an exact header match, then a contains-match over known aliases.
fn guess_field(field: &str, headers: &[String], options: &ImportOptions) -> Option<usize> {
    if let Some(wanted) = options.mapping.get(field) {
        return headers.iter().position(|h| h == wanted);
    }
    let aliases: &[&str] = match field {
        "id" => &["id", "feature", "key"],
        "status" => &["status", "state"],
        "horizon" => &["horizon", "priority", "when"],
        "target" => &["target", "release", "milestone", "version"],
        "summary" => &["summary", "description", "notes", "detail"],
        "area" => &["area", "topic", "direction", "component"],
        "effort" => &["effort", "size", "estimate"],
        "class" => &["class", "kind", "leverage"],
        "type" => &["type"],
        _ => &[],
    };
    headers
        .iter()
        .position(|h| aliases.contains(&h.as_str()))
        .or_else(|| {
            headers
                .iter()
                .position(|h| aliases.iter().any(|a| h.contains(a)))
        })
}

fn import_table(
    rows: &[Vec<String>],
    heading: Option<&str>,
    options: &ImportOptions,
    seen_slugs: &mut BTreeMap<String, usize>,
    plan: &mut ImportPlan,
) -> bool {
    let headers: Vec<String> = rows[0].iter().map(|h| h.trim().to_lowercase()).collect();
    let col = |field: &str| guess_field(field, &headers, options);
    let (id_col, summary_col) = (col("id"), col("summary"));
    if id_col.is_none() && summary_col.is_none() {
        // Not a feature table — a legend, a projection matrix, a key.
        // Manufacturing features from it would be worse than handing it
        // back to a human; the caller keeps it as prose.
        return false;
    }
    let (status_col, horizon_col, target_col) = (col("status"), col("horizon"), col("target"));
    let (area_col, type_col) = (col("area"), col("type"));

    let bucket = heading.map(str::to_string);
    if let Some(b) = &bucket {
        if target_col.is_none() && !plan.buckets.contains(b) {
            plan.buckets.push(b.clone());
        }
    }

    for row in &rows[1..] {
        let cell = |idx: Option<usize>| idx.and_then(|i| row.get(i)).map(String::as_str);
        let summary = cell(summary_col).unwrap_or("").trim().to_string();
        let raw_id = cell(id_col).unwrap_or("").trim().to_string();
        if raw_id.is_empty() && summary.is_empty() {
            continue;
        }
        let slug = unique_slug(&derive_slug(&raw_id, &summary), seen_slugs);
        let id = derive_id(&slug);
        let status = cell(status_col)
            .map(parse_status)
            .unwrap_or("todo")
            .to_string();
        let horizon = cell(horizon_col).and_then(parse_horizon);
        // `target` from a column when there is one, else from the enclosing
        // heading — a roadmap organised by `## Must` / `## Q3` states its
        // buckets in its structure, and re-typing them would be the same
        // mechanical transcription this command exists to remove.
        let target = cell(target_col)
            .map(clean_cell)
            .filter(|t| !t.is_empty())
            .or_else(|| bucket.clone());

        // `type` and `area` are mandatory frontmatter, so they can't be
        // commented out like the optional axes — a file missing them
        // doesn't parse, and `generate` would fail before the adopter saw
        // anything. Take them from a column when one maps, else guess and
        // say so.
        let area = cell(area_col)
            .map(split_multi)
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| vec![TODO_VALUE.to_string()]);
        let item_type = cell(type_col)
            .map(|t| clean_cell(t).to_lowercase())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "feature".to_string());

        if raw_id.is_empty() {
            plan.warnings.push(format!(
                "{slug}: no id column — slug derived from the summary, rename it if you had one"
            ));
        }
        if area_col.is_none() {
            plan.warnings.push(format!(
                "{slug}: no area column — wrote `{TODO_VALUE}` (map one with `--map area=<Header>`)"
            ));
        }
        if type_col.is_none() {
            plan.warnings
                .push(format!("{slug}: no type column — assumed `feature`"));
        }
        if let Some(h) = &horizon {
            if !plan.horizons.contains(h) {
                plan.horizons.push(h.clone());
            }
        }
        plan.features.push(PlannedFeature {
            contents: render_feature(
                &id,
                &item_type,
                &area,
                &status,
                horizon.as_deref(),
                target.as_deref(),
                &summary,
            ),
            slug,
            id,
        });
    }
    true
}

/// The catalog cell as prose: link syntax stripped to its text, so
/// `[F-foo](#f-foo)` reads as `F-foo`.
fn clean_cell(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let chars: Vec<char> = cell.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|c| *c == ']') {
                let text: String = chars[i + 1..i + 1 + close].iter().collect();
                let after = i + 1 + close + 1;
                if chars.get(after) == Some(&'(') {
                    if let Some(end) = chars[after..].iter().position(|c| *c == ')') {
                        out.push_str(&text);
                        i = after + end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out.trim().to_string()
}

/// Status from a glyph or a word. Unknown reads as `todo` — the safest
/// wrong answer, since it under-claims progress rather than over-claiming.
fn parse_status(cell: &str) -> &'static str {
    let text = cell.to_lowercase();
    if cell.contains('✅') || text.contains("done") || text.contains("shipped") {
        "done"
    } else if cell.contains('🚧') || text.contains("wip") || text.contains("progress") {
        "wip"
    } else if cell.contains('⛔') || text.contains("blocked") {
        "blocked"
    } else {
        "todo"
    }
}

/// A horizon cell keeps only its word — hand-written roadmaps decorate
/// them (`✅ Shipped`, `**Now**`), and the frontmatter wants the value.
fn parse_horizon(cell: &str) -> Option<String> {
    let cleaned: String = clean_cell(cell)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-')
        .collect();
    let word = cleaned.split_whitespace().next()?.to_lowercase();
    (!word.is_empty()).then_some(word)
}

/// Slug from the id cell when there is one, else from the summary's first
/// words. Always coerced to the canonical `f-<kebab>` shape, so an
/// imported tree passes the same slug rules a hand-authored one does.
fn derive_slug(raw_id: &str, summary: &str) -> String {
    let source = if raw_id.is_empty() { summary } else { raw_id };
    let cleaned = clean_cell(source).to_lowercase();
    // Without an id column the summary supplies the slug, so bound it:
    // a whole sentence makes an unusable filename. Counted in *words*, and
    // the cut happens before the word starts — truncating mid-word leaves
    // a stray letter hanging off the end.
    let word_budget = if raw_id.is_empty() { 5 } else { usize::MAX };
    let mut body = String::new();
    for ch in cleaned.chars() {
        let alnum = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        if alnum {
            let starting_word = body.is_empty() || body.ends_with('-');
            let words_so_far = body.split('-').filter(|s| !s.is_empty()).count();
            if starting_word && words_so_far >= word_budget {
                break;
            }
            body.push(ch);
        } else if !body.ends_with('-') {
            body.push('-');
        }
    }
    let body = body.trim_matches('-').to_string();
    let body = body.strip_prefix("f-").unwrap_or(&body).to_string();
    if body.is_empty() {
        return "f-imported".to_string();
    }
    let candidate = format!("f-{body}");
    if classify_slug(&candidate).is_ok() {
        candidate
    } else {
        "f-imported".to_string()
    }
}

/// Disambiguate a repeated slug rather than silently dropping a row: two
/// features that lowercase alike are a real collision `validate` would
/// report, and the import should not create it.
fn unique_slug(base: &str, seen: &mut BTreeMap<String, usize>) -> String {
    let count = seen.entry(base.to_string()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base.to_string()
    } else {
        format!("{base}-{count}")
    }
}

/// A multi-valued cell: hand-written roadmaps separate with `·`, `,`, `/`
/// or `;`. Kept lowercase, because a taxonomy value is a key, not prose.
fn split_multi(cell: &str) -> Vec<String> {
    clean_cell(cell)
        .split(['·', ',', '/', ';'])
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn render_feature(
    id: &str,
    item_type: &str,
    area: &[String],
    status: &str,
    horizon: Option<&str>,
    target: Option<&str>,
    summary: &str,
) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("+++\n");
    out.push_str(&format!("id = \"{id}\"\n"));
    out.push_str(&format!("type = \"{item_type}\"\n"));
    for (_, line) in UNDECIDABLE {
        out.push_str(&format!("# {line}\n"));
    }
    let area_list = area
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("area = [{area_list}]\n"));
    if let Some(h) = horizon {
        out.push_str(&format!("horizon = \"{h}\"\n"));
    }
    out.push_str(&format!("status = \"{status}\"\n"));
    // Mandatory like `type` and `area`, so a placeholder rather than a
    // comment — an absent `target` doesn't parse.
    out.push_str(&format!(
        "target = [\"{}\"]\n",
        target.unwrap_or(TODO_VALUE)
    ));
    out.push_str("+++\n\n");
    let body = summary.trim();
    if body.is_empty() {
        out.push_str("<TODO: one-paragraph summary — imported row had none.>\n");
    } else {
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// A `config.toml` that lets `generate` run, with the taxonomy suggestions
/// commented to match the commented frontmatter — uncommenting one side
/// needs the other, and having both in front of you makes that obvious.
fn render_config(buckets: &[String], horizons: &[String]) -> String {
    let list = |values: &[String]| {
        format!(
            "[{}]",
            values
                .iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let versions = if buckets.is_empty() {
        "[\"v0.1\", \"Later\"]".to_string()
    } else {
        list(buckets)
    };
    // Declared, not suggested: the import derived these values, and the
    // declared order is also the sort rank — leaving it out would fail
    // `validate` over something already known, and rank rows by id.
    let horizon_section = if horizons.is_empty() {
        String::new()
    } else {
        format!(
            "\n# Order is the sort rank. Reorder to taste — imported in the order\n\
             # the source document first used each value.\n\
             [fields.horizon]\nvalues = {}\n",
            list(horizons)
        )
    };
    format!(
        r#"# Written by `roadmark import`. Buckets come from the source document's
# headings where it had them.
versions = {versions}
title = "Roadmap"
{horizon_section}

# Uncomment a section here together with the matching line in the feature
# files. `validate` requires the declaration as soon as one feature carries
# the axis, so the two move together.
#
# [fields.type]
# values = ["feature", "fix", "chore"]
#
# [fields.class]
# values = ["differentiator", "enabler", "table-stakes", "polish", "bet"]
# required_when = {{ type = "feature" }}
#
# [fields.effort]
# values = ["S", "M", "L"]
#
# [fields.area]
# values = ["core", "cli", "docs"]
# multi = true
"#
    )
}

/// Collapse the runs of blank lines left behind by lifted tables, and
/// drop the result entirely if nothing but whitespace remains.
fn tidy_leftovers(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blanks = 0;
    for line in raw.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    format!(
        "<!-- Prose `roadmark import` could not attribute to a feature. \
         Move what belongs in a feature body into its file; the rest is a \
         candidate for a `sections` entry in config.toml. -->\n\n{trimmed}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(markdown: &str) -> ImportPlan {
        plan_import(markdown, &ImportOptions::default()).unwrap()
    }

    #[test]
    fn split_row_treats_an_escaped_pipe_as_content() {
        // The renderer writes `\|` for a literal pipe, so the importer must
        // read it back as one rather than as a column boundary.
        assert_eq!(
            split_row(r"| a \| b | c |").unwrap(),
            vec!["a | b".to_string(), "c".to_string()]
        );
        assert_eq!(split_row("no pipes here"), None);
    }

    #[test]
    fn a_delimiter_row_may_be_aligned() {
        assert!(is_delimiter_cell("---"));
        assert!(is_delimiter_cell(":---"));
        assert!(is_delimiter_cell(":---:"));
        assert!(!is_delimiter_cell("abc"));
        assert!(!is_delimiter_cell(""));
    }

    #[test]
    fn status_reads_a_glyph_or_a_word_and_defaults_to_todo() {
        assert_eq!(parse_status("✅"), "done");
        assert_eq!(parse_status("✅ Shipped"), "done");
        assert_eq!(parse_status("🚧"), "wip");
        assert_eq!(parse_status("In progress"), "wip");
        assert_eq!(parse_status("⛔ blocked upstream"), "blocked");
        assert_eq!(parse_status("☐"), "todo");
        // Unknown under-claims rather than over-claims progress.
        assert_eq!(parse_status("¯\\_(ツ)_/¯"), "todo");
    }

    #[test]
    fn horizon_keeps_only_its_word() {
        assert_eq!(parse_horizon("✅ Shipped").as_deref(), Some("shipped"));
        assert_eq!(parse_horizon("**Now**").as_deref(), Some("now"));
        assert_eq!(parse_horizon("").as_deref(), None);
    }

    #[test]
    fn a_cell_keeps_link_text_and_drops_the_target() {
        assert_eq!(clean_cell("[F-foo](#f-foo)"), "F-foo");
        assert_eq!(clean_cell("plain"), "plain");
        // An unterminated link is left alone rather than eaten.
        assert_eq!(clean_cell("[F-foo](#f-foo"), "[F-foo](#f-foo");
    }

    #[test]
    fn slugs_are_coerced_to_the_canonical_shape() {
        assert_eq!(derive_slug("F-region-frontiers", ""), "f-region-frontiers");
        assert_eq!(derive_slug("[F-foo](#f-foo)", ""), "f-foo");
        // Punctuation and case collapse; the result must satisfy the same
        // rule `add` enforces, so an imported tree isn't second-class.
        assert_eq!(derive_slug("Region Frontiers!", ""), "f-region-frontiers");
        assert!(classify_slug(&derive_slug("Region Frontiers!", "")).is_ok());
        // No id column: the summary supplies a bounded slug.
        assert_eq!(
            derive_slug("", "Regions are now a contiguous partition of space"),
            "f-regions-are-now-a-contiguous"
        );
    }

    #[test]
    fn repeated_slugs_are_disambiguated_rather_than_dropped() {
        let plan = plan("| ID | Summary |\n| --- | --- |\n| F-dup | One. |\n| F-dup | Two. |\n");
        let slugs: Vec<&str> = plan.features.iter().map(|f| f.slug.as_str()).collect();
        assert_eq!(slugs, vec!["f-dup", "f-dup-2"]);
    }

    #[test]
    fn a_table_that_is_not_a_feature_table_is_left_as_prose() {
        // A legend or projection matrix has neither an id nor a summary
        // column; manufacturing features from it would be worse than
        // leaving it for a human.
        let markdown = "\
| Projection | Direction |\n| --- | --- |\n| ROADMAP.md | files → doc |\n\n\
| ID | Summary |\n| --- | --- |\n| F-a | A thing. |\n";
        let plan = plan(markdown);
        assert_eq!(plan.features.len(), 1);
        assert!(plan.leftovers.contains("ROADMAP.md"), "{}", plan.leftovers);
    }

    #[test]
    fn headings_become_buckets_only_when_no_target_column_does() {
        let with_heading = plan("## Must\n\n| ID | Summary |\n| --- | --- |\n| F-a | A. |\n");
        assert_eq!(with_heading.buckets, vec!["Must"]);
        assert!(with_heading.features[0]
            .contents
            .contains("target = [\"Must\"]"));

        // An explicit column wins: it is the project's own answer.
        let with_column = plan(
            "## Must\n\n| ID | Target | Summary |\n| --- | --- | --- |\n| F-a | v0.9 | A. |\n",
        );
        assert!(with_column.buckets.is_empty());
        assert!(with_column.features[0]
            .contents
            .contains("target = [\"v0.9\"]"));
    }

    #[test]
    fn a_mapping_overrides_the_header_guess() {
        let mut options = ImportOptions::default();
        options.add_mapping("area=Direction").unwrap();
        let plan = plan_import(
            "| ID | Direction | Summary |\n| --- | --- | --- |\n| F-a | model · grouping | A. |\n",
            &options,
        )
        .unwrap();
        assert!(plan.features[0]
            .contents
            .contains("area = [\"model\", \"grouping\"]"));
    }

    #[test]
    fn an_unknown_mapping_field_is_refused_with_the_known_list() {
        let mut options = ImportOptions::default();
        let err = options.add_mapping("bogus=X").unwrap_err();
        assert!(format!("{err:#}").contains("not importable"));
        // …and a malformed spec says what the shape is.
        let err = options.add_mapping("no-equals-sign").unwrap_err();
        assert!(format!("{err:#}").contains("field=Header"));
    }

    #[test]
    fn the_bootstrapped_config_declares_the_horizons_it_derived() {
        // `validate` requires `[fields.horizon]` once a feature carries
        // one, and the declared order is the sort rank — failing over a
        // fact the import itself derived would be noise.
        let plan = plan(
            "| ID | Horizon | Summary |\n| --- | --- | --- |\n\
             | F-a | Now | A. |\n| F-b | Later | B. |\n",
        );
        assert_eq!(plan.horizons, vec!["now", "later"]);
        let config = render_config(&plan.buckets, &plan.horizons);
        assert!(config.contains("[fields.horizon]"), "{config}");
        assert!(config.contains(r#"values = ["now", "later"]"#), "{config}");
        // The undecidable axes stay commented, on both sides.
        assert!(config.contains("# [fields.class]"), "{config}");
    }
}
