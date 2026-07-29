//! `import` subcommand: bootstrap a `.roadmap/` tree from an existing
//! hand-written `ROADMAP.md`.
//!
//! Every candidate adopter already has a roadmap — that is the premise of
//! the pitch. Asking them to retype seventy rows is the adoption cost, and
//! hand-transcription of a *nearly* mechanical task is exactly where
//! silent mistakes come from.
//!
//! Two source shapes are read. A **table** gives id, status, horizon, the
//! summary prose, the areas, and the bucket when the document is organised
//! by headings. **Checkbox bullets** — `- [x] `F-thing` — prose…` under
//! bucket headings — give the same things by position instead of by
//! header, and are read only when the document holds no feature table and
//! only where the document does not distinguish them from checklists (see
//! [`plan_import`]). The bullet form is the poorer source on paper
//! and the richer one in practice: it has no columns, but its body is
//! paragraphs rather than one cell, which is what `## Details` wants.
//!
//! The split is deliberate and honest about its limits.
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
///
/// Written to fit 80 columns once commented (`# ` prefix included): a
/// feature file is markdown, and an adopter who lints their own
/// `.roadmap/` should not have to exclude what the import wrote (#71).
const UNDECIDABLE: &[(&str, &str)] = &[
    (
        "class",
        r#"class = "enabler"  # differentiator | enabler | table-stakes | polish | bet"#,
    ),
    ("effort", r#"effort = "M"       # S | M | L"#),
];

/// Placeholder for a mandatory field the source could not supply. Not a
/// plausible value on purpose: it parses, so `generate` runs, and it fails
/// membership the moment the axis is declared.
const TODO_VALUE: &str = "<TODO>";

/// Width an imported body is wrapped to. `## Details` reproduces a body
/// verbatim, so this is the same 80-column budget the rest of the
/// generated document keeps.
const BODY_WRAP_WIDTH: usize = 80;

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
///
/// Tables first, checkbox bullets only if the document had no feature table
/// (#57). The fallback is ordered, not merged: a document that states its
/// features in a table may still hold bullet checklists — release chores, a
/// definition of done — and reading those as features would manufacture
/// rows nobody wrote. Only a document with no table at all is unambiguous.
///
/// Within that fallback the same question returns, because a bullet roadmap
/// carries checklists too. The document answers it when it is consistent:
/// if *some* bullet names its feature in backticks, that is the shape of a
/// feature here, and the ones without an id are a checklist — kept as
/// leftovers. Only when no bullet anywhere carries an id is every bullet
/// read as a feature, with the slug derived from the text and a warning.
pub fn plan_import(markdown: &str, options: &ImportOptions) -> Result<ImportPlan> {
    let plan = scan(markdown, options, BulletMode::Ignore);
    if !plan.features.is_empty() {
        return Ok(plan);
    }
    let mode = if any_bullet_carries_an_id(markdown) {
        BulletMode::ReadIdentified
    } else {
        BulletMode::ReadAll
    };
    let plan = scan(markdown, options, mode);
    if plan.features.is_empty() {
        bail!(
            "no importable features found — expected a markdown table with a header row \
             and at least one of an `ID` or `Summary` column (use `--map field=Header` \
             if yours are named differently), or `- [ ]` / `- [x]` checkbox bullets"
        );
    }
    Ok(plan)
}

/// Whether the scan reads `- [x]` bullets as features, and which ones.
/// See [`plan_import`] for why this is a second pass and why the document
/// gets to answer the second question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BulletMode {
    /// Bullets are prose; a feature table already carried the document.
    Ignore,
    /// Only bullets naming an id in backticks are features.
    ReadIdentified,
    /// No bullet anywhere has an id, so every bullet is a feature.
    ReadAll,
}

/// Whether any top-level checkbox bullet opens with a backticked token.
fn any_bullet_carries_an_id(markdown: &str) -> bool {
    let lines: Vec<&str> = markdown.lines().collect();
    (0..lines.len())
        .any(|i| take_bullet(&lines, i).is_some_and(|(entry, _)| !entry.raw_id.is_empty()))
}

