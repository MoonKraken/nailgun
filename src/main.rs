mod frontmatter;
mod progress;
mod provider;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::Parser;
use tokio::task::JoinSet;

use frontmatter::Config;
use progress::Progress;
use provider::ImageProvider;

#[derive(Parser)]
#[command(version, about = "Generate YouTube thumbnails from script frontmatter")]
struct Args {
    /// Path to the script markdown file
    script: PathBuf,
    /// Number of thumbnails to generate per style/concept combination
    #[arg(default_value_t = 1)]
    count: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.count == 0 {
        bail!("count must be at least 1");
    }

    let content = std::fs::read_to_string(&args.script)
        .with_context(|| format!("reading {}", args.script.display()))?;
    let config = Config::from_script(&content)?;

    let epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let out_dir = args
        .script
        .parent()
        .unwrap_or(Path::new("."))
        .join("nailgun");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("creating {}", out_dir.display()))?;

    let provider = provider::default_provider()?;
    eprintln!("provider: {}", provider.name());

    // `None` represents the no-styles case: concepts are generated without a style line.
    let styles: Vec<Option<String>> = if config.styles.is_empty() {
        vec![None]
    } else {
        config.styles.keys().cloned().map(Some).collect()
    };
    let style_labels: Vec<String> = styles
        .iter()
        .map(|s| s.clone().unwrap_or_else(|| "default".into()))
        .collect();
    let concept_labels: Vec<String> = config.concepts.keys().cloned().collect();
    let progress = Arc::new(Progress::new(
        style_labels,
        concept_labels.clone(),
        args.count,
    ));

    let mut tasks = JoinSet::new();
    for (style_idx, style) in styles.iter().enumerate() {
        for (concept_idx, (concept_key, concept_text)) in config.concepts.iter().enumerate() {
            let style_text = style.as_ref().map(|k| config.styles[k].as_str());
            let prompt = build_prompt(concept_text, style_text);
            for n in 1..=args.count {
                let filename = output_filename(epoch, style.as_deref(), concept_key, n);
                let base = out_dir.join(filename);
                let provider = Arc::clone(&provider);
                let progress = Arc::clone(&progress);
                let prompt = prompt.clone();
                tasks.spawn(async move {
                    let result = generate_one(&*provider, &prompt, &base).await;
                    progress.record(concept_idx, style_idx, &result);
                });
            }
        }
    }
    tasks.join_all().await;

    let errors = progress.errors();
    if !errors.is_empty() {
        for error in &errors {
            eprintln!("error: {error}");
        }
        bail!("{} generation(s) failed", errors.len());
    }
    println!(
        "wrote {} thumbnail(s) to {}",
        styles.len() * concept_labels.len() * args.count,
        out_dir.display()
    );
    Ok(())
}

async fn generate_one(provider: &dyn ImageProvider, prompt: &str, base: &Path) -> Result<()> {
    let image = provider.generate(prompt).await?;
    let path = base.with_extension(image.extension);
    std::fs::write(&path, image.bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn build_prompt(concept: &str, style: Option<&str>) -> String {
    let mut prompt = format!("YouTube Thumbnail. 16:9 aspect ratio. {concept}");
    if let Some(style) = style {
        prompt.push_str(&format!("\nStyle: {style}"));
    }
    prompt
}

/// Output path base, without extension. The extension is appended after
/// generation, matching the format the provider actually returned.
fn output_filename(epoch: u64, style: Option<&str>, concept: &str, n: usize) -> String {
    let concept = sanitize(concept);
    match style {
        Some(style) => format!("{epoch}_{}_{concept}_{n}", sanitize(style)),
        None => format!("{epoch}_{concept}_{n}"),
    }
}

fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_with_style() {
        assert_eq!(
            build_prompt("A cog", Some("Lo-fi sketch")),
            "YouTube Thumbnail. 16:9 aspect ratio. A cog\nStyle: Lo-fi sketch"
        );
    }

    #[test]
    fn prompt_without_style_omits_style_line() {
        assert_eq!(
            build_prompt("A cog", None),
            "YouTube Thumbnail. 16:9 aspect ratio. A cog"
        );
    }

    #[test]
    fn filenames() {
        assert_eq!(
            output_filename(123, Some("cyberpunk"), "logo", 1),
            "123_cyberpunk_logo_1"
        );
        assert_eq!(output_filename(123, None, "logo", 2), "123_logo_2");
        assert_eq!(
            output_filename(123, Some("dark mode"), "my logo", 1),
            "123_dark_mode_my_logo_1"
        );
    }
}
