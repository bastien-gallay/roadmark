//! `validate` subcommand: schema, slug uniqueness, cross-references,
//! anchor drift.
//!
//! Pure read-only — never mutates the source tree. Collects all
//! issues into a `ValidationReport` instead of bailing on the first
//! parse error, so a single run surfaces every problem.
//!
//! Findings come in two tiers. **Hard errors** fail the run (exit 1):
//! the tree would generate a roadmap that is wrong, not merely thin.
//! **Warnings** are printed and counted but never change the exit code —
//! they name work a human still owes the file (an unwritten body, a prose
//! mention of an id nobody defines). Keeping them soft is what lets a
//! migration proceed — scaffold the files, fill the bodies second — instead
//! of refusing the tree for the whole of that interval.

use crate::add::classify_slug;
use crate::{
    anchor_id, axis_in_use, feature_md_paths, load_config, parse_feature, render, sort_features,
    Condition, Config, Feature, FieldKind, Frontmatter, LoadedSection,
};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct ValidationReport {
    /// `.roadmap/` source tree is absent on this checkout (e.g. CI, or a
    /// worktree where the source lives elsewhere). Skipped, not failed.
    pub source_missing: bool,
    /// An **explicitly passed** `--root` with no `features/` under it.
    /// Distinct from `source_missing`: the user named a tree, so silence
    /// would be a clean pass for a run that checked nothing (#31).
    pub missing_root: Option<PathBuf>,
    pub schema_errors: Vec<SchemaError>,
    pub duplicate_ids: Vec<String>,
    pub anchor_collisions: Vec<AnchorCollision>,
    /// Markdown links to a feature anchor no feature defines — a dead
    /// link in the published `ROADMAP.md` (#36).
    pub dangling_links: Vec<DanglingRef>,
    /// Anchors present in `ROADMAP.md` but absent from a fresh regen
    /// — inbound links to the roadmap would 404 after the next regen.
    pub anchors_missing_from_regen: Vec<String>,
    /// Anchors present in regen but absent from on-disk `ROADMAP.md`
    /// — release-prep regen never ran (or wasn't committed).
    pub anchors_missing_from_disk: Vec<String>,
    /// Soft findings: printed, never counted by [`Self::has_hard_errors`].
    pub warnings: Vec<Warning>,
}

#[derive(Debug)]
pub struct SchemaError {
    pub path: PathBuf,
    pub message: String,
}

/// A finding that is worth naming but must not fail the run.
#[derive(Debug)]
pub struct Warning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug)]
pub struct AnchorCollision {
    pub anchor: String,
    pub ids: Vec<String>,
}

/// A cross-reference to a feature id nothing declares.
#[derive(Debug)]
pub struct DanglingRef {
    pub path: PathBuf,
    /// The reference exactly as the body wrote it (`F-foo`, `f-foo`).
    pub reference: String,
}

impl ValidationReport {
    /// Nothing at all to say — no errors, no drift, **and no warnings**.
    /// A warnings-only report is not clean (there is something to read)
    /// yet still exits 0; see [`Self::has_hard_errors`].
    pub fn is_clean(&self) -> bool {
        self.source_missing
            || (self.missing_root.is_none()
                && self.schema_errors.is_empty()
                && self.duplicate_ids.is_empty()
                && self.anchor_collisions.is_empty()
                && self.dangling_links.is_empty()
                && self.warnings.is_empty()
                && !self.has_drift())
    }

    pub fn has_drift(&self) -> bool {
        !self.anchors_missing_from_regen.is_empty() || !self.anchors_missing_from_disk.is_empty()
    }

    pub fn has_hard_errors(&self) -> bool {
        self.missing_root.is_some()
            || !self.schema_errors.is_empty()
            || !self.duplicate_ids.is_empty()
            || !self.anchor_collisions.is_empty()
            || !self.dangling_links.is_empty()
    }

    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        if self.source_missing {
            out.push_str("validate: skipped (no `.roadmap/` source on this checkout)\n");
            return out;
        }
        if let Some(root) = &self.missing_root {
            let _ = writeln!(
                out,
                "no `.roadmap/` source under `--root {}` — nothing was validated",
                root.display()
            );
            return out;
        }
        if self.is_clean() {
            out.push_str("validate: clean\n");
            return out;
        }
        if !self.schema_errors.is_empty() {
            let _ = writeln!(out, "schema errors ({}):", self.schema_errors.len());
            for e in &self.schema_errors {
                let _ = writeln!(out, "  {}: {}", e.path.display(), e.message);
            }
        }
        if !self.duplicate_ids.is_empty() {
            let _ = writeln!(out, "duplicate ids ({}):", self.duplicate_ids.len());
            for id in &self.duplicate_ids {
                let _ = writeln!(out, "  {id}");
            }
        }
        if !self.anchor_collisions.is_empty() {
            let _ = writeln!(out, "anchor collisions ({}):", self.anchor_collisions.len());
            for c in &self.anchor_collisions {
                let _ = writeln!(out, "  anchor `{}` ← ids {:?}", c.anchor, c.ids);
            }
        }
        if !self.dangling_links.is_empty() {
            let _ = writeln!(
                out,
                "dangling links ({}) — the generated roadmap would link to a \
                 missing anchor:",
                self.dangling_links.len()
            );
            for r in &self.dangling_links {
                let _ = writeln!(
                    out,
                    "  {}: link to unknown feature id {}",
                    r.path.display(),
                    r.reference
                );
            }
        }
        if !self.anchors_missing_from_regen.is_empty() {
            let _ = writeln!(
                out,
                "anchors on disk but not in regen ({}) — broken inbound links after regen:",
                self.anchors_missing_from_regen.len()
            );
            for a in &self.anchors_missing_from_regen {
                let _ = writeln!(out, "  {a}");
            }
        }
        if !self.anchors_missing_from_disk.is_empty() {
            let _ = writeln!(
                out,
                "anchors in regen but not on disk ({}) — `ROADMAP.md` needs regen:",
                self.anchors_missing_from_disk.len()
            );
            for a in &self.anchors_missing_from_disk {
                let _ = writeln!(out, "  {a}");
            }
        }
        if !self.warnings.is_empty() {
            let _ = writeln!(out, "warnings ({}):", self.warnings.len());
            for w in &self.warnings {
                let _ = writeln!(out, "  {}: {}", w.path.display(), w.message);
            }
            // Spell out the exit code rather than leaving the reader to
            // infer it: a report that is not "clean" but does not fail is
            // the one outcome the two existing lines could not express.
            if !self.has_hard_errors() && !self.has_drift() {
                let _ = writeln!(
                    out,
                    "validate: no errors, {} warning(s) — warnings do not fail the run",
                    self.warnings.len()
                );
            }
        }
        out
    }
}

