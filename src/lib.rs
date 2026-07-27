//! Pure functions for the `roadmap` generator.
//!
//! Wired by `main.rs`. Kept fs-free so unit tests can pass strings
//! and snapshot the rendered output via `insta`.

pub mod add;
pub mod import;
pub mod rename;
pub mod validate;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One feature: TOML frontmatter + raw markdown body.
///
/// Body stays an unparsed `String` — a markdown parser would round-trip
/// poorly (loses author intent on edge cases), and the renderer only
/// needs the first paragraph for the catalog summary.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub frontmatter: Frontmatter,
    pub body: String,
}

/// Schema v2 frontmatter. Two orthogonal axes replace the old flat
/// `priority`: `class` (kind of leverage) and `effort`. The taxonomy
/// (`area`) is multi-valued. Allowed values for `type`/`class`/`effort`/
/// `area`/`horizon`/`severity` are **not** hardcoded here — they are
/// declared per-project in `config.toml` `[fields.*]` and enforced by
/// `validate`, so this generator stays reusable across projects.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Frontmatter {
    pub id: String,
    /// `feature | fix | chore`. Only features carry a `class`; only
    /// fixes carry a `severity`. `type` is a Rust keyword → renamed.
    #[serde(rename = "type")]
    pub item_type: String,
    /// Kind of leverage (feature-only): differentiator/enabler/… .
    #[serde(default)]
    pub class: Option<String>,
    /// S / M / L. Optional during migration (backfilled by triage).
    #[serde(default)]
    pub effort: Option<String>,
    /// Multi-valued taxonomy (renamed from the old single `topic`).
    pub area: Vec<String>,
    /// Ordering horizon (renamed from the old `priority`). Sort rank
    /// comes from the declared order of `[fields.horizon].values`.
    /// Optional — useful when priority lives on an external board; a
    /// feature without one sorts last within its bucket.
    #[serde(default)]
    pub horizon: Option<String>,
    pub status: Status,
    pub target: Vec<String>,
    /// Fix-only severity: critical/major/minor.
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub shipped: Shipped,
    /// Stable position within the "shipped" tier — set at flip-time
    /// (status: todo → done) so historical order survives regen.
    /// Optional; only required once the catalog includes shipped entries.
    #[serde(default)]
    pub shipped_order: Option<u32>,
    /// Every key this generator does not model, kept verbatim so a
    /// project can declare its own fields in `config.toml` (`tracked`,
    /// `owner`, …) without the binary knowing their names.
    ///
    /// This is why [`Frontmatter`] no longer carries
    /// `deny_unknown_fields`: serde cannot combine it with `flatten`.
    /// The guarantee it provided did not go away — it moved to
    /// [`check_declared_fields`], which rejects any key here that no
    /// `[fields.*]` declares. Enforcement is now one layer out, with a
    /// message that can say *which* declaration is missing.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Frontmatter {
    /// The taxonomy axes this generator models — the ones a `[fields.*]`
    /// section can constrain with `values` / `multi` / `required_when`.
    ///
    /// Since #22 a `[fields.*]` name *outside* this set is not a typo but
    /// a project-declared field. The typo guard moved to
    /// [`check_declared_fields`], which rejects a frontmatter key nobody
    /// declares — see [`Self::RESERVED_FIELD_NAMES`] for the names that
    /// are neither.
    pub const FIELD_NAMES: &'static [&'static str] =
        &["type", "class", "effort", "area", "horizon", "severity"];

    /// Modelled frontmatter keys that are **not** declarable axes.
    ///
    /// They are structural: `id` and `target` drive anchors and bucketing,
    /// `status` is the one hardcoded taxonomy
    /// ([ADR-0003](../docs/adr/0003-status-stays-hardcoded.md)), and
    /// `shipped`/`shipped_order` are shipping metadata. None of them
    /// answers [`Self::field_values`], and because they parse into named
    /// struct fields none reaches [`Frontmatter::extra`] either — so a
    /// `[fields.status]` would constrain nothing while making every
    /// feature look like it were missing a `status`. Declaring one is a
    /// config error, not a project field.
    pub const RESERVED_FIELD_NAMES: &'static [&'static str] =
        &["id", "status", "target", "shipped", "shipped_order"];

    /// The axes a feature may leave out **entirely** — the ones ADR-0002 is
    /// about. `type` and `area` are absent from this list because they are
    /// structurally mandatory: every frontmatter carries them, so "is this
    /// axis in use?" is always yes for those two and could never follow the
    /// data.
    ///
    /// This is what `validate` requires a `[fields.*]` declaration for when
    /// the tree uses the axis. Scoping it here rather than to all of
    /// [`Self::FIELD_NAMES`] is deliberate: requiring a declared value set
    /// for `type`/`area` would force *every* project to enumerate a
    /// taxonomy for them, which is a policy this tool does not get to
    /// impose. Declaring them is still supported and still enforced.
    pub const OMISSIBLE_FIELD_NAMES: &'static [&'static str] =
        &["class", "effort", "horizon", "severity"];

    /// Values a named schema field currently holds, for config-driven
    /// validation. `None` = neither a modelled field nor one this feature
    /// carries in [`Self::extra`], so there is nothing to check.
    /// `Some(vec)` = the present values (empty when a modelled optional
    /// field is unset), so the caller can enforce `required_when` and
    /// membership.
    ///
    /// Project-declared fields answer here too, which is what lets
    /// `render` and `validate` treat them exactly like the built-in axes
    /// rather than growing a second code path each.
    pub fn field_values(&self, name: &str) -> Option<Vec<String>> {
        let one = |s: &str| vec![s.to_string()];
        let opt = |o: &Option<String>| o.iter().cloned().collect::<Vec<_>>();
        match name {
            "type" => Some(one(&self.item_type)),
            "class" => Some(opt(&self.class)),
            "effort" => Some(opt(&self.effort)),
            "area" => Some(self.area.clone()),
            "horizon" => Some(opt(&self.horizon)),
            "severity" => Some(opt(&self.severity)),
            // A project-declared field the feature omits is simply absent
            // from `extra`, and this function has no `Config` to tell
            // "declared but unset" from "not a field at all" — so both
            // answer `None`. Callers iterating `config.fields` already
            // know the name is declared and read this as an empty value
            // list (`unwrap_or_default`), which is what lets
            // `required_when` fire on an omitted field. Don't read
            // `is_some()` as "declared"; it means "carried".
            _ => self.extra.get(name).map(toml_values),
        }
    }

    /// Every key this feature carries that the generator does not model —
    /// the candidates for a project declaration, and for a typo.
    pub fn extra_names(&self) -> impl Iterator<Item = &str> {
        self.extra.keys().map(String::as_str)
    }
}

/// A declared field's values as strings, whatever TOML shape it took.
///
/// Numbers stringify rather than being rejected: `tracked = 42` and
/// `tracked = "42"` should mean the same thing to a roadmap, and the
/// [`FieldKind`] check is where the shape is actually enforced.
fn toml_values(v: &toml::Value) -> Vec<String> {
    match v {
        toml::Value::Array(items) => items.iter().flat_map(toml_values).collect(),
        toml::Value::String(s) => vec![s.clone()],
        other => vec![other.to_string()],
    }
}

/// The one taxonomy field still hardcoded rather than config-declared
/// (`[fields.*]`) — see [ADR-0003](../docs/adr/0003-status-stays-hardcoded.md)
/// for why: `rank()` orders catalog rows and `Shipped`/`shipped_order` are
/// keyed off `Done`, so an arbitrary declared value would still need a
/// distinguished done-ness predicate. Not worth the coupling for the one
/// extra value (`Blocked`) actually needed.
///
/// `#[non_exhaustive]`: because the set lives in the binary rather than in
/// `config.toml`, every new value is a source break for any downstream crate
/// matching on it. Paying that once — here, alongside `Blocked`, which breaks
/// those matches anyway — means the next value doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Wip,
    /// Scoped and wanted, but cannot start for a reason outside the
    /// project (e.g. blocked upstream). Distinct from `Todo`, which
    /// invites someone to pick the work up.
    Blocked,
    Todo,
    Done,
}

impl Status {
    const fn rank(self) -> u8 {
        match self {
            Self::Wip => 0,
            // Closer to in-flight than untouched work, and wants to be
            // seen — sorts between `Wip` and `Todo`.
            Self::Blocked => 1,
            Self::Todo => 2,
            Self::Done => 3,
        }
    }

    const fn glyph(self) -> &'static str {
        match self {
            Self::Wip => "🚧",
            Self::Blocked => "⛔",
            Self::Todo => "☐",
            Self::Done => "✅",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Shipped {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub pr: u32,
}

/// `.roadmap/config.toml` contents.
///
/// `deny_unknown_fields` for the reason [`Frontmatter`] *used* to carry it
/// — the guard there now lives in [`check_declared_fields`], because
/// project-declared fields need `serde(flatten)`. Here the attribute stays,
/// and with a sharper edge: every key here is optional, so a typo has no
/// shape to fail on and would read as "the user didn't want that". TOML
/// makes it worse — a top-level key written *below* a `[fields.x]` table
/// belongs to that table, so a misplaced `sections = [...]` silently
/// becomes `fields.x.sections` and the narrative it declares never
/// appears. Both spellings are now rejected at parse time.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Bucket order. Earliest cycle first.
    ///
    /// Two readers, which is why `validate` polices this list rather than
    /// letting either pick a behaviour (#47): it ranks features
    /// ([`sort_features`]), and under [`Config::split_by_bucket`] it also
    /// orders and *names* the catalog's `##` sections.
    pub versions: Vec<String>,
    /// H1 heading for the generated `ROADMAP.md`. Defaults to `"Roadmap"`.
    #[serde(default = "default_title")]
    pub title: String,
    /// Optional project-specific note appended to the generated
    /// "DO NOT EDIT" banner — e.g. a pointer to an ADR or design doc.
    #[serde(default)]
    pub source_note: Option<String>,
    /// Emit one catalog table per bucket, `##`-headed and ordered by
    /// [`Config::versions`], instead of a single flat table.
    ///
    /// Opt-in (default `false`) because it rewrites every line of the
    /// generated document: for a project with few features, or no
    /// meaningful bucket axis, the flat table is the right output.
    #[serde(default)]
    pub split_by_bucket: bool,
    /// Header for the bucket column, when one is emitted. Defaults to
    /// `"Target"`.
    ///
    /// `versions` is a bucket order, not necessarily a release axis — a
    /// project bucketing by MoSCoW wants `Priority`, not `Target`.
    #[serde(default)]
    pub bucket_label: Option<String>,
    /// Heading for the trailing section holding features whose first
    /// `target` is not a declared bucket (or absent). Defaults to
    /// `"Unscheduled"`. Only consulted when `split_by_bucket` is set.
    #[serde(default)]
    pub unbucketed_label: Option<String>,
    /// Hand-written markdown files injected verbatim into the generated
    /// document, each at a declared slot. Emitted in declaration order
    /// within a slot.
    ///
    /// The escape hatch for prose that belongs to no single feature —
    /// triage notes, "why this slice", horizon commentary. A generated
    /// file can still have hand-written parts as long as the boundary is
    /// explicit, which is what the slot is.
    #[serde(default)]
    pub sections: Vec<Section>,
    /// Per-field allowed-value declarations, keyed by field name
    /// (`type`, `class`, `effort`, `area`, `horizon`, `severity`).
    /// `BTreeMap` so validation errors emit in a stable order.
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSpec>,
}

/// One hand-written markdown file declared in `config.toml`, and where
/// it lands in the generated document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// Path relative to the `.roadmap/` root. Must stay inside it —
    /// an absolute path or a `..` component is a schema error, so a
    /// config can't reach out of the source tree it describes.
    pub file: String,
    pub slot: Slot,
}