fn scan(markdown: &str, options: &ImportOptions, bullets: BulletMode) -> ImportPlan {
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
                if bullets != BulletMode::Ignore {
                    if let Some((entry, next)) = take_bullet(&lines, i) {
                        if entry.raw_id.is_empty() && bullets == BulletMode::ReadIdentified {
                            // A checklist in a document whose features are
                            // identified. Prose, not a feature nobody named.
                            for line in &lines[i..next] {
                                leftovers.push_str(line);
                                leftovers.push('\n');
                            }
                        } else {
                            import_bullet(&entry, heading.as_deref(), &mut seen_slugs, &mut plan);
                        }
                        i = next;
                        continue;
                    }
                }
                leftovers.push_str(line);
                leftovers.push('\n');
                i += 1;
            },
        }
    }

    plan.leftovers = tidy_leftovers(&leftovers);
    plan
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
                // A table cell is one line by construction, so it is the
                // *likelier* source of an over-wide body than a bullet —
                // and the table is the more common import shape. Wrapping
                // only the bullet path left #71 half fixed.
                &rewrap(&summary),
            ),
            slug,
            id,
        });
    }
    true
}

/// A checkbox bullet read as a row. Position replaces the header
/// inference a table needs: the checkbox is the status, the backticked
/// token is the id, the enclosing heading is the target, the rest is body.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BulletEntry {
    status: &'static str,
    /// The backticked token, if the bullet carried one.
    raw_id: String,
    /// Lead paragraph: the bullet's own prose, wrapped lines rejoined.
    prose: String,
    /// Everything indented under the bullet past its lead paragraph —
    /// nested bullets, further paragraphs — verbatim and dedented, kept as
    /// prose in the parent body rather than promoted to features of their
    /// own. roadmark has no sub-features, so promotion invents ids the
    /// source never wrote; that is a judgement call and belongs behind a
    /// flag, not in the default.
    tail: Vec<String>,
}

/// A top-level `- [x] ` / `- [ ] ` bullet and everything indented under it.
///
/// The entry ends at the first unindented line, so a wrapped bullet keeps
/// its continuation lines and the next bullet starts its own entry. Only
/// unindented bullets start one — an indented checkbox is nested content
/// of the bullet above it.
///
/// A blank line ends the entry only when what follows is unindented. A
/// *loose* list — blank lines between an item and its sub-items — is
/// ordinary markdown, and ending there would strand the sub-items in
/// leftovers while the CHANGELOG promised them to the parent's body.
fn take_bullet(lines: &[&str], start: usize) -> Option<(BulletEntry, usize)> {
    let line = lines.get(start)?;
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let (mark, head) = checkbox_at(line)?;

    let mut continuation: Vec<&str> = Vec::new();
    let mut i = start + 1;
    while let Some(next) = lines.get(i) {
        if next.trim().is_empty() {
            let mut j = i + 1;
            while lines.get(j).is_some_and(|l| l.trim().is_empty()) {
                j += 1;
            }
            match lines.get(j) {
                Some(l) if l.starts_with(char::is_whitespace) => {
                    continuation.push("");
                    i = j;
                    continue;
                },
                _ => break,
            }
        }
        if !next.starts_with(char::is_whitespace) {
            break;
        }
        continuation.push(next);
        i += 1;
    }

    // The lead paragraph runs until the first blank line or nested bullet;
    // everything after it keeps its own lines, so a sub-list stays a list
    // and a second paragraph stays a paragraph.
    let breaks = |l: &str| l.trim().is_empty() || bullet_marker(l.trim_start()).is_some();
    let split = continuation
        .iter()
        .position(|l| breaks(l))
        .unwrap_or(continuation.len());
    let (wrapped, tail) = continuation.split_at(split);
    let indent = tail
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(0);

    let mut prose = vec![head.trim().to_string()];
    prose.extend(wrapped.iter().map(|l| l.trim().to_string()));
    let prose = prose.join(" ").trim().to_string();
    let (raw_id, prose) = take_backticked_id(&prose);
    Some((
        BulletEntry {
            status: checkbox_status(mark),
            raw_id,
            prose,
            tail: tail.iter().map(|l| dedent(l, indent)).collect(),
        },
        i,
    ))
}