/// `root_explicit` is whether the user actually typed `--root`. It is the
/// whole difference between the documented escape hatch and a typo: an
/// absent `features/` under the *default* root means "this checkout has no
/// source, skip"; under a root the user named it means "the tree you asked
/// for is not there", and reporting clean would be a pass for a run that
/// verified nothing (#31).
pub fn validate(root: &Path, roadmap_md: &Path, root_explicit: bool) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    let features_dir = root.join("features");
    if !features_dir.is_dir() {
        if root_explicit {
            report.missing_root = Some(root.to_path_buf());
        } else {
            // No source on this checkout — silent-pass. Lets the recipe
            // run on checkouts where `.roadmap/` is absent (e.g. CI)
            // without manufacturing an error.
            report.source_missing = true;
        }
        return Ok(report);
    }

    let config = load_config(root).context("loading config.toml")?;

    let mut parsed: Vec<(PathBuf, Feature)> = Vec::new();
    for path in feature_md_paths(&features_dir)? {
        match std::fs::read_to_string(&path) {
            Ok(src) => match parse_feature(&src) {
                Ok(f) => {
                    check_feature_fields(&path, &f.frontmatter, &config, &mut report);
                    check_body(&path, &f.body, &mut report);
                    parsed.push((path.clone(), f));
                },
                Err(e) => report.schema_errors.push(SchemaError {
                    path: path.clone(),
                    message: format!("{e:#}"),
                }),
            },
            Err(e) => report.schema_errors.push(SchemaError {
                path: path.clone(),
                message: format!("read failed: {e}"),
            }),
        }
    }
    let section_files = check_sections(root, &config, &mut report);
    let section_bodies: Vec<(PathBuf, String)> = section_files
        .iter()
        .map(|(path, s)| (path.clone(), s.body.clone()))
        .collect();
    check_dangling_refs(&parsed, &section_bodies, &mut report);
    let features: Vec<Feature> = parsed.into_iter().map(|(_, f)| f).collect();

    // Config checks need the features: a `[fields.X]` section is required
    // by what the tree actually holds, not unconditionally.
    let config_path = root.join("config.toml");
    check_config_fields(&config_path, &config, &features, &mut report);
    check_versions(&config_path, &config, &mut report);

    let mut id_counts: HashMap<String, usize> = HashMap::new();
    for f in &features {
        *id_counts.entry(f.frontmatter.id.clone()).or_default() += 1;
    }
    for (id, n) in &id_counts {
        if *n > 1 {
            report.duplicate_ids.push(id.clone());
        }
    }
    report.duplicate_ids.sort();

    let mut anchor_to_ids: HashMap<String, BTreeSet<String>> = HashMap::new();
    for f in &features {
        anchor_to_ids
            .entry(anchor_id(&f.frontmatter.id))
            .or_default()
            .insert(f.frontmatter.id.clone());
    }
    for (anchor, ids) in anchor_to_ids {
        if ids.len() > 1 {
            report.anchor_collisions.push(AnchorCollision {
                anchor,
                ids: ids.into_iter().collect(),
            });
        }
    }
    report
        .anchor_collisions
        .sort_by(|a, b| a.anchor.cmp(&b.anchor));

    if !roadmap_md.is_file() {
        bail!("ROADMAP.md not found at: {}", roadmap_md.display());
    }
    let on_disk = std::fs::read_to_string(roadmap_md)
        .with_context(|| format!("reading {}", roadmap_md.display()))?;
    let on_disk_anchors = extract_anchors(&on_disk);

    let mut sorted = features;
    sort_features(&mut sorted, &config);
    // The regen must carry the same sections the on-disk file does, or a
    // hand-written block holding an `<a id>` would read as drift.
    let sections: Vec<LoadedSection> = section_files.into_iter().map(|(_, s)| s).collect();
    let regen = render(&sorted, &config, &sections);
    let regen_anchors = extract_anchors(&regen);

    report.anchors_missing_from_regen = on_disk_anchors
        .difference(&regen_anchors)
        .cloned()
        .collect();
    report.anchors_missing_from_disk = regen_anchors
        .difference(&on_disk_anchors)
        .cloned()
        .collect();

    Ok(report)
}

/// Config-driven per-feature schema checks: every declared field's value(s)
/// must be in its allow-list, `required_when` conditionals must hold, and
/// `area` must carry at least one value. One `SchemaError` per breach, in
/// stable (`BTreeMap`) field order so runs are reproducible.
fn check_feature_fields(
    path: &Path,
    fm: &Frontmatter,
    config: &Config,
    report: &mut ValidationReport,
) {
    let mut err = |message: String| {
        report.schema_errors.push(SchemaError {
            path: path.to_path_buf(),
            message,
        });
    };
    for (name, spec) in &config.fields {
        // A reserved name can't be constrained here and `check_config_fields`
        // already rejects the declaration. Skipping keeps that one config
        // error from also manufacturing a bogus "`status` is required" on
        // every feature file in the tree.
        if Frontmatter::RESERVED_FIELD_NAMES.contains(&name.as_str()) {
            continue;
        }
        // Every remaining name is declared, so "the feature doesn't carry
        // it" is an *empty* value list, not a reason to skip: a declared
        // field the feature omits is precisely what `required_when` is about.
        let values = fm.field_values(name).unwrap_or_default();
        if let Some(required_when) = &spec.required_when {
            // ALL declared conditions must hold (AND) for the field to be
            // required — a condition matches when the referenced field
            // currently carries one of the expected values. Honours every
            // key, not just `type`.
            let all_match = required_when.iter().all(|(cond_field, cond)| {
                fm.field_values(cond_field)
                    .is_some_and(|vals| cond.matches(&vals))
            });
            if all_match && values.is_empty() {
                // `required_when = {}` is the unconditional form (an empty
                // AND is vacuously true) — don't emit a dangling "when".
                if required_when.is_empty() {
                    err(format!("`{name}` is required"));
                } else {
                    err(format!(
                        "`{name}` is required when {}",
                        describe_condition(required_when)
                    ));
                }
            }
        }
        if !spec.multi && values.len() > 1 {
            err(format!(
                "`{name}` accepts a single value but {} were given",
                values.len()
            ));
        }
        // A closed value set and a shape check are alternatives: `values`
        // when the project can enumerate them, `kind` when it can't (a
        // tracking issue number has no value set to declare).
        for v in &values {
            match spec.kind {
                Some(kind) => {
                    if let Err(why) = check_kind(kind, v) {
                        err(format!("`{name}` value {v:?} {why}"));
                    }
                },
                None => {
                    if !spec.values.iter().any(|allowed| allowed == v) {
                        err(format!(
                            "unknown `{name}` value {v:?} (allowed: {})",
                            spec.values.join(", ")
                        ));
                    }
                },
            }
        }
    }
    for (name, value) in &fm.extra {
        // A table has no sensible cell rendering: `toml_values` would
        // stringify it back to its own source text and drop `{ x = 1 }`
        // verbatim into the catalog. Reject the shape rather than let a
        // `kind = "string"` check wave it through as "non-empty".
        let tabular = matches!(value, toml::Value::Table(_))
            || matches!(value, toml::Value::Array(items)
                if items.iter().any(|i| matches!(i, toml::Value::Table(_))));
        if tabular && config.fields.contains_key(name) {
            err(format!(
                "`{name}` is a table — declared fields take scalars, or a list of them"
            ));
        }
    }
    for name in crate::undeclared_fields(fm, config) {
        // The guarantee `deny_unknown_fields` used to give `Frontmatter`,
        // now that project-declared fields force `serde(flatten)`. Every
        // axis is optional, so an undeclared key would otherwise read as
        // an absent field and quietly drop out of the document.
        err(format!(
            "unknown frontmatter key `{name}` — declare it under \
             `[fields.{name}]` in config.toml, or fix the spelling"
        ));
    }
    if fm.area.is_empty() {
        err("`area` must list at least one value".to_string());
    }
}