/// Where a [`Section`] is injected.
///
/// Three slots, named for the document's structural landmarks rather
/// than for individual sections: under `split_by_bucket` the catalog is
/// several `##` sections, so `BeforeCatalog` means before the *first*
/// one and `AfterCatalog` after the *last* — the boundaries stay
/// well-defined however the catalog is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum Slot {
    /// After the title and banner, before the catalog.
    BeforeCatalog,
    /// After the catalog, before `## Details`.
    AfterCatalog,
    /// At the end of the document, after the last feature's details.
    AfterDetails,
}

/// A [`Section`] with its file read — what [`render`] consumes, so the
/// renderer stays string-in/string-out and the I/O lives in
/// [`load_sections`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSection {
    pub slot: Slot,
    /// The file's contents, injected verbatim: roadmark neither parses
    /// nor reformats it. Whatever the author wrote is what ships.
    pub body: String,
}

/// An empty project: no buckets, no field declarations, everything
/// off. Exists so callers building a `Config` in code — and every test
/// that only cares about two fields — can spell the rest
/// `..Config::default()` instead of tracking each new option.
impl Default for Config {
    fn default() -> Self {
        Self {
            versions: Vec::new(),
            title: default_title(),
            source_note: None,
            split_by_bucket: false,
            bucket_label: None,
            unbucketed_label: None,
            sections: Vec::new(),
            fields: BTreeMap::new(),
        }
    }
}

/// Column header used when the project declares no [`Config::bucket_label`].
const DEFAULT_BUCKET_LABEL: &str = "Target";

/// Heading for the tail section when the project declares no
/// [`Config::unbucketed_label`].
const DEFAULT_UNBUCKETED_LABEL: &str = "Unscheduled";

/// `##` heading of the single flat catalog table — also the fallback
/// heading under `split_by_bucket` when the project holds no features.
pub(crate) const FLAT_CATALOG_HEADING: &str = "Feature catalog";

/// `##` heading of the per-feature detail section.
pub(crate) const DETAILS_HEADING: &str = "Details";

impl Config {
    /// Heading of the trailing catalog group under `split_by_bucket`.
    ///
    /// One reader for the declared label and its default, so `render` and
    /// the `validate` collision check cannot disagree about which name the
    /// document will actually carry.
    pub(crate) fn unbucketed_heading(&self) -> &str {
        self.unbucketed_label
            .as_deref()
            .unwrap_or(DEFAULT_UNBUCKETED_LABEL)
    }
}

/// Declares the allowed values (and shape) of one schema field, so the
/// project — not this binary — owns its taxonomy.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    /// The closed set of accepted values. Empty when the field is
    /// checked by [`Self::kind`] instead — a tracking issue number has
    /// no enumerable value set.
    #[serde(default)]
    pub values: Vec<String>,
    /// Shape check for a field whose values can't be enumerated.
    /// Mutually exclusive with a non-empty [`Self::values`].
    #[serde(default)]
    pub kind: Option<FieldKind>,
    /// Whether the frontmatter field is an array (e.g. `area`).
    #[serde(default)]
    pub multi: bool,
    /// Conditional presence: `{ type = "feature" }` makes the field
    /// required only when `type` is `feature`; `{ horizon = ["now",
    /// "next"] }` when `horizon` is either. Multiple keys are ANDed.
    #[serde(default)]
    pub required_when: Option<BTreeMap<String, Condition>>,
    /// Emit this field as a catalog column under the given header.
    /// Absent means the field is validated but not tabulated.
    #[serde(default)]
    pub column: Option<String>,
    /// Turn each value into a link, substituting it for `{}`. E.g.
    /// `"https://github.com/owner/repo/issues/{}"`. Keeps the forge out
    /// of the binary: roadmark only knows the template.
    #[serde(default)]
    pub link: Option<String>,
}

/// What a declared field's values must *look* like, when they can't be
/// enumerated as a closed set.
///
/// Deliberately shallow — no `pattern`, because roadmark carries no regex
/// dependency (see AGENTS.md) and a half-regex would be worse than none.
/// These four cover the cases a roadmap actually needs; anything finer
/// belongs in the project's own CI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum FieldKind {
    /// Any integer.
    Integer,
    /// Any non-empty string.
    String,
    /// Must start with `http://` or `https://`.
    Url,
    /// A forge issue: a positive integer, with or without a leading `#`.
    /// Stays a dumb number as far as roadmark is concerned — only an
    /// adapter needs to know it means an issue.
    IssueRef,
}

/// One `required_when` condition: a single expected value, or any of a
/// list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Condition {
    One(String),
    Any(Vec<String>),
}

impl Condition {
    /// Does the referenced field currently hold a value this condition
    /// accepts? A list is an OR; a bare string is the one-element case.
    pub fn matches(&self, values: &[String]) -> bool {
        match self {
            Self::One(want) => values.iter().any(|v| v == want),
            Self::Any(wants) => values.iter().any(|v| wants.contains(v)),
        }
    }

    /// Human-readable form for error messages.
    pub fn describe(&self) -> String {
        match self {
            Self::One(want) => format!("{want:?}"),
            Self::Any(wants) => wants
                .iter()
                .map(|w| format!("{w:?}"))
                .collect::<Vec<_>>()
                .join(" or "),
        }
    }
}

fn default_title() -> String {
    "Roadmap".to_string()
}

/// Split a `+++`-fenced frontmatter doc into TOML head + markdown body.
///
/// Accepts trailing newline after closing fence. Body is returned
/// trimmed of one leading blank line if present.
pub fn split_frontmatter(src: &str) -> Result<(&str, &str)> {
    let rest = src
        .strip_prefix("+++\n")
        .ok_or_else(|| anyhow!("missing opening `+++` fence"))?;
    let end = rest
        .find("\n+++")
        .ok_or_else(|| anyhow!("missing closing `+++` fence"))?;
    let toml_block = &rest[..end];
    // Skip the closing fence + its trailing newline if present.
    let after = &rest[end + "\n+++".len()..];
    let body = after.strip_prefix('\n').unwrap_or(after);
    let body = body.strip_prefix('\n').unwrap_or(body);
    Ok((toml_block, body))
}

pub fn parse_feature(src: &str) -> Result<Feature> {
    // Normalize CRLF so Windows-authored (or autocrlf-checked-out) files
    // parse identically; the renderer emits LF-only output either way.
    let normalized: std::borrow::Cow<'_, str> = if src.contains('\r') {
        std::borrow::Cow::Owned(src.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(src)
    };
    let (toml_block, body) = split_frontmatter(&normalized)?;
    let frontmatter: Frontmatter =
        toml::from_str(toml_block).context("invalid TOML frontmatter")?;
    Ok(Feature {
        frontmatter,
        body: body.to_string(),
    })
}

/// Sort key: target[0] (via config bucket order) → status → horizon
/// (via config `[fields.horizon]` order) → shipped_order → id.
///
/// `shipped_order` (set at flip-time so historical order survives regen)
/// must sit *before* `id` in the key: `id` is unique, so any tiebreak
/// placed after it would never run. Features without a `shipped_order`
/// sort last within their tier (via `u32::MAX`), then break ties by `id`.
/// Unknown targets and unknown or absent horizons sort last; missing
/// target arrays are an upstream schema error caught at parse time.
fn sort_key<'a>(
    f: &'a Feature,
    version_index: &HashMap<&str, usize>,
    horizon_index: &HashMap<&str, usize>,
) -> (usize, u8, usize, u32, &'a str) {
    let target_idx = f
        .frontmatter
        .target
        .first()
        .and_then(|t| version_index.get(t.as_str()).copied())
        .unwrap_or(usize::MAX);
    let horizon_idx = f
        .frontmatter
        .horizon
        .as_deref()
        .and_then(|h| horizon_index.get(h).copied())
        .unwrap_or(usize::MAX);
    (
        target_idx,
        f.frontmatter.status.rank(),
        horizon_idx,
        f.frontmatter.shipped_order.unwrap_or(u32::MAX),
        &f.frontmatter.id,
    )
}

/// Build a value → declaration-order index for stable ranking.
///
/// **First declaration wins.** A repeated entry is a config mistake that
/// `validate` rejects (#47), but the renderer still has to pick a rank,
/// and it must be the same one [`catalog_groups`] picks when it
/// deduplicates the headings — otherwise a bucket's section is emitted at
/// its first position while its rows sort at the last, and the document
/// contradicts itself. `collect()` would give last-wins, which is also
/// reading a sort rank out of `HashMap` insertion semantics.
fn index_of(values: &[String]) -> HashMap<&str, usize> {
    let mut index = HashMap::with_capacity(values.len());
    for (i, v) in values.iter().enumerate() {
        index.entry(v.as_str()).or_insert(i);
    }
    index
}

pub fn sort_features(features: &mut [Feature], config: &Config) {
    let version_index = index_of(&config.versions);
    let horizon_index = config
        .fields
        .get("horizon")
        .map(|s| index_of(&s.values))
        .unwrap_or_default();
    features.sort_by(|a, b| {
        sort_key(a, &version_index, &horizon_index).cmp(&sort_key(
            b,
            &version_index,
            &horizon_index,
        ))
    });
}

/// Longest catalog Summary before it stops being scannable, in `char`s.
const SUMMARY_MAX_CHARS: usize = 120;

/// A short, scannable plain-text lead for the catalog Summary column.
///
/// Takes the first non-empty body **paragraph** — every line up to the next
/// blank one, joined with spaces — strips inline markdown (code-span
/// backticks, `*`/`_` emphasis markers, and `[text](url)` links folded to
/// `text`), collapses whitespace runs to single spaces, then truncates to
/// [`SUMMARY_MAX_CHARS`] on a word boundary — never mid-word, never mid-`char`.
/// A paragraph already within the budget is returned unchanged; a truncated
/// one gains a trailing `" …"`. The full body still lives in the Details
/// section.
///
/// The paragraph — not the line — is the unit because a house style that wraps
/// prose at 80 columns would otherwise silently truncate every summary it
/// wrapped, with `## Details` rendering the sentence whole a few screens down
/// (#55). Blank lines are the same boundary the body already uses, so a
/// one-line summary is unaffected.
fn summary(body: &str) -> String {
    let paragraph = first_paragraph(body);
    let cleaned = clean_inline_markdown(&paragraph);
    truncate_on_word_boundary(&cleaned, SUMMARY_MAX_CHARS)
}

