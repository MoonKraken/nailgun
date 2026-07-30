use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Deserialize)]
struct Frontmatter {
    nailgun: NailgunSection,
}

#[derive(Deserialize)]
struct NailgunSection {
    #[serde(default)]
    styles: BTreeMap<String, String>,
    #[serde(default)]
    concepts: BTreeMap<String, String>,
}

/// Styles and concepts parsed from a script's `nailgun:` frontmatter.
pub struct Config {
    pub styles: BTreeMap<String, String>,
    pub concepts: BTreeMap<String, String>,
}

impl Config {
    pub fn from_script(content: &str) -> Result<Self> {
        let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
        let parsed = matter
            .parse_with_struct::<Frontmatter>(content)
            .context("no valid `nailgun:` frontmatter found in script")?;
        let section = parsed.data.nailgun;
        if section.concepts.is_empty() {
            bail!("frontmatter must define at least one concept under `nailgun.concepts`");
        }
        Ok(Self {
            styles: section.styles,
            concepts: section.concepts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"---
nailgun:
    styles:
       cyberpunk: Purple and Blue LEDs
       sketch: Lo-fi sketch
    concepts:
        logo: Rust logo cog but with a lambda inside instead of an 'R'
        datacenter: Datacenter with a Rust logo on all of the racked servers
---

# Script body
"#;

    #[test]
    fn parses_agents_md_example() {
        let config = Config::from_script(EXAMPLE).unwrap();
        assert_eq!(config.styles.len(), 2);
        assert_eq!(config.styles["cyberpunk"], "Purple and Blue LEDs");
        assert_eq!(config.styles["sketch"], "Lo-fi sketch");
        assert_eq!(config.concepts.len(), 2);
        assert_eq!(
            config.concepts["logo"],
            "Rust logo cog but with a lambda inside instead of an 'R'"
        );
    }

    #[test]
    fn styles_are_optional() {
        let script = "---\nnailgun:\n    concepts:\n        logo: A cog\n---\nbody\n";
        let config = Config::from_script(script).unwrap();
        assert!(config.styles.is_empty());
        assert_eq!(config.concepts.len(), 1);
    }

    #[test]
    fn concepts_are_required() {
        let script = "---\nnailgun:\n    styles:\n        sketch: Lo-fi\n---\nbody\n";
        assert!(Config::from_script(script).is_err());
    }

    #[test]
    fn missing_frontmatter_is_an_error() {
        assert!(Config::from_script("# just a script\n").is_err());
    }
}