/// Sorted, human-readable rendering of a `required_when` condition set, so
/// error messages are deterministic regardless of `HashMap` iteration order.
fn describe_condition(cond: &BTreeMap<String, Condition>) -> String {
    cond.iter()
        .map(|(k, v)| format!("{k} = {}", v.describe()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Does one value satisfy a declared [`FieldKind`]? `Err` carries the
/// tail of the message ("is not an integer"), so the caller supplies the
/// field name and the value once.
///
/// Shallow on purpose: roadmark carries no regex dependency, and these
/// four shapes are what a roadmap actually needs. Anything finer is the
/// project's own CI.
fn check_kind(kind: FieldKind, value: &str) -> Result<(), &'static str> {
    match kind {
        FieldKind::Integer => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "is not an integer"),
        FieldKind::String => (!value.trim().is_empty()).then_some(()).ok_or("is empty"),
        FieldKind::Url => (value.starts_with("https://") || value.starts_with("http://"))
            .then_some(())
            .ok_or("is not an http(s) URL"),
        // `#42` and `42` are the same issue; a roadmap writes both.
        FieldKind::IssueRef => value
            .trim_start_matches('#')
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .map(|_| ())
            .ok_or("is not an issue reference (a positive integer, optionally `#`-prefixed)"),
    }
}

/// The body is the summary field, just not declared as one: `render` takes
/// the catalog's `Summary` cell from its first non-empty line. An empty (or
/// whitespace-only) body therefore produces a row that links somewhere and
/// says nothing, plus a `## Details` heading with nothing under it (#38).
///
/// A **warning**, not an error, because this is the normal shape of work in
/// progress: migrating a hand-written roadmap means scaffolding files first
/// and filling bodies second, and a hard error would refuse the tree for the
/// whole of that interval. The warning names the rows still owed a sentence
/// instead.
///
/// The threshold is emptiness only. A minimum length would also catch a body
/// that just says `TODO` — which is what `add`'s own scaffold writes — but
/// picking the number is a judgement the schema should not make silently, so
/// a placeholder counts as written.
fn check_body(path: &Path, body: &str, report: &mut ValidationReport) {
    if body.trim().is_empty() {
        report.warnings.push(Warning {
            path: path.to_path_buf(),
            message: "empty body — the catalog `Summary` cell comes from the first \
                      non-empty line, so this feature renders a blank row"
                .to_string(),
        });
    }
}

/// Does `token` have the shape of a feature id? Delegates to
/// `add::classify_slug` (on the lowercased token, since ids are their
/// anchors capitalised) so "what a feature id looks like" keeps one
/// definition: `f-<kebab-name>`, or the legacy `f<digits>`.
fn looks_like_feature_id(token: &str) -> bool {
    classify_slug(&token.to_lowercase()).is_ok()
}

/// How a body named a feature id. Ordered by severity — a link wins over a
/// bare mention of the same id in the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RefForm {
    /// `F-foo` in prose or backticks.
    Bare,
    /// `](#f-foo)` — a markdown link to the feature's anchor.
    Link,
}

/// Report cross-references to feature ids nothing declares (#36).
///
/// Anchor drift does **not** already catch this: drift compares a fresh
/// regen against the committed `ROADMAP.md`, and the regen embeds the same
/// dead link the source does, so the two agree and the check stays silent.
/// This is the only check that asks whether a reference points at anything.
///
/// Two forms, two tiers:
///
/// - a **link** (`](#f-foo)`) is a hard error — it ships a broken anchor in
///   the published roadmap, the same user-visible failure anchor drift
///   exists to prevent;
/// - a **bare token** (`F-foo`, backticked or not) is a warning — prose
///   legitimately names things that are not features, so failing the run on
///   one would be noise.
///
/// A link target only counts as a feature reference when it has the shape
/// of a feature id ([`looks_like_feature_id`]): `](#installation)` is a
/// link to a document section, not a dead feature ref, and reporting every
/// non-feature anchor would be wrong. The cost of that rule is that a
/// section anchor that happens to be shaped like a feature id is
/// indistinguishable from a real one and gets reported — the shape *is* the
/// only signal available, and shipping `#f-…` section anchors alongside a
/// roadmap is itself ambiguous.
/// `sections` carries the hand-written narrative blocks. They are scanned
/// on the same terms as feature bodies because they are where cross-feature
/// prose lives — which makes them the likeliest home for a link to a
/// feature, and the one place `rename` used to miss.
fn check_dangling_refs(
    parsed: &[(PathBuf, Feature)],
    sections: &[(PathBuf, String)],
    report: &mut ValidationReport,
) {
    let declared: BTreeSet<String> = parsed
        .iter()
        .map(|(_, f)| anchor_id(&f.frontmatter.id))
        .collect();
    let bodies = parsed
        .iter()
        .map(|(path, f)| (path, f.body.as_str()))
        .chain(sections.iter().map(|(path, body)| (path, body.as_str())));
    for (path, body) in bodies {
        for (anchor, (form, as_written)) in scan_feature_refs(body) {
            if declared.contains(&anchor) {
                continue;
            }
            match form {
                RefForm::Link => report.dangling_links.push(DanglingRef {
                    path: path.clone(),
                    reference: as_written,
                }),
                RefForm::Bare => report.warnings.push(Warning {
                    path: path.clone(),
                    message: format!("reference to unknown feature id {as_written}"),
                }),
            }
        }
    }
}

/// Every feature-id-shaped reference in `body`, keyed by [`anchor_id`] so
/// `F-Foo` and `#f-foo` are one reference, not two — the same lowercasing
/// rule the renderer and `rename` use, never a second one.
///
/// The value keeps the strongest form seen and the text as written, so
/// `[F-foo](#f-foo)` is reported once, as the link it is. Manual scanner —
/// the shapes are fixed and narrow, and (like `extract_anchors` and
/// `rename::replace_token`) don't justify a regex dep.
fn scan_feature_refs(body: &str) -> BTreeMap<String, (RefForm, String)> {
    let mut found: BTreeMap<String, (RefForm, String)> = BTreeMap::new();
    for target in link_targets(body) {
        record_ref(&mut found, RefForm::Link, &target);
    }
    for token in token_runs(body) {
        record_ref(&mut found, RefForm::Bare, token);
    }
    found
}

fn record_ref(found: &mut BTreeMap<String, (RefForm, String)>, form: RefForm, token: &str) {
    if !looks_like_feature_id(token) {
        return;
    }
    let entry = found
        .entry(anchor_id(token))
        .or_insert_with(|| (form, token.to_string()));
    if form > entry.0 {
        *entry = (form, token.to_string());
    }
}

/// Anchor targets of markdown links: the text between `](#` and the next
/// `)`. Kept verbatim (not trimmed to token chars) so a target like
/// `some_section` fails the feature-id shape test rather than being
/// truncated into one that passes it.
///
/// Code spans are blanked first ([`mask_code_spans`]). Prose that *quotes*
/// the link syntax — a body explaining that `](#f-foo)` is a dead link, or
/// a roadmap entry documenting this very check — is documentation, not a
/// link, and must not raise a hard error. Found by dogfooding: the first
/// feature file written about this check failed its own rule.
///
/// Bare tokens are deliberately still scanned inside code spans: a
/// backticked `` `F-foo` `` is a real mention, and it is only a warning.
fn link_targets(body: &str) -> Vec<String> {
    const OPEN: &str = "](#";
    let masked = mask_code_spans(body);
    let mut out = Vec::new();
    let mut rest = masked.as_str();
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        match after.find(')') {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            },
            None => break,
        }
    }
    out
}

/// Replace the contents of every backtick-delimited code span with spaces,
/// keeping byte length and every other character intact.
///
/// A run of N backticks opens a span that the next run of exactly N
/// backticks closes — the CommonMark rule, which is what lets `` `a` ``
/// and ```` ``a`b`` ```` both work, and what makes a fenced block (```)
/// just a long span. An unterminated run opens nothing, so a lone backtick
/// in prose does not swallow the rest of the body.
fn mask_code_spans(body: &str) -> String {
    let bytes: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != '`' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let open = run_len(&bytes, i);
        match closing_run(&bytes, i + open, open) {
            Some(close) => {
                // Blank the fence and its contents; a `)` or `](#` inside
                // is now invisible to the scan above.
                for _ in i..close + open {
                    out.push(' ');
                }
                i = close + open;
            },
            // Unterminated: it is an ordinary character after all.
            None => {
                for _ in 0..open {
                    out.push('`');
                }
                i += open;
            },
        }
    }
    out
}