/// The first non-blank run of lines, joined with single spaces.
///
/// Deliberately blind to markdown block structure: a body whose first
/// paragraph is a list or a heading joins into one line rather than
/// being parsed, which keeps this predictable and keeps the promise that
/// bodies stay unparsed strings.
fn first_paragraph(body: &str) -> String {
    body.lines()
        .skip_while(|l| l.trim().is_empty())
        .take_while(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fold inline markdown in a single line down to plain text: `[text](url)` →
/// `text`, drop code-span backticks, drop `*`/`_` **emphasis delimiters** while
/// keeping them intraword (so `lint_str` / `MAX_SEGMENT_WORDS` identifiers
/// survive), and collapse runs of whitespace to single spaces (trimming ends).
fn clean_inline_markdown(line: &str) -> String {
    let chars: Vec<char> = strip_inline_links(line).chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        match c {
            // Code-span delimiter: drop, keep the content.
            '`' => {},
            // A `*`/`_` flanked by alphanumerics on BOTH sides is an
            // identifier char, not an emphasis delimiter — keep it; drop it
            // otherwise (`*bold*`, `_em_`, `**strong**`).
            '*' | '_'
                if !(i > 0
                    && chars[i - 1].is_alphanumeric()
                    && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric())) => {},
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Replace `[text](url)` spans with their `text`, leaving everything else
/// (including lone brackets and reference-style links) untouched.
fn strip_inline_links(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((text, next)) = link_at(&chars, i) {
            out.extend(text);
            i = next;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// If a `[text](url)` link starts at `chars[start]`, return its `text` slice
/// and the index just past the closing `)`. Otherwise `None`. The text ends at
/// the first `]` immediately followed by `(`, so brackets inside the text
/// (`[see [ref]](url)`) don't cut it short.
fn link_at(chars: &[char], start: usize) -> Option<(&[char], usize)> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
    let close =
        (start + 1..chars.len()).find(|&j| chars[j] == ']' && chars.get(j + 1) == Some(&'('))?;
    let paren = (close + 2..chars.len()).find(|&j| chars[j] == ')')?;
    Some((&chars[start + 1..close], paren + 1))
}

/// Truncate `s` to at most `max_chars` `char`s, backing off to the last word
/// boundary so no word is split. Returns `s` unchanged when it already fits;
/// otherwise appends `" …"`. A single over-long word with no interior boundary
/// is hard-cut on a `char` boundary (never mid-UTF-8).
fn truncate_on_word_boundary(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    // Byte index just past the `max_chars`-th char — always a char boundary.
    let mut end = 0;
    for (count, (idx, ch)) in s.char_indices().enumerate() {
        if count >= max_chars {
            break;
        }
        end = idx + ch.len_utf8();
    }
    let prefix = &s[..end];
    let cut = match prefix.rfind(' ') {
        Some(pos) => &prefix[..pos],
        None => prefix,
    };
    format!("{} …", cut.trim_end())
}

/// The values a feature carries for one config-declared schema axis (a name
/// in [`Frontmatter::FIELD_NAMES`]), with blank entries dropped — a field
/// present but empty carries nothing.
///
/// Everything that asks "does this feature hold this axis?" goes through
/// [`Frontmatter::field_values`] here, so there is exactly one answer.
pub(crate) fn axis_values(fm: &Frontmatter, axis: &str) -> Vec<String> {
    fm.field_values(axis)
        .unwrap_or_default()
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .collect()
}

/// Does this project hold `axis` at all — does *any* feature carry a value
/// for it?
///
/// The single predicate behind two decisions that must never disagree
/// ([ADR-0002](../docs/adr/0002-partial-schema-adoption.md)):
///
/// - `render` emits an axis column iff the axis is in use;
/// - `validate` requires a `[fields.<axis>]` declaration iff the axis is in
///   use — declaring an axis nothing carries is the second home for a value
///   that ADR exists to remove.
///
/// `render` asks the question of its own cell matrix rather than calling
/// this (its `Class/Sev` column merges two axes, and probing the matrix is
/// what keeps the probe and the emitted cells from diverging), but both
/// sides read [`axis_values`], so "carries a value" means one thing.
pub(crate) fn axis_in_use(features: &[Feature], axis: &str) -> bool {
    features
        .iter()
        .any(|f| !axis_values(&f.frontmatter, axis).is_empty())
}

/// HTML id for the anchor: lowercase the feature id.
/// Matches GitHub's `<a id="f46">` / `<a id="f-foo">` convention.
///
/// Single definition of the id → anchor rule, shared by the renderer,
/// `validate` (collision detection), and `rename` (link rewriting) so
/// the three can never disagree on what a feature's anchor is.
pub(crate) fn anchor_id(id: &str) -> String {
    id.to_lowercase()
}

/// Percent-encode a value for substitution into a `link` template.
///
/// Keeps the RFC 3986 unreserved set plus the few sub-delimiters a forge
/// path routinely carries (`/`, `-`, `_`, `.`, `~`); everything else — a
/// space, a parenthesis, a `|` — becomes `%XX`. Without this, a value
/// like `Jane Doe (ops)` produces a markdown link that terminates at the
/// first space and leaves an unbalanced paren behind.
///
/// Hand-rolled rather than pulled in: it's a byte loop over a table, and
/// a dependency for that would be its own kind of cost.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            },
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Escape a value used as markdown *link text*: an unescaped `[` or `]`
/// would close the text early and leave the rest as literal characters.
fn escape_link_text(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('[', r"\[")
        .replace(']', r"\]")
}

/// Escape free text destined for a `|`-delimited markdown table cell:
/// a literal `|` would open a spurious column and a newline would break
/// the row, so escape the former and fold the latter to a space.
fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// One catalog column: header label plus how to read a feature's cell.
/// `value` returning `None` means "this feature has no value for the
/// axis" — a non-`always` column is omitted entirely when every feature
/// returns `None`, and a present column renders `None` cells as `—`.
struct CatalogColumn<'a> {
    /// Owned, not `&'static str`: the bucket column's header is
    /// project-declared ([`Config::bucket_label`]).
    header: String,
    /// Table identity (`ID` / `Status` / `Summary`): emitted even when
    /// a value would be empty, so the catalog shape is stable.
    always: bool,
    value: CellFn<'a>,
}

/// How a column reads one feature's cell. Boxed rather than a plain `fn`
/// pointer so a column can close over the config — the bucket column's
/// cell depends on which buckets are declared and on whether sections
/// carry them.
type CellFn<'a> = Box<dyn Fn(&Feature) -> Option<String> + 'a>;

/// One axis column's cell: the feature's values for `axis`, joined, or
/// `None` when it carries none — the `None` that makes the column
/// conditional. Reads [`axis_values`], the same accessor `validate` uses to
/// decide whether `[fields.<axis>]` is required.
fn axis_cell(f: &Feature, axis: &str) -> Option<String> {
    let values = axis_values(&f.frontmatter, axis);
    (!values.is_empty()).then(|| escape_cell(&values.join(", ")))
}

/// The catalog columns in emission order. Axis columns are conditional:
/// a project that never uses an axis gets no column for it.
fn catalog_columns(config: &Config) -> Vec<CatalogColumn<'_>> {
    let declared: HashSet<&str> = config.versions.iter().map(String::as_str).collect();
    let split = config.split_by_bucket;
    let mut columns = vec![
        CatalogColumn {
            header: "ID".into(),
            always: true,
            // Escaped as a whole: a `|` in a hand-written id would
            // otherwise open a phantom column and shift every cell after.
            value: Box::new(|f| {
                let id = &f.frontmatter.id;
                Some(escape_cell(&format!("[{id}](#{aid})", aid = anchor_id(id))))
            }),
        },
        CatalogColumn {
            header: "Type".into(),
            always: false,
            value: Box::new(|f| axis_cell(f, "type")),
        },
        CatalogColumn {
            header: "Class/Sev".into(),
            always: false,
            // `class` (feature-only) and `severity` (fix-only) are
            // mutually exclusive by taxonomy, so they share one column.
            value: Box::new(|f| axis_cell(f, "class").or_else(|| axis_cell(f, "severity"))),
        },
        CatalogColumn {
            header: "Effort".into(),
            always: false,
            value: Box::new(|f| axis_cell(f, "effort")),
        },
        CatalogColumn {
            header: "Area".into(),
            always: false,
            value: Box::new(|f| axis_cell(f, "area")),
        },
        CatalogColumn {
            header: "Horizon".into(),
            always: false,
            value: Box::new(|f| axis_cell(f, "horizon")),
        },
        CatalogColumn {
            header: "Status".into(),
            always: true,
            value: Box::new(|f| Some(f.frontmatter.status.glyph().to_string())),
        },
        CatalogColumn {
            header: config
                .bucket_label
                .clone()
                .unwrap_or_else(|| DEFAULT_BUCKET_LABEL.to_string()),
            always: false,
            // Conditional twice over. Absent like any other axis when no
            // feature carries a `target`; and under `split_by_bucket` the
            // section heading already names the bucket, so such a feature
            // needs no cell — leaving the column to disappear on its own
            // via the usual all-`None` rule.
            //
            // Suppressed only when the heading carries the *whole* value:
            // a single target, and a declared one. `target` is a list, and
            // only its first entry picks the section — dropping the cell
            // for `["v0.2", "v0.3"]` would erase `v0.3` from the document
            // entirely, and an undeclared target has no heading to carry
            // it at all. Either way the cell stays, so splitting never
            // hides a value.
            value: Box::new(move |f| {
                let t = &f.frontmatter.target;
                if t.is_empty() {
                    return None;
                }
                let carried_by_heading = t.len() == 1 && declared.contains(t[0].as_str()) && split;
                if carried_by_heading {
                    return None;
                }
                Some(escape_cell(&t.join(" → ")))
            }),
        },
        CatalogColumn {
            header: "Summary".into(),
            always: true,
            value: Box::new(|f| Some(escape_cell(&summary(&f.body)))),
        },
    ];
    // Project-declared columns land between the built-in axes and
    // `Summary`: `Summary` is free text and always last, and a declared
    // field is a fact about the feature like the axes before it.
    // `BTreeMap` order, so the catalog doesn't reshuffle between runs.
    let summary_col = columns.pop().expect("Summary column");
    for (name, spec) in &config.fields {
        let Some(header) = &spec.column else {
            continue;
        };
        // A built-in axis already has its column above; emitting a second
        // one would print the same value twice under two headers.
        // `validate` reports the declaration; skipping here keeps a tree
        // that ignored the report from rendering the duplicate.
        if Frontmatter::FIELD_NAMES.contains(&name.as_str()) {
            continue;
        }
        let name = name.clone();
        let link = spec.link.clone();
        columns.push(CatalogColumn {
            header: header.clone(),
            always: false,
            value: Box::new(move |f| {
                let values = axis_values(&f.frontmatter, &name);
                if values.is_empty() {
                    return None;
                }
                let rendered: Vec<String> = values
                    .iter()
                    .map(|v| match &link {
                        // The value goes in the link *text* as written, so
                        // `#42` reads as `#42` and links to issue 42 — but
                        // the URL gets a percent-encoded copy, or a value
                        // like `Jane Doe (ops)` emits a link whose spaces
                        // and unbalanced parens break it.
                        Some(template) => {
                            format!(
                                "[{}]({})",
                                escape_link_text(v),
                                template.replace("{}", &percent_encode(v.trim_start_matches('#')))
                            )
                        },
                        None => v.clone(),
                    })
                    .collect();
                Some(escape_cell(&rendered.join(", ")))
            }),
        });
    }
    columns.push(summary_col);
    columns
}

/// The feature's bucket, when its first `target` names a declared one.
///
/// `None` covers both "no target" and "a target `versions` doesn't
/// declare" — the two cases `sort_features` already ranks together at the
/// tail, so grouping and sorting agree without a second rule.
fn declared_bucket<'a>(f: &'a Feature, declared: &HashSet<&str>) -> Option<&'a str> {
    f.frontmatter
        .target
        .first()
        .map(String::as_str)
        .filter(|t| declared.contains(t))
}

