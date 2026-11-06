# Overview
nailgun uses image generation providers to generate YouTube thumbnails based on styles and concepts in the frontmatter of a script markdown file.

All generations should be done concurrently.
# Input
The first argument will be the script path. All output will be relative to the script path.

Second argument (optional, defaults is 1) is the number of thumbnails to generate for each cross product of styles and concepts. For example, if 1, two styles and two concepts would yield 4 total thumbnails.

`--provider` (optional) selects the image generation backend. Defaults to `nano-banana-pro`. Supported values: `nano-banana-pro` (alias `nano-banana`), `nano-banana-2.1`, `gpt-image-2`.
# Frontmatter
"concepts" is required. Styles is optional.

---
nailgun:
    styles:
       cyberpunk: Purple and Blue LEDs 
       sketch: Lo-fi sketch
    concepts:
        logo: Rust logo cog but with a lambda inside instead of an 'R'
        datacenter: Datacenter with a Rust logo on all of the racked servers
        
---

# Prompt
The prompt should be the following, with the appropriate values interpolated: 

"YouTube Thumbnail. 16:9 aspect ratio. {concept text}
Style: {style text}"

Omit the style line if no styles were provided in the frontmatter.
# Progress Display
Show a table where each column represents a style. Each row represents a concept.
# Output
The Thumbnails with the following directory structure. Epoch time should be the epoch time at the beginning of the program, ie all files yielded from the same run should have the same epoch time. The file extension matches the image format the provider actually returns (the Gemini API does not support requesting an output format; nano banana pro returns JPEG). Given an epoch time of 123 and the above frontmatter, the resulting file structure would be

script.md
nailgun/123_cyberpunk_logo_1.jpg
nailgun/123_cyberpunk_datacenter_1.jpg
nailgun/123_sketch_logo_1.jpg
nailgun/123_sketch_datacenter_1.jpg

# Image Generation
Interface should be generic such that new providers can be added easily. Supported providers: Google nano banana pro (default), Google Nano Banana 2.1, and OpenAI GPT Image 2.

# Development
Rust binary. `cargo build` / `cargo test` / `cargo clippy --all-targets` must stay clean.

- `src/main.rs` — CLI args, orchestration, prompt/filename building, concurrent generation
- `src/frontmatter.rs` — `nailgun:` frontmatter parsing (concepts required, styles optional)
- `src/provider.rs` — `ImageProvider` trait and `ProviderKind`; add providers by implementing the trait and selecting them in `from_kind`. `generate` returns image bytes plus the extension matching the format the backend sent (no transcoding). Nano banana pro (`gemini-3-pro-image`) reads `GEMINI_API_KEY` (or `GOOGLE_API_KEY`). Nano Banana 2.1 (`gemini-nano-banana-2.1`) shares the same Gemini implementation and key, differing only by model id. GPT Image 2 (`gpt-image-2`) reads `OPENAI_API_KEY` and requests 16:9 (`2048x1152`). `NAILGUN_API_BASE` overrides the API base URL for tests/mocks
- `src/progress.rs` — live table (styles as columns, concepts as rows); plain event lines when stdout is not a TTY

# Testing
Unit tests (`src/**`) must never make live requests. Live API coverage is limited to `tests/live.rs`, which makes exactly ONE generation (~$0.13) against the default nano banana pro provider and skips when no API key is set — keep its volume that low so `cargo test` stays cheap. `tests/pipeline.rs` covers the full binary against a localhost mock at zero cost (default provider, `--provider nano-banana-2.1`, and `--provider gpt-image-2`).