/// `- [x] rest` → `('x', "rest")`. `-`, `*` and `+` all open a list item.
fn checkbox_at(line: &str) -> Option<(char, &str)> {
    let rest = bullet_marker(line.trim_start())?;
    let mut chars = rest.chars();
    if chars.next()? != '[' {
        return None;
    }
    let mark = chars.next()?;
    if chars.next()? != ']' {
        return None;
    }
    let rest = &rest[2 + mark.len_utf8()..];
    // The space after `]` is required: `[x]y` is not a task list item.
    rest.strip_prefix(' ').map(|r| (mark, r))
}

/// The text after a `- ` / `* ` / `+ ` list marker, if the line opens one.
fn bullet_marker(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
}

/// The glyph inside the checkbox. Anything unrecognised reads as `todo`,
/// the same under-claiming default [`parse_status`] takes.
fn checkbox_status(mark: char) -> &'static str {
    match mark {
        'x' | 'X' | '✓' => "done",
        '~' | '/' | '>' => "wip",
        '!' => "blocked",
        _ => "todo",
    }
}

/// Lift the first code span out of a bullet's prose: it is the id. Returns
/// the token (empty when the bullet had none) and the prose without it,
/// with the `—`/`–`/`-`/`:` that separated them dropped.
fn take_backticked_id(prose: &str) -> (String, String) {
    // Anchored at the start on purpose. An unanchored scan would read the
    // code span in `- [ ] Fix the `foo` handler` as an id, inventing
    // `F-foo` *and* deleting the word from the body — silently, since a
    // non-empty id also suppresses the "no id in backticks" warning.
    let Some(after) = prose.strip_prefix('`') else {
        return (String::new(), prose.to_string());
    };
    let Some(close) = after.find('`') else {
        return (String::new(), prose.to_string());
    };
    let id = after[..close].trim().to_string();
    let rest = after[close + 1..].trim_start();
    let rest = rest
        .strip_prefix('—')
        .or_else(|| rest.strip_prefix('–'))
        .or_else(|| rest.strip_prefix('-'))
        .or_else(|| rest.strip_prefix(':'))
        .unwrap_or(rest);
    (id, rest.trim().to_string())
}

/// Drop up to `n` leading whitespace `char`s, so a nested list keeps its
/// *relative* indentation when it moves into a feature body.
///
/// Counted in `char`s but cut on a byte offset — a non-breaking space,
/// which arrives with any copy-paste from a browser, is one `char` and two
/// bytes, and slicing by the count would panic mid-`char`.
fn dedent(line: &str, n: usize) -> String {
    let cut = line
        .char_indices()
        .take(n)
        .take_while(|(_, c)| c.is_whitespace())
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    line[cut..].to_string()
}

/// Longest first sentence still worth having as a summary; past it the
/// author wrote a paragraph, and [`crate::summary`]'s own truncation is the
/// better bound.
const MAX_SUMMARY_SENTENCE: usize = 200;

/// Turn one bullet into a feature.
///
/// Same warnings as the table path, because the gap is the same: a bullet
/// list states id, status and target and nothing else, so `type` and `area`
/// are guesses the adopter is told about.
fn import_bullet(
    entry: &BulletEntry,
    heading: Option<&str>,
    seen_slugs: &mut BTreeMap<String, usize>,
    plan: &mut ImportPlan,
) {
    if entry.raw_id.is_empty() && entry.prose.is_empty() {
        return;
    }
    let slug = unique_slug(&derive_slug(&entry.raw_id, &entry.prose), seen_slugs);
    let id = derive_id(&slug);
    let target = heading.map(str::to_string);
    if let Some(b) = &target {
        if !plan.buckets.contains(b) {
            plan.buckets.push(b.clone());
        }
    }
    if entry.raw_id.is_empty() {
        plan.warnings.push(format!(
            "{slug}: no `id` in backticks — slug derived from the text, rename it if you had one"
        ));
    }
    plan.warnings.push(format!(
        "{slug}: a bullet list carries no area or type — wrote `{TODO_VALUE}` and assumed `feature`"
    ));
    plan.features.push(PlannedFeature {
        contents: render_feature(
            &id,
            "feature",
            &[TODO_VALUE.to_string()],
            entry.status,
            None,
            target.as_deref(),
            &bullet_body(entry),
        ),
        slug,
        id,
    });
}