/// Length of the backtick run starting at `start`.
fn run_len(chars: &[char], start: usize) -> usize {
    chars[start..].iter().take_while(|&&c| c == '`').count()
}

/// Index of the next run of *exactly* `want` backticks at or after `from`.
fn closing_run(chars: &[char], from: usize, want: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '`' {
            let n = run_len(chars, i);
            if n == want {
                return Some(i);
            }
            i += n;
        } else {
            i += 1;
        }
    }
    None
}

/// Maximal runs of slug/id characters, i.e. whole tokens. Splitting on the
/// same `is_token_char` rule `rename::replace_token` uses is what keeps
/// `F-foo` from matching inside `F-foobar` (or `CRLF-authored`).
fn token_runs(body: &str) -> Vec<&str> {
    body.split(|c: char| !crate::rename::is_token_char(c))
        .filter(|t| !t.is_empty())
        .collect()
}

/// Read every declared narrative section, reporting the ones that can't
/// be read instead of aborting the run.
///
/// A missing section file is a hard error: `generate` would fail outright,
/// so a `validate` that passed would be promising a document the very next
/// command refuses to produce. Returning what *did* load lets the anchor
/// diff run against the closest thing to the real output — the run is
/// already failing, and a second, bogus drift report on top of the real
/// error helps nobody.
fn check_sections(
    root: &Path,
    config: &Config,
    report: &mut ValidationReport,
) -> Vec<(PathBuf, LoadedSection)> {
    let config_path = root.join("config.toml");
    let mut loaded = Vec::new();
    for section in &config.sections {
        let path = match crate::section_path(root, &section.file) {
            Ok(p) => p,
            Err(e) => {
                report.schema_errors.push(SchemaError {
                    path: config_path.clone(),
                    message: format!("{e:#}"),
                });
                continue;
            },
        };
        match std::fs::read_to_string(&path) {
            Ok(body) => loaded.push((
                path.clone(),
                LoadedSection {
                    slot: section.slot,
                    body,
                },
            )),
            Err(e) => report.schema_errors.push(SchemaError {
                path: config_path.clone(),
                message: format!(
                    "declared section `{}` cannot be read ({e}) — `generate` would fail",
                    section.file
                ),
            }),
        }
    }
    loaded
}

/// `versions` is a bucket *order*: a sort rank, and since #35 also the
/// section order under `split_by_bucket`. Two ways to write one that no
/// single behaviour can honour (#47).
///
/// **A repeated entry.** Sorting and grouping each have to pick which
/// occurrence is the real rank; they now both pick the first
/// ([`index_of`](crate::index_of), `catalog_groups`), so the output is at
/// least coherent — but the config still describes a document order it
/// doesn't have, and saying so is the whole promise of this command.
///
/// **A collision with a heading `render` writes itself.** Under
/// `split_by_bucket` every project-supplied name becomes a `##` heading —
/// each bucket, and the tail group's label — in a document that already
/// holds [`structural_heading`]s. Any pair landing on the same text emits
/// the same `##` twice: ambiguous navigation, and MD024 for anyone linting
/// their generated `ROADMAP.md`. Three pairs, all checked: a bucket
/// against a structural heading, `unbucketed_label` against a structural
/// heading, and a bucket against `unbucketed_label`.
///
/// The collision checks are gated on `split_by_bucket` because flat mode
/// emits neither a bucket heading nor a tail one, so there is nothing to
/// collide with and the error would be about a document the tool isn't
/// producing.
fn check_versions(config_path: &Path, config: &Config, report: &mut ValidationReport) {
    let mut err = |message: String| {
        report.schema_errors.push(SchemaError {
            path: config_path.to_path_buf(),
            message,
        });
    };
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for v in &config.versions {
        if seen.insert(v.as_str()) {
            unique.push(v);
        } else {
            err(format!(
                "`versions` repeats `{v}` — the bucket order is a sort rank and \
                 (under `split_by_bucket`) the section order, so a value can only \
                 hold one position; the first occurrence is the one honoured"
            ));
        }
    }
    if !config.split_by_bucket {
        return;
    }
    // The tail group is a heading like any other, so it is checked as a
    // heading and not only as something buckets may collide *with*:
    // `unbucketed_label = "Details"` needs no `versions` entry at all to
    // put a second `## Details` in the document.
    let tail = config.unbucketed_heading();
    if let Some(what) = structural_heading(tail) {
        err(format!(
            "`unbucketed_label` resolves to `{tail}`, which is {what} — \
             `generate` would emit two `## {tail}` headings"
        ));
    }
    // Over the deduplicated list: a value written twice is one heading,
    // and one collision, however many times it was declared.
    for v in unique {
        let clash = structural_heading(v).map(str::to_string).or_else(|| {
            (v == tail).then(|| match &config.unbucketed_label {
                Some(_) => "`unbucketed_label`".to_string(),
                None => format!("the default `unbucketed_label` (`{tail}`)"),
            })
        });
        if let Some(clash) = clash {
            err(format!(
                "`versions` declares `{v}`, which collides with {clash} — \
                 `generate` would emit two `## {v}` headings"
            ));
        }
    }
}

/// Names `render` writes as `##` headings on its own account, so no
/// project-supplied heading may take one.
///
/// `Feature catalog` is reserved even under `split_by_bucket`, where it
/// only surfaces if the project holds no features: a tree between its
/// `init` and its first feature file is exactly when a config is being
/// written, and reserving the name unconditionally is cheaper than a rule
/// that switches on how full the tree happens to be.
fn structural_heading(name: &str) -> Option<&'static str> {
    match name {
        crate::DETAILS_HEADING => Some("the per-feature details heading"),
        crate::FLAT_CATALOG_HEADING => Some("the catalog's own heading for a feature-less tree"),
        _ => None,
    }
}

