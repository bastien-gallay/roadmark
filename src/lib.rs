//! Pure functions for the `roadmap` generator.
//!
//! Wired by `main.rs`. Kept fs-free so unit tests can pass strings
//! and snapshot the rendered output via `insta`.

pub mod add;
pub mod validate;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

/// One feature: TOML frontmatter + raw markdown body.
///
/// Body stays an unparsed `String` — a markdown parser would round-trip
/// poorly (loses author intent on edge cases), and the renderer only
/// needs the first paragraph for the catalog summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub frontmatter: Frontmatter,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Frontmatter {
    pub id: String,
    pub topic: String,
    pub status: Status,
    pub priority: Priority,
    pub target: Vec<String>,
    #[serde(default)]
    pub shipped: Shipped,
    /// Stable position within the "shipped" tier — set at flip-time
    /// (status: todo → done) so historical order survives regen.
    /// Optional; only required once the catalog includes shipped entries.
    #[serde(default)]
    pub shipped_order: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Wip,
    Todo,
    Done,
}

impl Status {
    const fn rank(self) -> u8 {
        match self {
            Self::Wip => 0,
            Self::Todo => 1,
            Self::Done => 2,
        }
    }

    const fn glyph(self) -> &'static str {
        match self {
            Self::Wip => "🚧",
            Self::Todo => "☐",
            Self::Done => "✅",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Next,
    Later,
    Speculative,
    Shipped,
}

impl Priority {
    const fn rank(self) -> u8 {
        match self {
            Self::Next => 0,
            Self::Later => 1,
            Self::Speculative => 2,
            Self::Shipped => 3,
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
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Bucket order for sorting and section emission. Earliest cycle first.
    pub versions: Vec<String>,
    /// H1 heading for the generated `ROADMAP.md`. Defaults to `"Roadmap"`.
    #[serde(default = "default_title")]
    pub title: String,
    /// Optional project-specific note appended to the generated
    /// "DO NOT EDIT" banner — e.g. a pointer to an ADR or design doc.
    #[serde(default)]
    pub source_note: Option<String>,
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
    let (toml_block, body) = split_frontmatter(src)?;
    let frontmatter: Frontmatter =
        toml::from_str(toml_block).context("invalid TOML frontmatter")?;
    Ok(Feature {
        frontmatter,
        body: body.to_string(),
    })
}

/// Sort key: target[0] (via config bucket order) → status → priority → id.
/// Unknown targets sort last; missing target arrays are an upstream
/// schema error caught at parse time.
fn sort_key<'a>(f: &'a Feature, version_index: &HashMap<&str, usize>) -> (usize, u8, u8, &'a str) {
    let target_idx = f
        .frontmatter
        .target
        .first()
        .and_then(|t| version_index.get(t.as_str()).copied())
        .unwrap_or(usize::MAX);
    (
        target_idx,
        f.frontmatter.status.rank(),
        f.frontmatter.priority.rank(),
        &f.frontmatter.id,
    )
}

pub fn sort_features(features: &mut [Feature], config: &Config) {
    let version_index: HashMap<&str, usize> = config
        .versions
        .iter()
        .enumerate()
        .map(|(i, v)| (v.as_str(), i))
        .collect();
    features.sort_by(|a, b| {
        let ka = sort_key(a, &version_index);
        let kb = sort_key(b, &version_index);
        ka.cmp(&kb).then_with(|| {
            // Stable shipped_order tiebreak among Done items.
            match (a.frontmatter.shipped_order, b.frontmatter.shipped_order) {
                (Some(x), Some(y)) => x.cmp(&y),
                _ => Ordering::Equal,
            }
        })
    });
}

/// First non-empty line of the body, used as the catalog summary.
fn summary(body: &str) -> &str {
    body.lines()
        .find(|l| !l.trim().is_empty())
        .map(str::trim)
        .unwrap_or("")
}

/// HTML id for the anchor: lowercase the feature id.
/// Matches GitHub's `<a id="f46">` / `<a id="f-foo">` convention.
fn anchor_id(id: &str) -> String {
    id.to_lowercase()
}

pub fn render(features: &[Feature], config: &Config) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(8 * 1024);
    let _ = writeln!(out, "# {}\n", config.title);
    out.push_str(
        "<!-- DO NOT EDIT — generated by `roadmap generate`. Source of truth: `.roadmap/`.",
    );
    if let Some(note) = &config.source_note {
        let _ = write!(out, " {note}");
    }
    out.push_str(" -->\n\n");
    out.push_str("## Feature catalog\n\n");
    out.push_str("| ID | Topic | Status | Target | Summary |\n");
    out.push_str("|---|---|---|---|---|\n");
    for f in features {
        let fm = &f.frontmatter;
        let aid = anchor_id(&fm.id);
        let target = fm.target.join(" → ");
        let _ = writeln!(
            out,
            "| <a id=\"{aid}\"></a>[{id}](#{aid}) | {topic} | {status} | {target} | {summary} |",
            id = fm.id,
            topic = fm.topic,
            status = fm.status.glyph(),
            target = target,
            summary = summary(&f.body),
        );
    }
    out
}

/// Read all `.roadmap/features/*.md` files under `root`, parse each,
/// and return them in load order (caller sorts).
pub fn load_features(root: &Path) -> Result<Vec<Feature>> {
    let dir = root.join("features");
    if !dir.is_dir() {
        bail!("expected directory: {}", dir.display());
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(&dir)
        .min_depth(1)
        .max_depth(1)
        .sort_by_file_name()
    {
        let entry = entry.context("walking features dir")?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "md") {
            let src = std::fs::read_to_string(entry.path())
                .with_context(|| format!("reading {}", entry.path().display()))?;
            let feature = parse_feature(&src)
                .with_context(|| format!("parsing {}", entry.path().display()))?;
            out.push(feature);
        }
    }
    Ok(out)
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
        Config {
            versions: vec!["v0.2.x".into(), "v0.3".into(), "v0.4".into()],
            title: "Roadmap".into(),
            source_note: None,
        }
    }