/// The catalog rows, grouped for emission: `(heading, row indices)` pairs.
///
/// Indices rather than features so the caller can read the cell matrix it
/// already built — every cell stays evaluated exactly once, which is what
/// keeps the column-presence probe and the emitted cells in step.
///
/// Flat mode is one group under `Feature catalog`. Under
/// `split_by_bucket` the groups follow [`Config::versions`] order — the
/// same order `sort_features` ranks by, so a section's rows keep the
/// document's global ordering — with a trailing group for features whose
/// first `target` is not a declared bucket (or absent), which is where
/// `sort_features` already puts them. An empty bucket yields no group, so
/// a declared-but-unused bucket leaves no hole.
///
/// A project with no features at all falls back to the flat group: there
/// is nothing to split, and dropping every empty section would leave the
/// document with no catalog and no header row — a shape flat mode never
/// produces, and the one an `init`ed-but-unfilled tree would hit first.
fn catalog_groups(features: &[Feature], config: &Config) -> Vec<(String, Vec<usize>)> {
    if !config.split_by_bucket || features.is_empty() {
        return vec![(
            FLAT_CATALOG_HEADING.to_string(),
            (0..features.len()).collect(),
        )];
    }
    let declared: HashSet<&str> = config.versions.iter().map(String::as_str).collect();
    let indices_where = |keep: &dyn Fn(Option<&str>) -> bool| -> Vec<usize> {
        features
            .iter()
            .enumerate()
            .filter(|(_, f)| keep(declared_bucket(f, &declared)))
            .map(|(i, _)| i)
            .collect()
    };
    // Deduplicated, first declaration wins — the same rank `index_of`
    // gives it, which is what keeps a section's position and its rows'
    // order from contradicting each other (#47). `validate` rejects the
    // repeat outright; this stays because `render` must still emit
    // *something* for a config that hasn't been through the gate, and
    // emitting the section twice would put every one of its ID links —
    // and so its `<a id>` anchor target — in the document twice.
    let mut seen = HashSet::new();
    let mut groups: Vec<(String, Vec<usize>)> = config
        .versions
        .iter()
        .filter(|v| seen.insert(v.as_str()))
        .map(|v| (v.clone(), indices_where(&|b| b == Some(v.as_str()))))
        .collect();
    groups.push((
        config.unbucketed_heading().to_string(),
        indices_where(&|b| b.is_none()),
    ));
    groups.retain(|(_, rows)| !rows.is_empty());
    groups
}

/// Leave `out` ending in exactly one blank line, so the next block can be
/// appended without reasoning about what the previous one left behind.
///
/// Every structural block calls this before writing its heading, which is
/// what lets hand-written sections be spliced between any two of them.
fn end_with_blank_line(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

/// Splice in the hand-written sections declared for `slot`, in
/// declaration order.
///
/// The body goes in **verbatim** — roadmark neither parses nor reformats
/// it. Only its *framing* is normalised: leading and trailing blank lines
/// are dropped so the document's spacing doesn't depend on how the
/// author's editor happened to save the file, and a file that is entirely
/// blank contributes nothing rather than a hole.
fn write_sections(out: &mut String, sections: &[LoadedSection], slot: Slot) {
    for section in sections.iter().filter(|s| s.slot == slot) {
        let body = section.body.trim_matches('\n');
        if body.trim().is_empty() {
            continue;
        }
        end_with_blank_line(out);
        out.push_str(body);
        out.push('\n');
    }
}

pub fn render(features: &[Feature], config: &Config, sections: &[LoadedSection]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(8 * 1024);
    let _ = writeln!(out, "# {}\n", config.title);
    out.push_str(
        "<!-- DO NOT EDIT — generated by `roadmark generate`. Source of truth: `.roadmap/`.",
    );
    if let Some(note) = &config.source_note {
        // A literal `-->` in the note would close the banner comment early,
        // leaking the remainder as visible text — neutralise it.
        let _ = write!(out, " {}", note.replace("-->", "--&gt;"));
    }
    out.push_str(" -->\n\n");
    write_sections(&mut out, sections, Slot::BeforeCatalog);
    // Only emit an axis column when at least one feature carries a value
    // for it; `always` columns are the table's identity and stay put.
    // Every cell is evaluated exactly once — the presence probe and the
    // emitted cells read the same matrix, so they cannot diverge.
    //
    // The probe spans *every* feature, not each section's own rows: one
    // column set for the whole document keeps the tables comparable, and
    // a per-section set would let a column appear and vanish down the
    // page for the same project.
    let columns = catalog_columns(config);
    let matrix: Vec<Vec<Option<String>>> = features
        .iter()
        .map(|f| columns.iter().map(|c| (c.value)(f)).collect())
        .collect();
    let active: Vec<usize> = (0..columns.len())
        .filter(|&i| columns[i].always || matrix.iter().any(|row| row[i].is_some()))
        .collect();
    let headers: Vec<&str> = active.iter().map(|&i| columns[i].header.as_str()).collect();
    for (heading, rows) in catalog_groups(features, config) {
        end_with_blank_line(&mut out);
        let _ = writeln!(out, "## {heading}\n");
        let _ = writeln!(out, "| {} |", headers.join(" | "));
        let _ = writeln!(out, "|{}", "---|".repeat(active.len()));
        for row in rows.into_iter().map(|i| &matrix[i]) {
            let line: Vec<&str> = active
                .iter()
                .map(|&i| row[i].as_deref().unwrap_or("—"))
                .collect();
            let _ = writeln!(out, "| {} |", line.join(" | "));
        }
    }
    write_sections(&mut out, sections, Slot::AfterCatalog);
    if !features.is_empty() {
        end_with_blank_line(&mut out);
        let _ = writeln!(out, "## {DETAILS_HEADING}");
        for f in features {
            let fm = &f.frontmatter;
            // The `<a id>` anchor lives on the detail heading, so the
            // catalog's ID link jumps here (and anchor drift still sees
            // one anchor per feature).
            let _ = write!(
                out,
                "\n### <a id=\"{aid}\"></a>{id}\n\n",
                aid = anchor_id(&fm.id),
                id = fm.id
            );
            if let Some(line) = shipped_line(&fm.shipped) {
                let _ = writeln!(out, "{line}\n");
            }
            let body = f.body.trim();
            if !body.is_empty() {
                let _ = writeln!(out, "{body}");
            }
        }
    }
    write_sections(&mut out, sections, Slot::AfterDetails);
    out
}

/// One-line shipping record for the Details section, or `None` when the
/// feature carries no shipped metadata (`version` is the marker field).
fn shipped_line(shipped: &Shipped) -> Option<String> {
    if shipped.version.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !shipped.date.is_empty() {
        parts.push(shipped.date.clone());
    }
    if shipped.pr != 0 {
        parts.push(format!("PR #{}", shipped.pr));
    }
    Some(if parts.is_empty() {
        format!("Shipped in {}.", shipped.version)
    } else {
        format!("Shipped in {} ({}).", shipped.version, parts.join(", "))
    })
}

/// List `*.md` files directly under `dir`, in filename order.
///
/// Single source of the "which files are feature files" rule, shared by
/// `load_features` (generate) and `validate` so the two can never drift
/// on the walk depth, sort, or extension filter.
pub fn feature_md_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .min_depth(1)
        .max_depth(1)
        .sort_by_file_name()
    {
        let entry = entry.context("walking features dir")?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "md") {
            paths.push(entry.path().to_path_buf());
        }
    }
    Ok(paths)
}

/// Read all `.roadmap/features/*.md` files under `root`, parse each,
/// and return them in load order (caller sorts).
pub fn load_features(root: &Path, config: &Config) -> Result<Vec<Feature>> {
    let dir = root.join("features");
    if !dir.is_dir() {
        bail!("expected directory: {}", dir.display());
    }
    let mut out = Vec::new();
    for path in feature_md_paths(&dir)? {
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let feature = parse_feature(&src).with_context(|| format!("parsing {}", path.display()))?;
        check_declared_fields(&feature.frontmatter, config)
            .with_context(|| format!("parsing {}", path.display()))?;
        out.push(feature);
    }
    Ok(out)
}

/// Write `contents` to `path` atomically: a sibling temp file first, then a
/// rename over the destination.
///
/// The point is the *failure* path, not the success one. `roadmark generate >
/// ROADMAP.md` has the shell truncate the destination to zero bytes before the
/// binary runs, so any error — an unparseable feature file, a missing config —
/// leaves the committed roadmap empty and nothing written in its place. Going
/// through a temp file removes the shell from the write path: the destination
/// is untouched until a complete document exists on disk.
///
/// The temp file is a sibling (same directory, so the rename stays within one
/// filesystem and is therefore atomic), named per-process so concurrent runs
/// in the same directory can't clobber each other's staging file. It is
/// removed on any failure after creation, so a failed run leaves no litter.
///
/// Two behaviours the shell redirect had and a naive rename would lose, both
/// restored here: a symlinked destination is written *through* rather than
/// replaced, and an existing destination keeps its permission bits.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    // `fs::rename` does not follow a symlink at the destination — it swaps the
    // link itself for a regular file and leaves the real document stale, which
    // is exactly the silent-staleness a docs-site checkout would hit. Resolve
    // the link so the temp file lands beside the *target*. A dangling or
    // otherwise unresolvable link falls back to the path as given.
    let resolved = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path).ok(),
        _ => None,
    };
    let path = resolved.as_deref().unwrap_or(path);

    // A bare relative filename (`ROADMAP.md`) has an empty parent, which is
    // not a directory any temp file can live in — that's the current one.
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("not a file path: {}", path.display()))?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(format!(".roadmark-tmp-{}", std::process::id()));
    let tmp = dir.join(tmp_name);

    // The temp file is created fresh at `0666 & !umask`; without this the
    // rename would widen (or otherwise reset) a deliberately restricted mode
    // on every regen.
    let perms = std::fs::metadata(path).ok().map(|m| m.permissions());

    if let Err(e) = stage(&tmp, contents, perms) {
        // Cleanup covers the write itself, not just the rename: a failure
        // partway through `write_all` would otherwise strand a truncated
        // staging file in the user's working tree.
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing temp file {}", tmp.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("replacing {}", path.display()));
    }
    // Best-effort: persist the directory entry, so the rename survives a crash
    // too. Opening a directory isn't portable (it fails on Windows), and the
    // roadmap is regenerable, so a failure here is not worth reporting.
    let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    Ok(())
}