/// One-time config sanity: reject a `[fields.*]` name the generator doesn't
/// model (a typo silently disables that field's validation), and require a
/// `[fields.X]` section for every *omissible* axis the tree actually uses.
///
/// "Actually uses" is [`axis_in_use`], the same predicate that decides
/// whether `render` emits a column for the axis, so the validator and the
/// renderer cannot disagree about which axes this project holds. Requiring
/// a declaration for an axis no feature carries would force a project to
/// declare values nothing uses and nothing renders — the second home for a
/// value that [ADR-0002](../docs/adr/0002-partial-schema-adoption.md)
/// exists to remove (#34).
///
/// The scan runs over [`Frontmatter::OMISSIBLE_FIELD_NAMES`], not all of
/// `FIELD_NAMES`. `type` and `area` are structurally mandatory, so "in use"
/// is always true for them and the rule would degenerate into "every
/// project must enumerate a taxonomy for `type` and `area`" — a policy
/// this tool does not get to impose, and one that would break every config
/// that omits them today. Declaring either is still supported and still
/// enforced per feature.
///
/// When the axis *is* used the omission stays a hard error: without the
/// declaration nothing checks the values, and for `horizon` the declared
/// order is also the sort rank, so within-tier ordering degrades to id
/// order for every feature carrying one.
fn check_config_fields(
    config_path: &Path,
    config: &Config,
    features: &[Feature],
    report: &mut ValidationReport,
) {
    // A `[fields.X]` naming something outside `FIELD_NAMES` used to be a
    // typo; since #22 it is a project-declared field, so the typo check
    // reverses direction — `check_declared_fields` catches the key nobody
    // declared, which is where the mistake is actually made. What stays
    // checkable here is the declaration's own coherence.
    for (name, spec) in &config.fields {
        let mut err = |message: String| {
            report.schema_errors.push(SchemaError {
                path: config_path.to_path_buf(),
                message,
            });
        };
        if Frontmatter::RESERVED_FIELD_NAMES.contains(&name.as_str()) {
            // Not a project field and not a declarable axis: these parse
            // into named struct fields, so they never reach `extra` and
            // never answer `field_values`. Declaring one constrains
            // nothing while making every feature look like it were
            // missing the field.
            err(format!(
                "`[fields.{name}]` is not declarable — `{name}` is part of the core \
                 schema, not a taxonomy axis (declarable: {})",
                Frontmatter::FIELD_NAMES.join(", ")
            ));
            continue;
        }
        if spec.column.is_some() && Frontmatter::FIELD_NAMES.contains(&name.as_str()) {
            err(format!(
                "`[fields.{name}]` sets `column`, but `{name}` is a built-in axis and \
                 already has one — the declaration would emit the same value twice"
            ));
        }
        if let Some(required_when) = &spec.required_when {
            for (cond_field, cond) in required_when {
                if matches!(cond, Condition::Any(values) if values.is_empty()) {
                    err(format!(
                        "`[fields.{name}]` has an empty `required_when.{cond_field}` list, \
                         which no value can match — the field would never be required"
                    ));
                }
            }
        }
        if !spec.values.is_empty() && spec.kind.is_some() {
            err(format!(
                "`[fields.{name}]` sets both `values` and `kind` — a closed value set \
                 and a shape check are alternatives, not layers"
            ));
        }
        if spec.values.is_empty() && spec.kind.is_none() {
            err(format!(
                "`[fields.{name}]` declares neither `values` nor `kind`, so nothing \
                 about the field is checked"
            ));
        }
        if spec.link.is_some() && spec.column.is_none() {
            err(format!(
                "`[fields.{name}]` sets `link` without `column` — the link would \
                 never be rendered"
            ));
        }
    }
    for name in Frontmatter::OMISSIBLE_FIELD_NAMES {
        if config.fields.contains_key(*name) || !axis_in_use(features, name) {
            continue;
        }
        let mut message = format!(
            "missing `[fields.{name}]` — some feature carries `{name}`, so the config \
             must declare its allowed values; nothing checks the field otherwise"
        );
        if *name == "horizon" {
            message.push_str(
                " (and for `horizon` the declared value order is the sort rank, so \
                 without it rows fall back to id order)",
            );
        }
        report.schema_errors.push(SchemaError {
            path: config_path.to_path_buf(),
            message,
        });
    }
}