/// The body a bullet becomes: first sentence, blank line, the rest.
///
/// The paragraph is the catalog Summary (#55), and a bullet's prose is one
/// wrapped paragraph — so importing it whole would put the entire entry in
/// the cell and let the width truncation cut it mid-thought. Splitting at
/// the first sentence is what makes the bullet form, whose prose is its
/// richness, produce a catalog that still scans. Nothing is lost: the rest
/// of the paragraph and any nested list follow it in the body, and
/// `## Details` renders all of it.
///
/// Both halves are re-wrapped, because reading the bullet joined its
/// continuation lines and `## Details` reproduces a body verbatim — so a
/// source wrapped at 68 and 48 columns came out as one 104-column line the
/// adopter could not find anywhere in their own file (#71). Keeping their
/// original breaks is not on the table: the sentence boundary does not
/// align with them, and it is the split that forces the recomposition. So
/// the choice is between one long line and a re-wrapped one, and only the
/// second is lintable. `entry.tail` is left alone — nested lists and later
/// paragraphs keep their own lines, which are still the author's.
fn bullet_body(entry: &BulletEntry) -> String {
    let (summary, rest) = split_first_sentence(&entry.prose);
    let mut paragraphs = vec![rewrap(&summary)];
    if !rest.is_empty() {
        paragraphs.push(rewrap(&rest));
    }
    if !entry.tail.is_empty() {
        paragraphs.push(entry.tail.join("\n").trim_end().to_string());
    }
    paragraphs.retain(|p| !p.trim().is_empty());
    paragraphs.join("\n\n")
}

/// Wrap one recomposed paragraph to the width the generated document has
/// to fit.
///
/// **Never opens a line on a token that would start a markdown block.**
/// Wrapping moves words to column 0, where markdown reads them
/// structurally: greedy wrapping of `… specification x x x x 1. Then we
/// verify …` put `1.` at the start of a line, and CommonMark turned the
/// author's sentence into an ordered list — a silent change of meaning,
/// plus an MD032 error replacing the MD013 one this was fixing. Fixing a
/// lint defect by introducing a lint defect is the thing the 80-column
/// rule exists to prevent, so the line overflows instead, on the same
/// reasoning `wrap_words` overflows a URL longer than the budget: width is
/// a lint limit, meaning is not negotiable.
///
/// This is why it does not simply call `wrap_words`. That one wraps the
/// banner, which lives inside an HTML comment where no token is
/// structural; here every line is markdown.
fn rewrap(paragraph: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for word in paragraph.split_whitespace() {
        match lines.last_mut() {
            Some(line)
                if line.chars().count() + 1 + word.chars().count() <= BODY_WRAP_WIDTH
                    || starts_a_block(word) =>
            {
                line.push(' ');
                line.push_str(word);
            },
            _ => lines.push(word.to_string()),
        }
    }
    lines.join("\n")
}

/// Whether this token, placed at column 0, would open a markdown block
/// rather than continue a paragraph.
///
/// Deliberately over-inclusive: a false positive costs one overflowing
/// line, a false negative silently rewrites the author's prose into a
/// list, a heading, or a quote. `-` and `=` runs are here for the setext
/// case, where the *previous* line becomes the heading.
fn starts_a_block(word: &str) -> bool {
    let ordered_marker = || {
        let digits = word.trim_end_matches(['.', ')']);
        digits.len() + 1 == word.len()
            && !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit())
    };
    matches!(word, "-" | "+" | "*")
        || word.starts_with('>')
        || word.starts_with('|')
        || word.starts_with("```")
        || word.starts_with("~~~")
        || word.starts_with('<')
        || (!word.is_empty() && word.chars().all(|c| c == '#'))
        || (!word.is_empty() && word.chars().all(|c| c == '='))
        || (!word.is_empty() && word.chars().all(|c| c == '-'))
        || ordered_marker()
}