/// Write the staging file and get it onto the disk before it is renamed into
/// place: atomic against *errors* is not atomic against a crash, and on
/// several filesystems an unsynced rename can surface as a zero-length
/// destination after a power loss — the very outcome `write_atomic` exists to
/// prevent.
fn stage(tmp: &Path, contents: &str, perms: Option<std::fs::Permissions>) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(tmp)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);
    if let Some(perms) = perms {
        std::fs::set_permissions(tmp, perms)?;
    }
    Ok(())
}

/// Frontmatter keys the generator doesn't model and the config doesn't
/// declare, in sorted order.
///
/// This is the guarantee `deny_unknown_fields` gave [`Frontmatter`] until
/// project-declared fields (#22) forced `serde(flatten)`, which serde
/// cannot combine with it. The rule is unchanged in substance — every axis
/// is optional, so a typo'd key would otherwise read as an absent field —
/// and stronger in one respect: it can name the missing declaration
/// instead of just refusing the key.
pub fn undeclared_fields(fm: &Frontmatter, config: &Config) -> Vec<String> {
    fm.extra_names()
        .filter(|name| !config.fields.contains_key(*name))
        .map(str::to_string)
        .collect()
}

/// Reject a feature carrying a key nothing declares.
///
/// Called by [`load_features`] so `generate` fails on a typo exactly as it
/// did before #22. `validate` deliberately does *not* call this — it
/// collects the same finding per file instead of bailing on the first.
pub fn check_declared_fields(fm: &Frontmatter, config: &Config) -> Result<()> {
    let unknown = undeclared_fields(fm, config);
    if unknown.is_empty() {
        return Ok(());
    }
    bail!(
        "unknown frontmatter key(s): {} — declare in config.toml, or fix the spelling",
        unknown
            .iter()
            .map(|n| format!("`{n}` (add `[fields.{n}]`)"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Resolve a declared section path against the `.roadmap/` root.
///
/// Rejects anything that would leave the source tree — an absolute path,
/// or any `..` component. The config is the project's own, so this is not
/// a security boundary; it is a predictability one. `.roadmap/` is the
/// source of truth, and a document assembled partly from outside it can't
/// be reproduced from a checkout of it.
///
/// Shared with `validate`, so the path it reports missing is the path
/// `generate` would read.
pub(crate) fn section_path(root: &Path, file: &str) -> Result<PathBuf> {
    let rel = Path::new(file);
    if rel.is_absolute() {
        bail!("section file must be relative to the roadmap root: {file}");
    }
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("section file must stay inside the roadmap root: {file}");
    }
    Ok(root.join(rel))
}

/// Read every declared section file, in declaration order.
///
/// The I/O half of [`Section`]; `render` takes the results so it stays
/// filesystem-free and snapshot-testable. A missing file is an error
/// here — a declared section that silently vanished would leave a hole
/// in the document that nothing else would report.
pub fn load_sections(root: &Path, config: &Config) -> Result<Vec<LoadedSection>> {
    config
        .sections
        .iter()
        .map(|s| {
            let path = section_path(root, &s.file)?;
            let body = std::fs::read_to_string(&path)
                .with_context(|| format!("reading section {}", path.display()))?;
            Ok(LoadedSection { slot: s.slot, body })
        })
        .collect()
}

pub fn load_config(root: &Path) -> Result<Config> {
    let path = root.join("config.toml");
    let src =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&src).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut fields = BTreeMap::new();
        fields.insert(
            "horizon".to_string(),
            FieldSpec {
                values: ["now", "next", "later", "parked", "shipped"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                multi: false,
                required_when: None,
                ..FieldSpec::default()
            },
        );
        Config {
            versions: vec!["v0.2.x".into(), "v0.3".into(), "v0.4".into()],
            fields,
            ..Config::default()
        }
    }

    fn feat(id: &str, status: Status, horizon: &str, target: &str) -> Feature {
        Feature {
            frontmatter: Frontmatter {
                id: id.into(),
                item_type: "feature".into(),
                class: None,
                effort: None,
                area: vec!["arch".into()],
                horizon: Some(horizon.into()),
                status,
                target: vec![target.into()],
                severity: None,
                shipped: Shipped::default(),
                shipped_order: None,
                extra: BTreeMap::new(),
            },
            body: "Summary line.".into(),
        }
    }

    #[test]
    fn split_frontmatter_basic() {
        let src = "+++\nid = \"f1\"\n+++\n\nbody text\n";
        let (toml, body) = split_frontmatter(src).unwrap();
        assert_eq!(toml, "id = \"f1\"");
        assert_eq!(body, "body text\n");
    }

    #[test]
    fn parse_minimal() {
        let src = "+++\n\
id = \"F-foo\"\n\
type = \"feature\"\n\
area = [\"arch\"]\n\
horizon = \"next\"\n\
status = \"todo\"\n\
target = [\"v0.2.x\"]\n\
+++\n\nThe summary.\n";
        let f = parse_feature(src).unwrap();
        assert_eq!(f.frontmatter.id, "F-foo");
        assert_eq!(f.frontmatter.item_type, "feature");
        assert_eq!(f.frontmatter.area, vec!["arch".to_string()]);
        assert_eq!(f.frontmatter.horizon.as_deref(), Some("next"));
        assert_eq!(f.frontmatter.status, Status::Todo);
        assert_eq!(f.body, "The summary.\n");
    }

    #[test]
    fn parse_blocked_status() {
        let src = "+++\n\
id = \"F-foo\"\n\
type = \"feature\"\n\
area = [\"arch\"]\n\
horizon = \"next\"\n\
status = \"blocked\"\n\
target = [\"v0.2.x\"]\n\
+++\n\nThe summary.\n";
        let f = parse_feature(src).unwrap();
        assert_eq!(f.frontmatter.status, Status::Blocked);
    }

    #[test]
    fn parse_without_horizon() {
        let src = "+++\n\
id = \"F-board\"\n\
type = \"feature\"\n\
area = [\"arch\"]\n\
status = \"todo\"\n\
target = [\"v0.2.x\"]\n\
+++\n\nPriority lives on the board.\n";
        let f = parse_feature(src).unwrap();
        assert_eq!(f.frontmatter.id, "F-board");
        assert_eq!(f.frontmatter.horizon, None);
    }

    const TYPO_SRC: &str = "+++\n\
id = \"F-foo\"\n\
type = \"feature\"\n\
area = [\"arch\"]\n\
horizen = \"next\"\n\
status = \"todo\"\n\
target = [\"v0.2.x\"]\n\
+++\n\nThe summary.\n";

    /// With `horizon` optional, a typo'd key would otherwise silently read
    /// as "no horizon". `deny_unknown_fields` used to keep that a parse
    /// error; project-declared fields (#22) force `serde(flatten)`, which
    /// serde can't combine with it, so the key now *parses* into `extra`
    /// and the rejection moves one layer out.
    #[test]
    fn parse_keeps_an_unmodelled_key_instead_of_dropping_it() {
        let f = parse_feature(TYPO_SRC).unwrap();
        assert_eq!(f.frontmatter.horizon, None);
        // Kept, not discarded — which is what lets the next layer name it.
        assert!(f.frontmatter.extra.contains_key("horizen"));
    }

    /// The guarantee itself, at its new home: a key no `[fields.*]`
    /// declares is still refused, and the message names the declaration
    /// that would make it legal.
    #[test]
    fn an_undeclared_key_is_rejected_against_the_config() {
        let f = parse_feature(TYPO_SRC).unwrap();
        let err = check_declared_fields(&f.frontmatter, &cfg()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("horizen"), "got: {msg}");
        assert!(msg.contains("[fields.horizen]"), "got: {msg}");
    }

    /// …and a key the config *does* declare is accepted, which is the
    /// whole point of the change.
    #[test]
    fn a_declared_key_is_accepted() {
        let src = "+++\n\
id = \"F-foo\"\n\
type = \"feature\"\n\
area = [\"arch\"]\n\
tracked = 42\n\
status = \"todo\"\n\
target = [\"v0.2.x\"]\n\
+++\n\nThe summary.\n";
        let f = parse_feature(src).unwrap();
        let mut config = cfg();
        config.fields.insert(
            "tracked".to_string(),
            FieldSpec {
                kind: Some(FieldKind::IssueRef),
                ..FieldSpec::default()
            },
        );
        assert!(check_declared_fields(&f.frontmatter, &config).is_ok());
        // An integer answers as a string, so every consumer treats a
        // declared field exactly like a built-in axis.
        assert_eq!(
            f.frontmatter.field_values("tracked"),
            Some(vec!["42".to_string()])
        );
    }

    #[test]
    fn parse_accepts_crlf_line_endings() {
        let src = "+++\r\n\
id = \"F-foo\"\r\n\
type = \"feature\"\r\n\
area = [\"arch\"]\r\n\
horizon = \"next\"\r\n\
status = \"todo\"\r\n\
target = [\"v0.2.x\"]\r\n\
+++\r\n\r\nThe summary.\r\n";
        let f = parse_feature(src).unwrap();
        assert_eq!(f.frontmatter.id, "F-foo");
        assert_eq!(f.body, "The summary.\n");
    }

    #[test]
    fn sort_target_then_status_then_horizon_then_id() {
        let mut fs = vec![
            feat("f-z", Status::Todo, "next", "v0.3"),
            feat("f-a", Status::Todo, "next", "v0.2.x"),
            feat("f-b", Status::Wip, "next", "v0.2.x"),
            feat("f-c", Status::Todo, "later", "v0.2.x"),
        ];
        sort_features(&mut fs, &cfg());
        let ids: Vec<&str> = fs.iter().map(|f| f.frontmatter.id.as_str()).collect();
        assert_eq!(ids, vec!["f-b", "f-a", "f-c", "f-z"]);
    }

    #[test]
    fn blocked_sorts_after_wip_and_before_todo() {
        let mut fs = vec![
            feat("f-todo", Status::Todo, "next", "v0.2.x"),
            feat("f-blocked", Status::Blocked, "next", "v0.2.x"),
            feat("f-wip", Status::Wip, "next", "v0.2.x"),
        ];
        sort_features(&mut fs, &cfg());
        let ids: Vec<&str> = fs.iter().map(|f| f.frontmatter.id.as_str()).collect();
        assert_eq!(ids, vec!["f-wip", "f-blocked", "f-todo"]);
    }

    #[test]
    fn missing_horizon_sorts_last_within_bucket() {
        let mut no_horizon = feat("f-a", Status::Todo, "unused", "v0.2.x");
        no_horizon.frontmatter.horizon = None;
        let mut fs = vec![
            no_horizon,
            // `parked` is the last declared horizon — a feature without
            // one must still land after it, but before the next bucket.
            feat("f-parked", Status::Todo, "parked", "v0.2.x"),
            feat("f-next-bucket", Status::Todo, "now", "v0.3"),
        ];
        sort_features(&mut fs, &cfg());
        let ids: Vec<&str> = fs.iter().map(|f| f.frontmatter.id.as_str()).collect();
        assert_eq!(ids, vec!["f-parked", "f-a", "f-next-bucket"]);
    }

    #[test]
    fn anchor_lowercases_id() {
        assert_eq!(anchor_id("F-Roadmap-TOML"), "f-roadmap-toml");
        assert_eq!(anchor_id("F22"), "f22");
    }

    #[test]
    fn title_defaults_when_omitted() {
        let config: Config = toml::from_str("versions = [\"v1\"]\n").unwrap();
        assert_eq!(config.title, "Roadmap");
        assert!(config.source_note.is_none());
        assert!(config.fields.is_empty());
    }

    #[test]
    fn fields_parse_from_config() {
        let src = "versions = [\"v1\"]\n\
[fields.class]\n\
values = [\"differentiator\", \"enabler\"]\n\
[fields.area]\n\
values = [\"rules\", \"docs\"]\n\
multi = true\n\
[fields.class.required_when]\n\
type = \"feature\"\n";
        let config: Config = toml::from_str(src).unwrap();
        assert_eq!(config.fields["area"].values, vec!["rules", "docs"]);
        assert!(config.fields["area"].multi);
        assert!(!config.fields["class"].multi);
        assert_eq!(
            config.fields["class"].required_when.as_ref().unwrap()["type"],
            Condition::One("feature".to_string())
        );
    }

    #[test]
    fn summary_short_line_returned_unchanged() {
        let body = "A concise lead sentence.\n\nMore prose below.";
        let s = summary(body);
        assert_eq!(s, "A concise lead sentence.");
        assert!(!s.contains('…'));
    }

    /// A summary wrapped by an 80-column house style is one sentence, not two
    /// cells' worth — the whole first paragraph makes the cell (#55).
    #[test]
    fn summary_joins_a_wrapped_first_paragraph() {
        let body = "A summary sentence that the author wrapped\n\
                    across two source lines on purpose.\n\n\
                    Second paragraph.\n";
        assert_eq!(
            summary(body),
            "A summary sentence that the author wrapped across two source lines on purpose."
        );
    }

    /// The blank line still ends it: later paragraphs stay out of the cell.
    #[test]
    fn summary_stops_at_the_first_blank_line() {
        let body = "\n\nLead sentence.\n\nSecond paragraph must not appear.\n";
        assert_eq!(summary(body), "Lead sentence.");
    }

    #[test]
    fn summary_long_line_truncates_on_word_boundary() {
        let word = "lorem ipsum dolor ";
        let long = word.repeat(20); // ~360 chars, no markdown
        let s = summary(&long);
        assert!(s.ends_with(" …"), "expected trailing ellipsis, got {s:?}");
        assert!(
            s.chars().count() <= 122,
            "summary too long: {} chars",
            s.chars().count()
        );
        // The kept prefix stops on a whole word — no dangling partial token.
        let kept = s.trim_end_matches('…').trim_end();
        assert!(kept
            .split(' ')
            .all(|w| ["lorem", "ipsum", "dolor"].contains(&w)));
    }

    #[test]
    fn summary_strips_inline_markdown() {
        let body = "Migrate `.roadmap/` with *bold* and _em_ and a [F22](#f22) link.";
        let s = summary(body);
        assert!(!s.contains('`'), "backticks remain: {s:?}");
        assert!(!s.contains('*'), "asterisks remain: {s:?}");
        assert!(!s.contains('_'), "underscores remain: {s:?}");
        assert!(s.contains("F22"));
        assert!(!s.contains("#f22"), "link url leaked: {s:?}");
        assert_eq!(s, "Migrate .roadmap/ with bold and em and a F22 link.");
    }

    #[test]
    fn summary_never_cuts_mid_word() {
        // 120-char budget lands inside a word; the boundary back-off must keep
        // only whole words.
        let line = "alpha bravo charlie delta echo foxtrot golf hotel india juliet \
                    kilo lima mike november oscar papa quebec romeo sierra tango";
        let s = summary(line);
        assert!(s.ends_with(" …"));
        let kept = s.trim_end_matches('…').trim_end();
        let words: Vec<&str> = line.split_whitespace().collect();
        // Every kept token is a complete original word (prefix of the list).
        for (i, w) in kept.split(' ').enumerate() {
            assert_eq!(w, words[i], "token {i} was cut mid-word: {w:?}");
        }
    }

    #[test]
    fn summary_is_multibyte_safe() {
        // Accented chars and em dashes: must not panic and must cut on a char
        // boundary (valid UTF-8 out).
        let line = "é—é—é ".repeat(60); // well over 120 chars, multibyte throughout
        let s = summary(&line);
        assert!(s.ends_with(" …"));
        assert!(s.chars().count() <= 122);
        // Round-trips as valid UTF-8 (no mid-char split would have panicked).
        assert!(String::from_utf8(s.into_bytes()).is_ok());
    }

    #[test]
    fn summary_preserves_snake_case_identifiers() {
        // Underscores/asterisks inside a word are identifier chars, not
        // emphasis — they must survive the strip.
        let body = "Doctests for `Engine::with_profile` and `Engine::lint_str`, \
                    plus MAX_SEGMENT_WORDS.";
        let s = summary(body);
        assert!(s.contains("Engine::with_profile"), "got {s:?}");
        assert!(s.contains("Engine::lint_str"), "got {s:?}");
        assert!(s.contains("MAX_SEGMENT_WORDS"), "got {s:?}");
        assert!(!s.contains('`'), "backticks remain: {s:?}");
    }

    #[test]
    fn summary_link_text_may_contain_brackets() {
        // The `](` boundary closes the link text, not the first `]`, so
        // brackets inside the text are preserved as plain text.
        let s = summary("See [the [inner] note](https://x) here.");
        assert_eq!(s, "See the [inner] note here.");
    }

    #[test]
    fn render_uses_title_and_source_note() {
        let config = Config {
            versions: vec!["v1".into()],
            title: "My Project — Roadmap".into(),
            source_note: Some("See docs/adr.".into()),
            ..Config::default()
        };
        let out = render(&[], &config, &[]);
        assert!(out.starts_with("# My Project — Roadmap\n\n"));
        assert!(out.contains("generated by `roadmark generate`"));
        assert!(out.contains("Source of truth: `.roadmap/`. See docs/adr. -->"));
    }

    #[test]
    fn shipped_order_breaks_ties_before_id() {
        // Same target/status/horizon, distinct ids — the alphabetically
        // later id (f-zeta) must still sort first because its shipped_order
        // is lower. Regression guard: this only works when shipped_order
        // sits before id in the sort key.
        let mut a = feat("f-alpha", Status::Done, "shipped", "v0.2.x");
        a.frontmatter.shipped_order = Some(3);
        let mut z = feat("f-zeta", Status::Done, "shipped", "v0.2.x");
        z.frontmatter.shipped_order = Some(1);
        let mut fs = vec![a, z];
        sort_features(&mut fs, &cfg());
        let ids: Vec<&str> = fs.iter().map(|f| f.frontmatter.id.as_str()).collect();
        assert_eq!(ids, vec!["f-zeta", "f-alpha"]);
    }

    #[test]
    fn features_without_shipped_order_fall_back_to_id() {
        let mut fs = vec![
            feat("f-b", Status::Done, "shipped", "v0.2.x"),
            feat("f-a", Status::Done, "shipped", "v0.2.x"),
        ];
        sort_features(&mut fs, &cfg());
        let ids: Vec<&str> = fs.iter().map(|f| f.frontmatter.id.as_str()).collect();
        assert_eq!(ids, vec!["f-a", "f-b"]);
    }

    #[test]
    fn escape_cell_escapes_pipes_and_folds_newlines() {
        assert_eq!(escape_cell("CLI | TUI"), "CLI \\| TUI");
        assert_eq!(escape_cell("line1\nline2"), "line1 line2");
    }

    #[test]
    fn render_escapes_pipe_in_free_text_columns() {
        let mut f = feat("f-x", Status::Todo, "next", "v0.2.x");
        f.frontmatter.area = vec!["CLI | TUI".into()];
        f.body = "Support `a | b` operator.".into();
        let out = render(&[f], &cfg(), &[]);
        // The row must carry only the intended column separators plus
        // the two escaped literals — never a raw unescaped `|` in the text.
        // The summary strips code-span backticks; pipe-escaping still applies.
        assert!(out.contains("CLI \\| TUI"));
        assert!(out.contains("Support a \\| b operator."));
    }

    #[test]
    fn render_emits_schema_fields_in_catalog_row() {
        let mut f = feat("f-x", Status::Todo, "next", "v0.2.x");
        f.frontmatter.class = Some("enabler".into());
        f.frontmatter.effort = Some("M".into());
        let out = render(&[f], &cfg(), &[]);
        assert!(out.contains("| [f-x](#f-x) | feature | enabler | M | arch | next | ☐ | v0.2.x |"));
    }

    #[test]
    fn render_shows_blocked_glyph_in_catalog_row() {
        let f = feat("f-x", Status::Blocked, "next", "v0.2.x");
        let out = render(&[f], &cfg(), &[]);
        assert!(out.contains("| [f-x](#f-x) | feature | arch | next | ⛔ | v0.2.x |"));
    }

    #[test]
    fn render_omits_horizon_column_until_some_feature_carries_one() {
        // Alone, an absent horizon drops the column entirely…
        let mut f = feat("f-x", Status::Todo, "unused", "v0.2.x");
        f.frontmatter.horizon = None;
        let out = render(&[f.clone()], &cfg(), &[]);
        assert!(!out.contains("Horizon"));
        // …but once any feature carries one, the gap renders as `—`.
        let g = feat("f-y", Status::Todo, "next", "v0.2.x");
        let out = render(&[f, g], &cfg(), &[]);
        assert!(
            out.contains("| [f-x](#f-x) | feature | arch | — | ☐ | v0.2.x |"),
            "got:\n{out}"
        );
    }

    #[test]
    fn render_shows_severity_for_fixes_in_class_sev_column() {
        let mut f = feat("f-broken", Status::Wip, "now", "v0.2.x");
        f.frontmatter.item_type = "fix".into();
        f.frontmatter.severity = Some("major".into());
        let out = render(&[f], &cfg(), &[]);
        assert!(out.contains("| fix | major |"));
    }

    #[test]
    fn render_omits_axis_columns_no_feature_uses() {
        // The stock `feat` carries no class/severity and no effort, so
        // those two columns must vanish; the axes it does carry stay.
        let f = feat("f-x", Status::Todo, "next", "v0.2.x");
        let out = render(&[f], &cfg(), &[]);
        assert!(out.contains("| ID | Type | Area | Horizon | Status | Target | Summary |"));
        assert!(out.contains("|---|---|---|---|---|---|---|\n"));
        assert!(!out.contains("Class/Sev"));
        assert!(!out.contains("Effort"));
        assert!(out.contains("| [f-x](#f-x) | feature | arch | next | ☐ | v0.2.x |"));
    }

    #[test]
    fn render_keeps_partially_used_axis_column_with_dash() {
        // One feature has an effort and an empty target, the other the
        // reverse: both columns must appear, with `—` filling the gaps.
        let mut a = feat("f-a", Status::Todo, "next", "v0.2.x");
        a.frontmatter.effort = Some("M".into());
        a.frontmatter.target = Vec::new();
        let b = feat("f-b", Status::Todo, "next", "v0.2.x");
        let out = render(&[a, b], &cfg(), &[]);
        assert!(out.contains("| ID | Type | Effort | Area | Horizon | Status | Target | Summary |"));
        assert!(out.contains("| [f-a](#f-a) | feature | M | arch | next | ☐ | — |"));
        assert!(out.contains("| [f-b](#f-b) | feature | — | arch | next | ☐ | v0.2.x |"));
    }

    /// `cfg()` with per-bucket sections turned on.
    fn cfg_split() -> Config {
        Config {
            split_by_bucket: true,
            ..cfg()
        }
    }

    #[test]
    fn split_by_bucket_emits_one_catalog_per_declared_bucket() {
        let out = render(
            &[
                feat("f-a", Status::Todo, "now", "v0.2.x"),
                feat("f-b", Status::Todo, "now", "v0.4"),
            ],
            &cfg_split(),
            &[],
        );
        // Sections replace the single flat catalog, in `versions` order.
        assert!(!out.contains("## Feature catalog"), "got {out}");
        let a = out.find("## v0.2.x").expect("first bucket heading");
        let b = out.find("## v0.4").expect("second bucket heading");
        assert!(a < b, "buckets out of declared order:\n{out}");
        // The heading carries the bucket, so the column drops out.
        assert!(!out.contains("| Target |"), "got {out}");
        // `## Details` stays flat and stays one list.
        assert_eq!(out.matches("## Details").count(), 1, "got {out}");
        assert!(out.find("## Details").unwrap() > b, "got {out}");
    }

    #[test]
    fn split_by_bucket_emits_no_heading_for_an_empty_bucket() {
        // `cfg()` declares v0.2.x, v0.3, v0.4; only v0.3 is used.
        let out = render(
            &[feat("f-a", Status::Todo, "now", "v0.3")],
            &cfg_split(),
            &[],
        );
        assert!(out.contains("## v0.3"), "got {out}");
        assert!(!out.contains("## v0.2.x"), "got {out}");
        assert!(!out.contains("## v0.4"), "got {out}");
    }

    #[test]
    fn split_by_bucket_collects_untargeted_features_in_a_trailing_section() {
        let mut loose = feat("f-loose", Status::Todo, "now", "v0.2.x");
        loose.frontmatter.target = Vec::new();
        let out = render(
            &[feat("f-a", Status::Todo, "now", "v0.2.x"), loose],
            &cfg_split(),
            &[],
        );
        let bucket = out.find("## v0.2.x").expect("bucket heading");
        let tail = out.find("## Unscheduled").expect("tail heading");
        assert!(bucket < tail, "tail section not last:\n{out}");
        assert!(out[tail..].contains("[f-loose](#f-loose)"), "got {out}");
    }

    #[test]
    fn split_by_bucket_keeps_the_bucket_column_for_an_undeclared_target() {
        // `v9.9` is not in `versions`, so no heading can carry it — the
        // column has to stay, or splitting would swallow the value.
        let mut stray = feat("f-stray", Status::Todo, "now", "v9.9");
        stray.frontmatter.target = vec!["v9.9".into()];
        let out = render(
            &[feat("f-a", Status::Todo, "now", "v0.2.x"), stray],
            &cfg_split(),
            &[],
        );
        assert!(out.contains("| Target |"), "got {out}");
        assert!(out.contains("| [f-stray](#f-stray) |"), "got {out}");
        assert!(out.contains("| v9.9 |"), "got {out}");
        // The feature that *is* in a section still leaves its cell empty.
        assert!(
            out.contains("| [f-a](#f-a) | feature | arch | now | ☐ | — |"),
            "got {out}"
        );
    }

    #[test]
    fn split_by_bucket_keeps_the_bucket_column_for_a_multi_valued_target() {
        // Only the *first* target picks the section, so the heading does
        // not carry the rest — dropping the cell would erase `v0.4` from
        // the document entirely.
        let mut spanning = feat("f-span", Status::Todo, "now", "v0.2.x");
        spanning.frontmatter.target = vec!["v0.2.x".into(), "v0.4".into()];
        let out = render(&[spanning], &cfg_split(), &[]);
        assert!(out.contains("## v0.2.x"), "got {out}");
        assert!(out.contains("| Target |"), "got {out}");
        assert!(out.contains("v0.2.x → v0.4"), "got {out}");
    }

    #[test]
    fn split_by_bucket_with_no_features_still_emits_a_catalog() {
        // Every section would be empty; falling through to zero sections
        // would leave a document with no catalog and no header row, which
        // flat mode never produces.
        let out = render(&[], &cfg_split(), &[]);
        assert!(out.contains("## Feature catalog"), "got {out}");
        assert!(out.contains("| ID | Status | Summary |"), "got {out}");
    }

    #[test]
    fn split_by_bucket_emits_a_repeated_version_once() {
        // A duplicated `versions` entry is a config mistake `validate`
        // rejects (#47); emitting its section twice would duplicate every
        // ID link, and so every anchor target, in the same document.
        let config = Config {
            versions: vec!["v0.2.x".into(), "v0.3".into(), "v0.2.x".into()],
            ..cfg_split()
        };
        let out = render(&[feat("f-a", Status::Todo, "now", "v0.2.x")], &config, &[]);
        assert_eq!(out.matches("## v0.2.x").count(), 1, "got {out}");
        assert_eq!(out.matches("[f-a](#f-a)").count(), 1, "got {out}");
    }

    /// The half of #47 that isn't a validation error: while the config is
    /// still wrong, the two readers of `versions` must at least agree.
    /// `catalog_groups` keeps the first declaration, so `index_of` has to
    /// rank by it too — last-wins would sort `v0.2.x`'s rows as though the
    /// bucket came after `v0.3` while its section is emitted before it.
    #[test]
    fn a_repeated_version_ranks_and_groups_at_its_first_position() {
        let config = Config {
            versions: vec!["v0.2.x".into(), "v0.3".into(), "v0.2.x".into()],
            ..cfg_split()
        };
        let mut features = vec![
            feat("f-later", Status::Todo, "now", "v0.3"),
            feat("f-early", Status::Todo, "now", "v0.2.x"),
        ];
        sort_features(&mut features, &config);
        assert_eq!(features[0].frontmatter.id, "f-early");

        let out = render(&features, &config, &[]);
        let at = |needle: &str| out.find(needle).unwrap_or_else(|| panic!("got {out}"));
        assert!(at("## v0.2.x") < at("## v0.3"), "got {out}");
        assert!(
            at("[f-early](#f-early)") < at("[f-later](#f-later)"),
            "got {out}"
        );
    }

    #[test]
    fn bucket_label_renames_the_bucket_column() {
        let config = Config {
            bucket_label: Some("Priority".into()),
            ..cfg()
        };
        let out = render(&[feat("f-a", Status::Todo, "now", "v0.2.x")], &config, &[]);
        assert!(out.contains("| Priority |"), "got {out}");
        assert!(!out.contains("| Target |"), "got {out}");
    }

    #[test]
    fn unbucketed_label_renames_the_trailing_section() {
        let mut loose = feat("f-loose", Status::Todo, "now", "v0.2.x");
        loose.frontmatter.target = Vec::new();
        let config = Config {
            unbucketed_label: Some("Needs definition".into()),
            ..cfg_split()
        };
        let out = render(&[loose], &config, &[]);
        assert!(out.contains("## Needs definition"), "got {out}");
        assert!(!out.contains("## Unscheduled"), "got {out}");
    }

    fn section(slot: Slot, body: &str) -> LoadedSection {
        LoadedSection {
            slot,
            body: body.to_string(),
        }
    }

    #[test]
    fn sections_land_at_their_declared_slots() {
        let out = render(
            &[feat("f-a", Status::Todo, "now", "v0.2.x")],
            &cfg(),
            &[
                section(Slot::AfterDetails, "## Epilogue\n"),
                section(Slot::BeforeCatalog, "## Preamble\n"),
                section(Slot::AfterCatalog, "## Notes\n"),
            ],
        );
        let at = |needle: &str| {
            out.find(needle)
                .unwrap_or_else(|| panic!("{needle}\n{out}"))
        };
        assert!(at("## Preamble") < at("## Feature catalog"), "got {out}");
        assert!(at("## Feature catalog") < at("## Notes"), "got {out}");
        assert!(at("## Notes") < at("## Details"), "got {out}");
        assert!(at("## Details") < at("## Epilogue"), "got {out}");
    }

    #[test]
    fn sections_in_one_slot_keep_declaration_order() {
        let out = render(
            &[],
            &cfg(),
            &[
                section(Slot::BeforeCatalog, "first"),
                section(Slot::BeforeCatalog, "second"),
            ],
        );
        assert!(out.find("first") < out.find("second"), "got {out}");
    }

    #[test]
    fn section_body_is_injected_verbatim() {
        // No parsing, no reformatting: a fenced block, a table and a
        // trailing-space line survive exactly as written.
        let body = "Text with `code` and a | pipe.\n\n```rust\nlet x = 1;\n```";
        let out = render(&[], &cfg(), &[section(Slot::BeforeCatalog, body)]);
        assert!(out.contains(body), "got {out}");
    }

    #[test]
    fn section_framing_is_normalised_but_content_is_not() {
        // Blank lines around the body are the document's business, not the
        // author's editor's — otherwise the output depends on how the file
        // happened to be saved.
        let out = render(
            &[],
            &cfg(),
            &[section(Slot::BeforeCatalog, "\n\n\nhello\n\n\n")],
        );
        assert!(
            out.contains("-->\n\nhello\n\n## Feature catalog"),
            "got {out}"
        );
    }

    #[test]
    fn a_blank_section_file_contributes_nothing() {
        let bare = render(&[], &cfg(), &[]);
        let with_blank = render(&[], &cfg(), &[section(Slot::BeforeCatalog, "\n  \n")]);
        assert_eq!(bare, with_blank);
    }

    #[test]
    fn sections_keep_their_spacing_around_bucket_sections() {
        // `before-catalog` lands before the *first* bucket section and
        // `after-catalog` after the *last*, so the slots stay well-defined
        // when the catalog is several tables rather than one.
        let out = render(
            &[
                feat("f-a", Status::Todo, "now", "v0.2.x"),
                feat("f-b", Status::Todo, "now", "v0.4"),
            ],
            &cfg_split(),
            &[
                section(Slot::BeforeCatalog, "PRE"),
                section(Slot::AfterCatalog, "POST"),
            ],
        );
        let at = |needle: &str| {
            out.find(needle)
                .unwrap_or_else(|| panic!("{needle}\n{out}"))
        };
        assert!(at("PRE") < at("## v0.2.x"), "got {out}");
        assert!(at("## v0.4") < at("POST"), "got {out}");
        assert!(at("POST") < at("## Details"), "got {out}");
        // Every heading still has its blank line above it.
        assert!(!out.contains("PRE\n## "), "got {out}");
        assert!(out.contains("\n\nPOST\n\n## Details"), "got {out}");
    }

    #[test]
    fn section_path_stays_inside_the_roadmap_root() {
        let root = Path::new("/tmp/.roadmap");
        assert!(section_path(root, "preamble.md").is_ok());
        assert!(section_path(root, "notes/intro.md").is_ok());
        assert!(section_path(root, "../escape.md").is_err());
        assert!(section_path(root, "notes/../../escape.md").is_err());
        assert!(section_path(root, "/etc/passwd").is_err());
    }

    /// `cfg()` plus a project-declared `tracked` field, rendered as a
    /// linked catalog column — the shape #22 asked for.
    fn cfg_tracked() -> Config {
        let mut config = cfg();
        config.fields.insert(
            "tracked".to_string(),
            FieldSpec {
                kind: Some(FieldKind::IssueRef),
                column: Some("Tracked".into()),
                link: Some("https://example.test/issues/{}".into()),
                required_when: Some(BTreeMap::from([(
                    "horizon".to_string(),
                    Condition::Any(vec!["now".into(), "next".into()]),
                )])),
                ..FieldSpec::default()
            },
        );
        config
    }

    fn feat_tracked(id: &str, horizon: &str, tracked: Option<toml::Value>) -> Feature {
        let mut f = feat(id, Status::Todo, horizon, "v0.2.x");
        if let Some(v) = tracked {
            f.frontmatter.extra.insert("tracked".to_string(), v);
        }
        f
    }

    #[test]
    fn a_declared_field_becomes_a_linked_catalog_column() {
        let out = render(
            &[feat_tracked("f-a", "now", Some(toml::Value::Integer(42)))],
            &cfg_tracked(),
            &[],
        );
        assert!(out.contains("| Tracked |"), "got {out}");
        assert!(
            out.contains("[42](https://example.test/issues/42)"),
            "got {out}"
        );
    }

    /// A `#`-prefixed reference links to the bare number but keeps the `#`
    /// as its link text — `#42` is how a roadmap writes it.
    #[test]
    fn a_hash_prefixed_issue_ref_links_to_the_bare_number() {
        let out = render(
            &[feat_tracked(
                "f-a",
                "now",
                Some(toml::Value::String("#42".into())),
            )],
            &cfg_tracked(),
            &[],
        );
        assert!(
            out.contains("[#42](https://example.test/issues/42)"),
            "got {out}"
        );
    }

    /// Declared columns follow ADR-0002 like every other axis: no feature
    /// carries the field → no column.
    #[test]
    fn a_declared_column_is_omitted_when_no_feature_carries_it() {
        let out = render(&[feat_tracked("f-a", "now", None)], &cfg_tracked(), &[]);
        assert!(!out.contains("Tracked"), "got {out}");
    }

    /// …and `Summary` stays last, because it is free text and a declared
    /// field is a fact about the feature like the axes before it.
    #[test]
    fn declared_columns_sit_before_summary() {
        let out = render(
            &[feat_tracked("f-a", "now", Some(toml::Value::Integer(7)))],
            &cfg_tracked(),
            &[],
        );
        let header = out
            .lines()
            .find(|l| l.starts_with("| ID |"))
            .expect("header row");
        assert!(header.ends_with("| Tracked | Summary |"), "got {header:?}");
    }

    /// A built-in axis already has a column; a `column` declaration on one
    /// must not emit the same value twice under two headers.
    #[test]
    fn a_column_declared_on_a_builtin_axis_does_not_duplicate_it() {
        let mut config = cfg();
        config.fields.insert(
            "horizon".to_string(),
            FieldSpec {
                values: vec!["now".into()],
                column: Some("When".into()),
                ..FieldSpec::default()
            },
        );
        let out = render(&[feat("f-a", Status::Todo, "now", "v0.2.x")], &config, &[]);
        assert!(out.contains("| Horizon |"), "got {out}");
        assert!(!out.contains("| When |"), "got {out}");
        // The value appears once, under the built-in header — not twice.
        assert_eq!(out.matches(" now ").count(), 1, "got {out}");
    }

    /// A free-text value would otherwise emit a link whose spaces end the
    /// URL early and whose parens close it in the wrong place.
    #[test]
    fn a_linked_value_is_percent_encoded_in_the_url() {
        let mut config = cfg();
        config.fields.insert(
            "owner".to_string(),
            FieldSpec {
                kind: Some(FieldKind::String),
                column: Some("Owner".into()),
                link: Some("https://example.test/u/{}".into()),
                ..FieldSpec::default()
            },
        );
        let mut f = feat("f-a", Status::Todo, "now", "v0.2.x");
        f.frontmatter.extra.insert(
            "owner".to_string(),
            toml::Value::String("Jane Doe (ops)".into()),
        );
        let out = render(&[f], &config, &[]);
        assert!(
            out.contains("[Jane Doe (ops)](https://example.test/u/Jane%20Doe%20%28ops%29)"),
            "got {out}"
        );
    }

    #[test]
    fn percent_encode_keeps_path_shape_and_escapes_the_rest() {
        assert_eq!(percent_encode("42"), "42");
        assert_eq!(percent_encode("a/b-c_d.e~f"), "a/b-c_d.e~f");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("(x)"), "%28x%29");
        assert_eq!(percent_encode("a|b"), "a%7Cb");
        // Multi-byte input encodes per byte, as a URL must.
        assert_eq!(percent_encode("é"), "%C3%A9");
    }

    #[test]
    fn link_text_escapes_brackets_that_would_close_it_early() {
        assert_eq!(escape_link_text("a[b]c"), r"a\[b\]c");
        assert_eq!(escape_link_text("plain"), "plain");
    }

    #[test]
    fn condition_matches_a_single_value_or_any_of_a_list() {
        let one = Condition::One("feature".into());
        assert!(one.matches(&["feature".to_string()]));
        assert!(!one.matches(&["fix".to_string()]));

        let any = Condition::Any(vec!["now".into(), "next".into()]);
        assert!(any.matches(&["next".to_string()]));
        assert!(!any.matches(&["later".to_string()]));
        // Multi-valued fields match if *any* value satisfies the condition.
        assert!(any.matches(&["later".to_string(), "now".to_string()]));
    }

    #[test]
    fn condition_describes_itself_for_error_messages() {
        assert_eq!(Condition::One("fix".into()).describe(), "\"fix\"");
        assert_eq!(
            Condition::Any(vec!["now".into(), "next".into()]).describe(),
            "\"now\" or \"next\""
        );
    }

    /// A single string and a one-element list are the same condition, so a
    /// config can move between the two forms without changing meaning.
    #[test]
    fn required_when_accepts_both_scalar_and_list_forms() {
        #[derive(Deserialize)]
        struct Probe {
            required_when: BTreeMap<String, Condition>,
        }
        let scalar: Probe = toml::from_str("required_when = { type = \"fix\" }").unwrap();
        let list: Probe = toml::from_str("required_when = { type = [\"fix\"] }").unwrap();
        assert!(scalar.required_when["type"].matches(&["fix".to_string()]));
        assert!(list.required_when["type"].matches(&["fix".to_string()]));
    }

    #[test]
    fn render_empty_catalog_keeps_identity_columns() {
        // No features → no axis has a value; only the identity columns
        // remain and the separator row matches their count.
        let out = render(&[], &cfg(), &[]);
        assert!(out.contains("| ID | Status | Summary |"));
        assert!(out.contains("|---|---|---|\n"));
    }

    #[test]
    fn render_emits_details_with_full_body_and_shipped_line() {
        let mut f = feat("f-x", Status::Done, "shipped", "v0.2.x");
        f.body = "Summary line.\n\nSecond paragraph with detail.\n".into();
        f.frontmatter.shipped = Shipped {
            version: "v0.2.0".into(),
            date: "2026-07-12".into(),
            pr: 1,
        };
        let out = render(&[f], &cfg(), &[]);
        assert!(out.contains("## Details"));
        assert!(out.contains("### <a id=\"f-x\"></a>f-x"));
        assert!(out.contains("Shipped in v0.2.0 (2026-07-12, PR #1)."));
        assert!(out.contains("Second paragraph with detail."));
    }

    #[test]
    fn render_omits_details_section_when_no_features() {
        let out = render(&[], &cfg(), &[]);
        assert!(!out.contains("## Details"));
    }

    #[test]
    fn shipped_line_variants() {
        let full = Shipped {
            version: "v1".into(),
            date: "2026-01-01".into(),
            pr: 7,
        };
        assert_eq!(
            shipped_line(&full).unwrap(),
            "Shipped in v1 (2026-01-01, PR #7)."
        );
        let bare = Shipped {
            version: "v1".into(),
            ..Shipped::default()
        };
        assert_eq!(shipped_line(&bare).unwrap(), "Shipped in v1.");
        assert!(shipped_line(&Shipped::default()).is_none());
    }

    fn unique_tmp(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("roadmark-lib-{label}-{}-{n}", std::process::id()))
    }

    #[test]
    fn write_atomic_creates_then_replaces() {
        let dir = unique_tmp("atomic-replace");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ROADMAP.md");

        write_atomic(&target, "first\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "first\n");
        write_atomic(&target, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "second\n");

        // The staging file is renamed, not left behind: the directory holds
        // exactly the destination.
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("ROADMAP.md")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A bare relative filename has an empty parent — the temp file has to
    /// land in the current directory rather than at the filesystem root.
    #[test]
    fn write_atomic_handles_bare_relative_filename() {
        let dir = unique_tmp("atomic-relative");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("out.md");
        // Resolve the sibling-directory rule without depending on the
        // process-wide cwd (tests share it): `dir/out.md` already exercises
        // the `Some(parent)` arm, so assert the fallback arm directly.
        assert_eq!(
            Path::new("ROADMAP.md")
                .parent()
                .filter(|p| !p.as_os_str().is_empty()),
            None
        );
        write_atomic(&target, "x\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "x\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The destination must survive a failed write — that is the whole
    /// reason the function exists (see #41).
    #[test]
    fn write_atomic_leaves_destination_intact_when_staging_fails() {
        let dir = unique_tmp("atomic-fail");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ROADMAP.md");
        std::fs::write(&target, "PRECIOUS\n").unwrap();

        // A destination whose parent does not exist: staging fails, so the
        // rename never runs. The real file elsewhere is untouched.
        let doomed = dir.join("missing-subdir").join("ROADMAP.md");
        assert!(write_atomic(&doomed, "new\n").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "PRECIOUS\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A published-docs checkout symlinks `ROADMAP.md` into the site tree; the
    /// shell redirect wrote through the link, and so must we. Replacing the
    /// link would leave the real document serving stale content forever.
    #[cfg(unix)]
    #[test]
    fn write_atomic_writes_through_a_symlinked_destination() {
        let dir = unique_tmp("atomic-symlink");
        std::fs::create_dir_all(dir.join("real")).unwrap();
        let real = dir.join("real").join("ROADMAP.md");
        std::fs::write(&real, "old\n").unwrap();
        let link = dir.join("ROADMAP.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomic(&link, "new\n").unwrap();

        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new\n");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The temp file is born at `0666 & !umask`; a deliberately restricted
    /// destination must not be widened by a regen.
    #[cfg(unix)]
    #[test]
    fn write_atomic_preserves_destination_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_tmp("atomic-perms");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ROADMAP.md");
        std::fs::write(&target, "old\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();

        write_atomic(&target, "new\n").unwrap();

        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_neutralises_comment_terminator_in_source_note() {
        let config = Config {
            versions: vec!["v1".into()],
            title: "T".into(),
            source_note: Some("see foo --> bar".into()),
            ..Config::default()
        };
        let out = render(&[], &config, &[]);
        // The only `-->` in the output is the banner's own closing fence.
        assert_eq!(out.matches("-->").count(), 1);
        assert!(out.contains("see foo --&gt; bar"));
    }
}