    fn feat(id: &str, status: Status, priority: Priority, target: &str) -> Feature {
        Feature {
            frontmatter: Frontmatter {
                id: id.into(),
                topic: "Test".into(),
                status,
                priority,
                target: vec![target.into()],
                shipped: Shipped::default(),
                shipped_order: None,
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
topic = \"Architecture\"\n\
status = \"todo\"\n\
priority = \"next\"\n\
target = [\"v0.2.x\"]\n\
+++\n\nThe summary.\n";
        let f = parse_feature(src).unwrap();
        assert_eq!(f.frontmatter.id, "F-foo");
        assert_eq!(f.frontmatter.status, Status::Todo);
        assert_eq!(f.body, "The summary.\n");
    }

    #[test]
    fn sort_target_then_status_then_priority_then_id() {
        let mut fs = vec![
            feat("f-z", Status::Todo, Priority::Next, "v0.3"),
            feat("f-a", Status::Todo, Priority::Next, "v0.2.x"),
            feat("f-b", Status::Wip, Priority::Next, "v0.2.x"),
            feat("f-c", Status::Todo, Priority::Later, "v0.2.x"),
        ];
        sort_features(&mut fs, &cfg());
        let ids: Vec<&str> = fs.iter().map(|f| f.frontmatter.id.as_str()).collect();
        assert_eq!(ids, vec!["f-b", "f-a", "f-c", "f-z"]);
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
    }

    #[test]
    fn render_uses_title_and_source_note() {
        let config = Config {
            versions: vec!["v1".into()],
            title: "My Project — Roadmap".into(),
            source_note: Some("See docs/adr.".into()),
        };
        let out = render(&[], &config);
        assert!(out.starts_with("# My Project — Roadmap\n\n"));
        assert!(out.contains("generated by `roadmap generate`"));
        assert!(out.contains("Source of truth: `.roadmap/`. See docs/adr. -->"));
    }
}