/// Tokens that end in `.` without ending a sentence.
const ABBREVIATIONS: &[&str] = &[
    "e.g", "i.e", "cf", "vs", "resp", "al", "fig", "no", "approx",
];

/// Shortest run of text that can plausibly be a whole sentence, in `char`s.
const MIN_SENTENCE_CHARS: usize = 20;

/// Split a paragraph after its first sentence, returning it and the rest.
///
/// Hand-rolled like every other scan here, and deliberately conservative:
/// a terminator must be followed by whitespace or the end of the text, may
/// carry closing brackets or quotes, must leave at least
/// [`MIN_SENTENCE_CHARS`] behind it, and must not follow an abbreviation or
/// a single-letter initial. When nothing qualifies — or the sentence runs
/// past [`MAX_SUMMARY_SENTENCE`] — the whole paragraph is the first
/// sentence, which degrades to the pre-#57 behaviour rather than cutting
/// somewhere wrong.
fn split_first_sentence(text: &str) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let mut end = i + 1;
        while matches!(chars.get(end), Some(')' | '"' | '\'' | '`' | ']' | '»')) {
            end += 1;
        }
        let terminal = chars.get(end).is_none_or(|n| n.is_whitespace());
        if !terminal || end < MIN_SENTENCE_CHARS {
            continue;
        }
        if end > MAX_SUMMARY_SENTENCE {
            break;
        }
        if ends_with_abbreviation(&chars[..i]) {
            continue;
        }
        let head: String = chars[..end].iter().collect();
        let tail: String = chars[end..].iter().collect();
        return (head.trim().to_string(), tail.trim().to_string());
    }
    (text.trim().to_string(), String::new())
}