/// Extract the contents of every `<a id="…">` in markdown.
/// Manual scanner — the shape is fixed and narrow, doesn't justify a regex dep.
pub fn extract_anchors(md: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let needle = "<a id=\"";
    let mut rest = md;
    while let Some(start) = rest.find(needle) {
        let after = &rest[start + needle.len()..];
        match after.find('"') {
            Some(end) => {
                out.insert(after[..end].to_string());
                rest = &after[end + 1..];
            },
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_anchors_basic() {
        let md = r#"<a id="f22"></a> ... <a id="f-foo"></a> ..."#;
        let got = extract_anchors(md);
        let want: BTreeSet<String> = ["f22", "f-foo"].iter().map(|s| s.to_string()).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn extract_anchors_ignores_other_html() {
        let md = r##"<div id="x"></div> <a href="#y">z</a> <a id="ok"></a>"##;
        let got = extract_anchors(md);
        let want: BTreeSet<String> = ["ok"].iter().map(|s| s.to_string()).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn extract_anchors_unterminated_is_safe() {
        let md = r#"<a id="oops"#;
        assert!(extract_anchors(md).is_empty());
    }

    #[test]
    fn report_clean_when_empty() {
        let r = ValidationReport::default();
        assert!(r.is_clean());
        assert!(!r.has_drift());
        assert!(!r.has_hard_errors());
    }

    #[test]
    fn validate_skips_when_source_missing() {
        // Pointing `root` at any non-existent `features/` parent should
        // silent-pass — the recipe runs on source-less checkouts too.
        let tmp = std::env::temp_dir().join("roadmark-skip-missing");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let roadmap_md = tmp.join("ROADMAP.md");
        std::fs::write(&roadmap_md, "").unwrap();
        let r = validate(&tmp, &roadmap_md, false).unwrap();
        assert!(r.source_missing);
        assert!(r.is_clean());
        assert!(r.to_text().contains("skipped"));
    }

    /// Same tree, same absence — but the user named the root, so silence
    /// would be a clean pass for a run that checked nothing (#31).
    #[test]
    fn validate_errors_when_explicit_root_has_no_features() {
        let tmp = std::env::temp_dir().join("roadmark-explicit-missing");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let roadmap_md = tmp.join("ROADMAP.md");
        std::fs::write(&roadmap_md, "").unwrap();
        let r = validate(&tmp, &roadmap_md, true).unwrap();
        assert!(!r.source_missing);
        assert!(!r.is_clean());
        assert!(r.has_hard_errors());
        let text = r.to_text();
        assert!(text.contains(&tmp.display().to_string()), "got: {text}");
        assert!(!text.contains("skipped"), "got: {text}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn report_drift_only() {
        let mut r = ValidationReport::default();
        r.anchors_missing_from_disk.push("f-new".into());
        assert!(!r.is_clean());
        assert!(r.has_drift());
        assert!(!r.has_hard_errors());
    }

    fn fm(item_type: &str, class: Option<&str>, area: Vec<&str>, horizon: &str) -> Frontmatter {
        Frontmatter {
            id: "F-x".into(),
            item_type: item_type.into(),
            class: class.map(Into::into),
            effort: None,
            area: area.into_iter().map(Into::into).collect(),
            horizon: Some(horizon.into()),
            status: crate::Status::Todo,
            target: vec!["v0.2.x".into()],
            severity: None,
            shipped: crate::Shipped::default(),
            shipped_order: None,
            extra: BTreeMap::new(),
        }
    }

    /// Standard version/title boilerplate around a `fields` map — the single
    /// place tests build a `Config`, so struct changes cost one edit.
    fn cfg(fields: std::collections::BTreeMap<String, crate::FieldSpec>) -> Config {
        Config {
            versions: vec!["v0.2.x".into()],
            title: "T".into(),
            fields,
            ..Config::default()
        }
    }

    fn cfg_with_fields() -> Config {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "class".to_string(),
            FieldSpec {
                values: vec!["differentiator".into(), "enabler".into()],
                multi: false,
                required_when: Some(BTreeMap::from([(
                    "type".to_string(),
                    Condition::One("feature".to_string()),
                )])),
                ..FieldSpec::default()
            },
        );
        fields.insert(
            "area".to_string(),
            FieldSpec {
                values: vec!["rules".into(), "docs".into()],
                multi: true,
                required_when: None,
                ..FieldSpec::default()
            },
        );
        cfg(fields)
    }

    #[test]
    fn field_check_flags_unknown_value() {
        let mut r = ValidationReport::default();
        let feature = fm("feature", Some("enabler"), vec!["nope"], "next");
        check_feature_fields(Path::new("f.md"), &feature, &cfg_with_fields(), &mut r);
        assert!(r
            .schema_errors
            .iter()
            .any(|e| e.message.contains("unknown `area` value \"nope\"")));
    }

    #[test]
    fn field_check_requires_class_for_features() {
        let mut r = ValidationReport::default();
        let feature = fm("feature", None, vec!["rules"], "next");
        check_feature_fields(Path::new("f.md"), &feature, &cfg_with_fields(), &mut r);
        assert!(r.schema_errors.iter().any(|e| e
            .message
            .contains("`class` is required when type = \"feature\"")));
    }

    #[test]
    fn field_check_allows_missing_class_for_non_features() {
        let mut r = ValidationReport::default();
        let feature = fm("chore", None, vec!["rules"], "next");
        check_feature_fields(Path::new("f.md"), &feature, &cfg_with_fields(), &mut r);
        assert!(
            r.schema_errors.is_empty(),
            "chore without class must pass: {:?}",
            r.schema_errors
        );
    }

    #[test]
    fn field_check_flags_empty_area() {
        let mut r = ValidationReport::default();
        let feature = fm("feature", Some("enabler"), vec![], "next");
        check_feature_fields(Path::new("f.md"), &feature, &cfg_with_fields(), &mut r);
        assert!(r
            .schema_errors
            .iter()
            .any(|e| e.message.contains("`area` must list at least one value")));
    }

    /// Regression for the "`required_when` only honours `type`" bug: a
    /// condition keyed on a non-`type` field must still be evaluated.
    #[test]
    fn field_check_required_when_honours_non_type_key() {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "effort".to_string(),
            FieldSpec {
                values: vec!["S".into(), "M".into(), "L".into()],
                multi: false,
                required_when: Some(BTreeMap::from([(
                    "horizon".to_string(),
                    Condition::One("now".to_string()),
                )])),
                ..FieldSpec::default()
            },
        );
        let config = cfg(fields);
        // effort is unset and horizon == "now" → the rule must fire.
        let feature = fm("feature", None, vec!["rules"], "now");
        let mut r = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &feature, &config, &mut r);
        assert!(
            r.schema_errors.iter().any(|e| e
                .message
                .contains("`effort` is required when horizon = \"now\"")),
            "got: {:?}",
            r.schema_errors
        );

        // horizon != "now" → the rule must NOT fire.
        let other = fm("feature", None, vec!["rules"], "next");
        let mut r2 = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &other, &config, &mut r2);
        assert!(r2.schema_errors.is_empty(), "got: {:?}", r2.schema_errors);
    }

    fn cfg_with_horizon(required_when: Option<BTreeMap<String, Condition>>) -> Config {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "horizon".to_string(),
            FieldSpec {
                values: vec!["now".into(), "next".into(), "later".into()],
                multi: false,
                required_when,
                ..FieldSpec::default()
            },
        );
        cfg(fields)
    }

    /// `horizon` is optional: its absence alone is not a schema error.
    #[test]
    fn field_check_allows_missing_horizon() {
        let mut feature = fm("feature", None, vec!["rules"], "now");
        feature.horizon = None;
        let mut r = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &feature, &cfg_with_horizon(None), &mut r);
        assert!(
            r.schema_errors.is_empty(),
            "missing horizon must pass: {:?}",
            r.schema_errors
        );
    }

    /// When present, `horizon` must still belong to the declared set.
    #[test]
    fn field_check_flags_unknown_horizon_value() {
        let feature = fm("feature", None, vec!["rules"], "someday");
        let mut r = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &feature, &cfg_with_horizon(None), &mut r);
        assert!(
            r.schema_errors
                .iter()
                .any(|e| e.message.contains("unknown `horizon` value \"someday\"")),
            "got: {:?}",
            r.schema_errors
        );
    }

    /// A config that declares `horizon` required (via `required_when`)
    /// keeps its old behavior: absence is an error when the condition holds.
    #[test]
    fn field_check_required_when_still_applies_to_horizon() {
        let config = cfg_with_horizon(Some(BTreeMap::from([(
            "type".to_string(),
            Condition::One("feature".to_string()),
        )])));
        let mut feature = fm("feature", None, vec!["rules"], "now");
        feature.horizon = None;
        let mut r = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &feature, &config, &mut r);
        assert!(
            r.schema_errors.iter().any(|e| e
                .message
                .contains("`horizon` is required when type = \"feature\"")),
            "got: {:?}",
            r.schema_errors
        );

        // The condition does not hold → absence stays fine.
        let mut chore = fm("chore", None, vec!["rules"], "now");
        chore.horizon = None;
        let mut r2 = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &chore, &config, &mut r2);
        assert!(r2.schema_errors.is_empty(), "got: {:?}", r2.schema_errors);
    }

    /// `required_when = {}` is the unconditional form (an empty AND is
    /// vacuously true): absence is always an error, and the message must
    /// not dangle a trailing "when".
    #[test]
    fn field_check_empty_required_when_means_always() {
        let config = cfg_with_horizon(Some(BTreeMap::new()));
        let mut chore = fm("chore", None, vec!["rules"], "now");
        chore.horizon = None;
        let mut r = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &chore, &config, &mut r);
        assert!(
            r.schema_errors
                .iter()
                .any(|e| e.message.contains("`horizon` is required")
                    && !e.message.contains("required when")),
            "got: {:?}",
            r.schema_errors
        );
    }

    /// Regression for the dead `multi` knob: `multi = false` must reject a
    /// field carrying more than one value.
    #[test]
    fn field_check_enforces_multi_false() {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "area".to_string(),
            FieldSpec {
                values: vec!["rules".into(), "docs".into()],
                multi: false,
                required_when: None,
                ..FieldSpec::default()
            },
        );
        let config = cfg(fields);
        let feature = fm("feature", None, vec!["rules", "docs"], "next");
        let mut r = ValidationReport::default();
        check_feature_fields(Path::new("f.md"), &feature, &config, &mut r);
        assert!(
            r.schema_errors.iter().any(|e| e
                .message
                .contains("`area` accepts a single value but 2 were given")),
            "got: {:?}",
            r.schema_errors
        );
    }

    /// Since #22 a `[fields.X]` outside the built-in set is a *project
    /// declaration*, not a typo — so the old "unknown field name" check is
    /// gone, and what stays checkable is the declaration's own coherence.
    #[test]
    fn config_check_flags_an_incoherent_declaration() {
        use crate::{FieldKind, FieldSpec};
        let mut fields = std::collections::BTreeMap::new();
        // Declared name outside FIELD_NAMES: legal now.
        fields.insert(
            "tracked".to_string(),
            FieldSpec {
                kind: Some(FieldKind::IssueRef),
                column: Some("Tracked".into()),
                ..FieldSpec::default()
            },
        );
        // Both a closed value set and a shape check: alternatives, not layers.
        fields.insert(
            "owner".to_string(),
            FieldSpec {
                values: vec!["a".into()],
                kind: Some(FieldKind::String),
                ..FieldSpec::default()
            },
        );
        // Neither: nothing about the field would be checked.
        fields.insert("mystery".to_string(), FieldSpec::default());
        // A link with no column would never be rendered.
        fields.insert(
            "spec".to_string(),
            FieldSpec {
                kind: Some(FieldKind::Url),
                link: Some("https://x/{}".into()),
                ..FieldSpec::default()
            },
        );
        // A core-schema key: neither a declarable axis nor a project field.
        fields.insert("status".to_string(), FieldSpec::default());
        // A `column` on a built-in axis, which already has one.
        fields.insert(
            "horizon".to_string(),
            FieldSpec {
                values: vec!["now".into()],
                column: Some("When".into()),
                ..FieldSpec::default()
            },
        );
        // An empty condition list can never match, so the field would
        // never be required — the same "checks nothing" class as above.
        fields.insert(
            "reviewer".to_string(),
            FieldSpec {
                kind: Some(FieldKind::String),
                required_when: Some(std::collections::BTreeMap::from([(
                    "type".to_string(),
                    Condition::Any(vec![]),
                )])),
                ..FieldSpec::default()
            },
        );
        let config = cfg(fields);
        let mut r = ValidationReport::default();
        check_config_fields(Path::new("config.toml"), &config, &[], &mut r);
        let msgs: Vec<&str> = r.schema_errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(msgs.len(), 6, "got: {msgs:?}");
        assert!(
            msgs.iter()
                .any(|m| m.contains("`[fields.status]` is not declarable")),
            "got: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("`horizon` is a built-in axis")),
            "got: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("empty `required_when.type` list")),
            "got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("`values` and `kind`")),
            "got: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("neither `values` nor `kind`")),
            "got: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("`link` without `column`")),
            "got: {msgs:?}"
        );
    }

    fn versions_errors(config: &Config) -> Vec<String> {
        let mut r = ValidationReport::default();
        check_versions(Path::new("config.toml"), config, &mut r);
        r.schema_errors.into_iter().map(|e| e.message).collect()
    }

    /// The rank/section disagreement of #47 is fixed in `index_of`, but
    /// the config is still describing an order it can't have — and in flat
    /// mode, where no heading is emitted, this is the *only* thing that
    /// reports it.
    #[test]
    fn versions_check_rejects_a_repeated_entry() {
        let config = Config {
            versions: vec!["v1".into(), "v2".into(), "v1".into()],
            ..Config::default()
        };
        let msgs = versions_errors(&config);
        assert_eq!(msgs.len(), 1, "got: {msgs:?}");
        assert!(msgs[0].contains("`versions` repeats `v1`"), "got: {msgs:?}");
    }

    /// One heading, one collision — however many times it was declared.
    #[test]
    fn versions_check_reports_a_repeated_reserved_name_once() {
        let config = Config {
            versions: vec!["Details".into(), "Details".into()],
            split_by_bucket: true,
            ..Config::default()
        };
        let msgs = versions_errors(&config);
        assert_eq!(msgs.len(), 2, "got: {msgs:?}");
        assert_eq!(
            msgs.iter().filter(|m| m.contains("collides")).count(),
            1,
            "got: {msgs:?}"
        );
    }

    /// The three names `render` writes itself, plus the tail label under
    /// its declared spelling. Each would emit a second `## {name}`.
    #[test]
    fn versions_check_rejects_a_bucket_colliding_with_a_structural_heading() {
        for (versions, unbucketed_label, want) in [
            (vec!["Details"], None, "the per-feature details heading"),
            (
                vec!["Feature catalog"],
                None,
                "the catalog's own heading for a feature-less tree",
            ),
            (
                vec!["Unscheduled"],
                None,
                "the default `unbucketed_label` (`Unscheduled`)",
            ),
            (vec!["Later"], Some("Later"), "`unbucketed_label`"),
        ] {
            let config = Config {
                versions: versions.iter().map(|v| v.to_string()).collect(),
                unbucketed_label: unbucketed_label.map(str::to_string),
                split_by_bucket: true,
                ..Config::default()
            };
            let msgs = versions_errors(&config);
            assert_eq!(msgs.len(), 1, "{versions:?} got: {msgs:?}");
            assert!(msgs[0].contains(want), "{versions:?} got: {msgs:?}");
            assert!(
                msgs[0].contains(&format!("two `## {}` headings", versions[0])),
                "{versions:?} got: {msgs:?}"
            );
        }
    }

    /// The tail heading is project-supplied too, and needs no `versions`
    /// entry to collide: `unbucketed_label = "Details"` alone puts a second
    /// `## Details` in the document, from the group `catalog_groups`
    /// always appends.
    #[test]
    fn versions_check_rejects_an_unbucketed_label_taking_a_structural_heading() {
        for (label, want) in [
            ("Details", "the per-feature details heading"),
            (
                "Feature catalog",
                "the catalog's own heading for a feature-less tree",
            ),
        ] {
            let config = Config {
                versions: vec!["v1".into()],
                unbucketed_label: Some(label.to_string()),
                split_by_bucket: true,
                ..Config::default()
            };
            let msgs = versions_errors(&config);
            assert_eq!(msgs.len(), 1, "{label} got: {msgs:?}");
            assert!(
                msgs[0].contains(&format!("`unbucketed_label` resolves to `{label}`")),
                "{label} got: {msgs:?}"
            );
            assert!(msgs[0].contains(want), "{label} got: {msgs:?}");
        }
    }

    /// Flat mode emits no bucket heading at all, so there is nothing for a
    /// reserved name to collide with — reporting one would be an error
    /// about a document the tool isn't producing.
    #[test]
    fn versions_check_ignores_heading_collisions_in_flat_mode() {
        let config = Config {
            versions: vec!["Details".into(), "Unscheduled".into()],
            unbucketed_label: Some("Feature catalog".into()),
            ..Config::default()
        };
        assert!(versions_errors(&config).is_empty());
    }

    /// Wrap the `fm` helper's frontmatter into a feature, for the checks
    /// that ask what the *tree* holds rather than what one file says.
    fn feature_of(frontmatter: Frontmatter) -> Feature {
        Feature {
            frontmatter,
            body: "Summary line.\n".into(),
        }
    }

    /// A config declaring only `type` still owes `[fields.horizon]` once a
    /// feature carries a horizon — the value order is the sort rank.
    #[test]
    fn config_check_flags_missing_horizon_when_a_feature_carries_one() {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "type".to_string(),
            FieldSpec {
                values: vec!["feature".into()],
                multi: false,
                required_when: None,
                ..FieldSpec::default()
            },
        );
        let config = cfg(fields);
        let features = vec![feature_of(fm("feature", None, vec!["rules"], "now"))];
        let mut r = ValidationReport::default();
        check_config_fields(Path::new("config.toml"), &config, &features, &mut r);
        assert!(
            r.schema_errors
                .iter()
                .any(|e| e.message.contains("missing `[fields.horizon]`")),
            "got: {:?}",
            r.schema_errors
        );
    }

    /// #34: a board-canonical project holds no `horizon` at all. Requiring
    /// the section would force it to declare values nothing uses and
    /// nothing renders — the second home ADR-0002 exists to remove.
    #[test]
    fn config_check_allows_missing_horizon_when_no_feature_carries_one() {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        for name in ["type", "area"] {
            fields.insert(
                name.to_string(),
                FieldSpec {
                    values: vec!["feature".into(), "rules".into()],
                    multi: true,
                    required_when: None,
                    ..FieldSpec::default()
                },
            );
        }
        let config = cfg(fields);
        let mut without = fm("feature", None, vec!["rules"], "now");
        without.horizon = None;
        let features = vec![feature_of(without)];
        let mut r = ValidationReport::default();
        check_config_fields(Path::new("config.toml"), &config, &features, &mut r);
        assert!(
            r.schema_errors.is_empty(),
            "an unused axis needs no declaration: {:?}",
            r.schema_errors
        );
    }

    /// The rule is per-axis, not a `horizon` special case: the same tree
    /// owes `[fields.effort]` as soon as one feature carries an effort.
    #[test]
    fn config_check_generalises_to_other_axes() {
        use crate::FieldSpec;
        let mut fields = std::collections::BTreeMap::new();
        for name in ["type", "area", "horizon"] {
            fields.insert(
                name.to_string(),
                FieldSpec {
                    values: vec!["feature".into(), "rules".into(), "now".into()],
                    multi: true,
                    required_when: None,
                    ..FieldSpec::default()
                },
            );
        }
        let config = cfg(fields);
        let mut with_effort = fm("feature", None, vec!["rules"], "now");
        with_effort.effort = Some("M".into());
        let features = vec![feature_of(with_effort)];
        let mut r = ValidationReport::default();
        check_config_fields(Path::new("config.toml"), &config, &features, &mut r);
        assert!(
            r.schema_errors
                .iter()
                .any(|e| e.message.contains("missing `[fields.effort]`")),
            "got: {:?}",
            r.schema_errors
        );
    }

    #[test]
    fn empty_body_is_a_warning_not_an_error() {
        for body in ["", "   \n\t\n"] {
            let mut r = ValidationReport::default();
            check_body(Path::new("f-x.md"), body, &mut r);
            assert_eq!(r.warnings.len(), 1, "body {body:?} → {:?}", r.warnings);
            assert!(r.warnings[0].message.contains("empty body"));
            assert!(!r.has_hard_errors());
            assert!(!r.is_clean(), "a warning is something to say");
        }
    }

    #[test]
    fn one_line_body_is_clean() {
        let mut r = ValidationReport::default();
        check_body(Path::new("f-x.md"), "A summary line.\n", &mut r);
        assert!(r.warnings.is_empty(), "got: {:?}", r.warnings);
        assert!(r.is_clean());
    }

    /// A warnings-only report exits 0 (`has_hard_errors` is false) but must
    /// not claim to be clean — the summary line says so in words.
    #[test]
    fn warnings_only_report_says_so_without_saying_clean() {
        let mut r = ValidationReport::default();
        r.warnings.push(Warning {
            path: PathBuf::from("f-x.md"),
            message: "empty body".into(),
        });
        assert!(!r.has_hard_errors());
        assert!(!r.has_drift());
        assert!(!r.is_clean());
        let text = r.to_text();
        assert!(!text.contains("validate: clean"), "got: {text}");
        assert!(text.contains("warnings (1):"), "got: {text}");
        assert!(text.contains("no errors, 1 warning(s)"), "got: {text}");
    }

    #[test]
    fn link_targets_are_taken_verbatim() {
        assert_eq!(
            link_targets("see [a](#f-one) and [b](#some_section) and [c](http://x)"),
            vec!["f-one", "some_section"]
        );
        // An unterminated link is not a target — and must not loop.
        assert!(link_targets("[a](#f-one").is_empty());
    }

    #[test]
    fn token_runs_split_on_token_boundaries() {
        assert_eq!(
            token_runs("CRLF-authored files, `F-foo`."),
            vec!["CRLF-authored", "files", "F-foo"]
        );
    }

    #[test]
    fn feature_id_shape_gates_what_counts_as_a_reference() {
        assert!(looks_like_feature_id("F-foo"));
        assert!(looks_like_feature_id("f-foo-bar"));
        assert!(looks_like_feature_id("F139"));
        // Section anchors and ordinary prose are not feature references.
        assert!(!looks_like_feature_id("installation"));
        assert!(!looks_like_feature_id("some_section"));
        assert!(!looks_like_feature_id("files"));
    }

    /// `[F-foo](#f-foo)` is one reference in two spellings, folded through
    /// `anchor_id` and reported at the stronger (link) severity.
    #[test]
    fn scan_folds_link_and_prose_forms_into_one_reference() {
        let refs = scan_feature_refs("See [F-foo](#f-foo), and F-foo again.");
        assert_eq!(refs.len(), 1, "got: {refs:?}");
        assert_eq!(refs["f-foo"].0, RefForm::Link);
    }

    #[test]
    fn scan_ignores_ids_nested_in_longer_tokens() {
        let refs = scan_feature_refs("F-foobar and CRLF-authored are not F-foo.");
        let keys: Vec<&str> = refs.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["f-foo", "f-foobar"]);
    }

    /// Prose that *quotes* the link syntax is documentation, not a link.
    /// Regression: the first roadmap entry written about this check failed
    /// its own rule, because it spelled out `](#f-foo)` in a code span.
    #[test]
    fn scan_does_not_read_a_link_inside_a_code_span() {
        let refs = scan_feature_refs("A link `](#f-foo)` is a hard error.");
        // Still seen — as a bare mention, which is only a warning.
        assert_eq!(refs["f-foo"].0, RefForm::Bare, "got: {refs:?}");
    }

    /// A fenced block is just a long code span, and the fence must not
    /// leak: text after it is scanned normally.
    #[test]
    fn scan_masks_fenced_blocks_and_resumes_after_them() {
        let body = "```\n[x](#f-inside)\n```\nThen [y](#f-outside).";
        let refs = scan_feature_refs(body);
        assert_eq!(refs["f-inside"].0, RefForm::Bare, "got: {refs:?}");
        assert_eq!(refs["f-outside"].0, RefForm::Link, "got: {refs:?}");
    }

    /// An unterminated backtick is an ordinary character — it must not
    /// swallow the rest of the body and hide real links.
    #[test]
    fn scan_unterminated_backtick_does_not_mask_the_rest() {
        let refs = scan_feature_refs("a ` stray tick then [y](#f-real).");
        assert_eq!(refs["f-real"].0, RefForm::Link, "got: {refs:?}");
    }

    fn dangling(bodies: &[&str], declared: &[&str]) -> ValidationReport {
        let mut parsed: Vec<(PathBuf, Feature)> = Vec::new();
        for (i, id) in declared.iter().enumerate() {
            let mut frontmatter = fm("feature", None, vec!["rules"], "now");
            frontmatter.id = (*id).to_string();
            parsed.push((
                PathBuf::from(format!("f-{i}.md")),
                Feature {
                    frontmatter,
                    body: bodies.get(i).copied().unwrap_or("Body.").to_string(),
                },
            ));
        }
        let mut r = ValidationReport::default();
        check_dangling_refs(&parsed, &[], &mut r);
        r
    }

    #[test]
    fn dangling_link_is_a_hard_error() {
        let r = dangling(&["Successor of [F-gone](#f-gone)."], &["F-here"]);
        assert!(r.has_hard_errors());
        assert_eq!(r.dangling_links.len(), 1, "got: {:?}", r.dangling_links);
        assert_eq!(r.dangling_links[0].reference, "f-gone");
        assert!(
            r.warnings.is_empty(),
            "not reported twice: {:?}",
            r.warnings
        );
    }

    #[test]
    fn dangling_bare_token_is_only_a_warning() {
        let r = dangling(&["Sibling to `F-gone`, roughly."], &["F-here"]);
        assert!(!r.has_hard_errors());
        assert_eq!(r.warnings.len(), 1, "got: {:?}", r.warnings);
        assert!(r.warnings[0]
            .message
            .contains("reference to unknown feature id F-gone"));
    }

    #[test]
    fn live_references_are_clean() {
        let r = dangling(
            &["Builds on [F-there](#f-there) and F-there.", "Body."],
            &["F-here", "F-there"],
        );
        assert!(r.is_clean(), "got: {}", r.to_text());
    }

    /// Ids are matched through `anchor_id`, so case never invents a
    /// dangling reference (nor hides one).
    #[test]
    fn reference_matching_is_case_insensitive() {
        let r = dangling(&["See [f-there](#f-there) and F-THERE."], &["F-There"]);
        assert!(r.is_clean(), "got: {}", r.to_text());
    }

    /// Whole-token matching: a reference to `F-foo` must not be satisfied
    /// by — nor confused with — a declared `F-foobar`.
    #[test]
    fn reference_does_not_match_inside_a_longer_id() {
        let r = dangling(
            &["Unlike [F-foo](#f-foo), F-foobar shipped."],
            &["F-foobar"],
        );
        assert_eq!(r.dangling_links.len(), 1, "got: {:?}", r.dangling_links);
        assert_eq!(r.dangling_links[0].reference, "f-foo");
    }

    /// A link to a document section is not a feature reference: only
    /// feature-id-shaped targets are checked.
    #[test]
    fn link_to_non_feature_anchor_is_ignored() {
        let r = dangling(
            &["See [the schema](#schema-v2) and [why](#rationale)."],
            &["F-here"],
        );
        assert!(r.is_clean(), "got: {}", r.to_text());
    }
}