/// Whether the word ending at this point is an abbreviation or an initial —
/// `e.g`, `Fig`, `J` in `J. Doe` — rather than the end of a sentence.
fn ends_with_abbreviation(before: &[char]) -> bool {
    let word: String = before
        .iter()
        .rev()
        .take_while(|c| !c.is_whitespace())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let word = word
        .trim_start_matches(['(', '"', '\'', '`', '['])
        .to_lowercase();
    word.chars().count() <= 1 || ABBREVIATIONS.contains(&word.as_str())
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

/// Character budget for a *derived* slug body, before the `f-` prefix.
/// `render` writes the anchor as `<a id="…"></a>`, a 13-column frame, so
/// 67 characters is where the emitted line crosses 80; this leaves room
/// for the prefix and a little headroom.
const SLUG_MAX_CHARS: usize = 60;

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
    // Five words is a bound on *count*, not on length, so five long ones
    // still produce an id that overflows the anchor line `render` emits
    // (#67 — `<a id="…"></a>` leaves 67 characters for the slug). Cut back
    // to a word boundary; a single word past the budget is kept whole, on
    // the same reasoning as `wrap_words` overflowing a long URL rather
    // than breaking it. An id the source wrote in backticks is never cut:
    // it is the author's, and truncating it would break their references.
    let body = if raw_id.is_empty() && body.chars().count() > SLUG_MAX_CHARS {
        match body[..SLUG_MAX_CHARS].rfind('-') {
            Some(cut) => body[..cut].to_string(),
            None => body,
        }
    } else {
        body
    };
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
    body: &str,
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
    let body = body.trim();
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
    // The comment opens the file, so it is the first thing an 80-column
    // lint reads — and as one line it was 180 columns wide (#67). Fenced
    // over several lines rather than wrapped inline: `-->` on its own line
    // keeps the closing delimiter off the last sentence, where a future
    // edit would push it past the limit again.
    format!(
        "<!--\nProse `roadmark import` could not attribute to a feature. \
         Move what belongs\nin a feature body into its file; the rest is a \
         candidate for a `sections`\nentry in config.toml.\n-->\n\n{trimmed}\n"
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

    /// Wrapping moves words to column 0, where markdown reads them
    /// structurally. Greedy wrapping put `1.` at the start of a line and
    /// CommonMark turned the sentence into an ordered list — MD032 in
    /// place of the MD013 the wrap was fixing, plus a silent change of
    /// meaning. The line overflows instead.
    #[test]
    fn wrapping_never_opens_a_line_on_a_block_marker() {
        let para = "Stage two flips the router as described in the referenced \
                    specification x x x x 1. Then we verify the rest of the flow.";
        let wrapped = rewrap(para);
        for line in wrapped.lines() {
            let first = line.split_whitespace().next().unwrap_or("");
            assert!(!starts_a_block(first), "line opens a block: {line:?}");
        }
        // The overflowing line is the price, and it is stated: the token
        // that would have opened the block is pulled up rather than
        // starting a line, so line one runs past the budget.
        assert!(wrapped.starts_with("Stage two"), "got {wrapped:?}");
        assert!(
            wrapped.lines().next().unwrap().ends_with("x x x x 1."),
            "got {wrapped:?}"
        );
        assert!(wrapped.lines().next().unwrap().chars().count() > 80);
        // Nothing is lost — only the break moved.
        assert_eq!(
            wrapped.split_whitespace().collect::<Vec<_>>(),
            para.split_whitespace().collect::<Vec<_>>()
        );
    }

    /// A table cell is one line by construction, so it is the likelier
    /// source of an over-wide body than a bullet — and the table is the
    /// more common import shape. #71 fixed only the bullet path.
    #[test]
    fn a_table_row_body_is_wrapped_too() {
        let plan = plan(
            "# P\n\n## v1\n\n\
             | ID | Status | Summary |\n| --- | --- | --- |\n\
             | F-alpha | done | Instrumentation dashboards consolidate telemetry \
             aggregation pipelines across every single regional deployment we run. |\n",
        );
        for line in plan.features[0].contents.lines() {
            assert!(
                line.chars().count() <= 80,
                "line is {} columns: {line:?}",
                line.chars().count()
            );
        }
    }

    /// Reading a bullet joins its continuation lines, and `## Details`
    /// reproduces a body verbatim — so without re-wrapping, a source the
    /// adopter had wrapped at 68 and 48 columns came out as one 104-column
    /// line that exists nowhere in their file (#71). Every word is theirs
    /// and there is nothing for them to fix, which is exactly the shape of
    /// defect the 80-column rule exists to prevent.
    #[test]
    fn an_imported_file_fits_the_lint_the_generated_document_must_pass() {
        let plan = plan(
            "# P\n\n## v1.0\n\n\
             - [x] Instrumentation dashboards consolidate telemetry aggregation\n  \
             pipelines across every regional deployment. They also fan out over \
             each availability zone, which is where the per-work cost hides.\n",
        );
        let contents = &plan.features[0].contents;
        for line in contents.lines() {
            assert!(
                line.chars().count() <= 80,
                "line is {} columns: {line:?}",
                line.chars().count()
            );
        }
        // Re-wrapped, not truncated: every word survives, and the only
        // thing that changed is where the lines break.
        let body = contents.split("+++\n").nth(2).unwrap();
        assert_eq!(
            body.split_whitespace().collect::<Vec<_>>().join(" "),
            "Instrumentation dashboards consolidate telemetry aggregation \
             pipelines across every regional deployment. They also fan out \
             over each availability zone, which is where the per-work cost \
             hides."
        );
        // The summary sentence still opens its own paragraph.
        assert!(
            contents.contains("regional deployment.\n\nThey also fan out"),
            "got {contents}"
        );
    }

    /// Five words is a bound on count, not on length. Five long ones used
    /// to produce a slug whose `<a id="…"></a>` line broke 80 columns in
    /// the generated document (#67), which is a lint failure the adopter
    /// cannot fix — the id came from the tool, not from them.
    #[test]
    fn a_derived_slug_cannot_overflow_the_anchor_line() {
        // Five words, 79 characters — under the word budget, over the
        // character one. The cut lands on a word boundary.
        let slug = derive_slug(
            "",
            "Instrumentation instrumentation instrumentation instrumentation \
             instrumentation",
        );
        assert_eq!(slug, "f-instrumentation-instrumentation-instrumentation");
        assert!(classify_slug(&slug).is_ok());
        // Five words that fit are left alone — the bound is a ceiling, not
        // a target, and a shorter id than the source justifies is a loss.
        assert_eq!(
            derive_slug(
                "",
                "Instrumentation dashboards consolidate telemetry aggregation"
            ),
            "f-instrumentation-dashboards-consolidate-telemetry-aggregation"
        );
        // 13 columns of `<a id="` … `"></a>` frame around whichever it is.
        for s in [
            &slug,
            &derive_slug(
                "",
                "Instrumentation dashboards consolidate telemetry aggregation",
            ),
        ] {
            assert!(s.chars().count() + 13 <= 80, "got {s}");
        }
        // An id the source wrote itself is never cut: it is the author's,
        // and truncating it would break the references pointing at it.
        let given = "F-instrumentation-dashboards-consolidate-telemetry-aggregation-x";
        assert_eq!(derive_slug(given, ""), given.to_lowercase());
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

    /// The shape a hand-written `ROADMAP.md` most often takes: checkbox
    /// bullets under bucket headings, no table anywhere (#57).
    #[test]
    fn checkbox_bullets_import_when_the_document_has_no_table() {
        let plan = plan(
            "### Must\n\n\
             - [x] `F-app-shell` — window, lifecycle, bounds (menu: deferred to M3 with\n  \
             the keymap). The deferral left one visible gap on macOS.\n\
             - [ ] `F-packaging-ci` — signed mac/win/linux builds + CI gate\n",
        );
        let slugs: Vec<&str> = plan.features.iter().map(|f| f.slug.as_str()).collect();
        assert_eq!(slugs, vec!["f-app-shell", "f-packaging-ci"]);
        assert_eq!(plan.buckets, vec!["Must"]);

        let shell = &plan.features[0].contents;
        assert!(shell.contains("status = \"done\""), "{shell}");
        assert!(shell.contains("target = [\"Must\"]"), "{shell}");
        // The wrapped continuation line is part of the same sentence…
        assert!(
            shell.contains("deferred to M3 with the keymap)."),
            "{shell}"
        );
        // …and the sentence after it is a paragraph of its own, so the
        // catalog Summary is a summary rather than the whole entry.
        assert!(
            shell.contains(").\n\nThe deferral left one visible gap on macOS."),
            "{shell}"
        );
        assert!(plan.features[1].contents.contains("status = \"todo\""));
    }

    /// A document that states its features in a table may still hold
    /// checklists; reading those as features would invent rows.
    #[test]
    fn bullets_stay_prose_when_the_document_also_has_a_feature_table() {
        let plan = plan(
            "| ID | Summary |\n| --- | --- |\n| F-a | A. |\n\n\
             ## Release checklist\n\n- [ ] tag the merge commit\n",
        );
        let slugs: Vec<&str> = plan.features.iter().map(|f| f.slug.as_str()).collect();
        assert_eq!(slugs, vec!["f-a"]);
        assert!(
            plan.leftovers.contains("- [ ] tag the merge commit"),
            "{}",
            plan.leftovers
        );
    }

    /// roadmark has no sub-features, so a nested bullet stays prose in its
    /// parent's body rather than becoming an id nobody wrote.
    #[test]
    fn a_nested_bullet_stays_in_its_parents_body() {
        let plan = plan(
            "- [ ] `F-parent` — the parent entry, stated in full.\n  \
             - [ ] a sub-item\n    - [ ] deeper still\n",
        );
        assert_eq!(plan.features.len(), 1);
        let body = &plan.features[0].contents;
        assert!(body.contains("- [ ] a sub-item"), "{body}");
        // Relative nesting survives the move into the body.
        assert!(body.contains("\n  - [ ] deeper still"), "{body}");
    }

    #[test]
    fn a_bullet_without_a_backticked_id_derives_one_and_says_so() {
        let plan = plan("- [ ] Regions are now a contiguous partition of space.\n");
        assert_eq!(plan.features[0].slug, "f-regions-are-now-a-contiguous");
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("no `id` in backticks")));
    }

    /// A code span in the middle of a bullet is prose. Reading it as an id
    /// would invent `F-foo` *and* delete the word from the body, and the
    /// non-empty id would suppress the warning that says a slug was
    /// guessed — the one thing that would have shown it.
    #[test]
    fn a_code_span_inside_the_prose_is_not_an_id() {
        let plan = plan("- [ ] Fix the `foo` handler so it stops dropping events.\n");
        assert_eq!(plan.features[0].slug, "f-fix-the-foo-handler-so");
        assert!(plan.features[0]
            .contents
            .contains("Fix the `foo` handler so it stops dropping events."));
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("no `id` in backticks")));
    }

    /// A non-breaking space arrives with any copy-paste from a browser. It
    /// is one `char` and two bytes, so a `char`-counted dedent used as a
    /// byte offset panics mid-`char` — an import crash, not an exit code.
    #[test]
    fn non_breaking_indentation_does_not_panic() {
        let plan = plan("- [ ] `F-a` — a thing worth doing.\n\u{a0}- [ ] sub-item\n");
        assert_eq!(plan.features.len(), 1);
        assert!(plan.features[0].contents.contains("- [ ] sub-item"));
    }

    /// A bullet roadmap carries checklists too. When the document names
    /// its features in backticks, the bullets that don't are prose — not
    /// features with a slug invented from a chore.
    #[test]
    fn a_checklist_beside_identified_bullets_stays_prose() {
        let plan = plan(
            "## Must\n\n- [ ] `F-a` — a real feature.\n\n\
             ## Release checklist\n\n- [ ] tag the merge commit\n- [ ] publish to crates.io\n",
        );
        let slugs: Vec<&str> = plan.features.iter().map(|f| f.slug.as_str()).collect();
        assert_eq!(slugs, vec!["f-a"]);
        assert!(
            plan.leftovers.contains("tag the merge commit"),
            "{}",
            plan.leftovers
        );
        // …and the checklist heading is not a release bucket.
        assert_eq!(plan.buckets, vec!["Must"]);
    }

    /// With no id anywhere the document has no such convention, so every
    /// bullet is a feature and the slug is derived, as before.
    #[test]
    fn every_bullet_is_a_feature_when_none_carries_an_id() {
        let plan = plan("- [ ] first thing to do\n- [x] second thing to do\n");
        assert_eq!(plan.features.len(), 2);
    }

    /// A loose list — blank lines between an item and its sub-items — is
    /// ordinary markdown. Ending the entry at the blank would strand the
    /// sub-items in leftovers.
    #[test]
    fn a_loose_list_keeps_its_sub_items() {
        let plan = plan(
            "- [x] `F-a` — thing one.\n\n  - [ ] sub after blank\n\n\
             - [ ] `F-b` — thing two.\n",
        );
        let slugs: Vec<&str> = plan.features.iter().map(|f| f.slug.as_str()).collect();
        assert_eq!(slugs, vec!["f-a", "f-b"]);
        assert!(
            plan.features[0].contents.contains("- [ ] sub after blank"),
            "{}",
            plan.features[0].contents
        );
        assert!(
            !plan.leftovers.contains("sub after blank"),
            "{}",
            plan.leftovers
        );
    }

    #[test]
    fn the_checkbox_glyph_is_the_status() {
        assert_eq!(checkbox_status('x'), "done");
        assert_eq!(checkbox_status('X'), "done");
        assert_eq!(checkbox_status('~'), "wip");
        assert_eq!(checkbox_status('!'), "blocked");
        assert_eq!(checkbox_status(' '), "todo");
        // `[x]y` is not a task list item — the space is required.
        assert!(checkbox_at("- [x]nope").is_none());
        assert_eq!(checkbox_at("* [ ] yes").unwrap(), (' ', "yes"));
    }

    #[test]
    fn the_first_sentence_survives_abbreviations_and_versions() {
        let (head, tail) = split_first_sentence(
            "Ship the thing on v0.2.1, e.g. behind a flag. Then remove the flag.",
        );
        assert_eq!(head, "Ship the thing on v0.2.1, e.g. behind a flag.");
        assert_eq!(tail, "Then remove the flag.");

        // A paragraph with no sentence break stays whole — the width
        // truncation in `render` is the right bound, not a wrong cut.
        let (head, tail) = split_first_sentence("no terminator anywhere in here");
        assert_eq!(head, "no terminator anywhere in here");
        assert!(tail.is_empty());
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
