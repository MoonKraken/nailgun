//! Live integration test: exactly ONE real generation against the real API
//! (~$0.13 at current nano banana pro 1K pricing). This is the only test
//! allowed to make live requests. Skipped when no API key is present.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_MAGIC: &[u8] = b"\xff\xd8\xff";

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nailgun-{tag}-{}-{nanos}", std::process::id()))
}

#[test]
fn one_live_generation() {
    if std::env::var("GEMINI_API_KEY").is_err() && std::env::var("GOOGLE_API_KEY").is_err() {
        eprintln!("skipping live test: GEMINI_API_KEY/GOOGLE_API_KEY not set");
        return;
    }

    let dir = temp_dir("live");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("script.md");
    // One concept, no styles => exactly one generation.
    std::fs::write(
        &script,
        "---\nnailgun:\n    concepts:\n        dot: A single red dot centered on a plain white background\n---\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nailgun"))
        .arg(&script)
        .env_remove("NAILGUN_API_BASE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let files: Vec<_> = std::fs::read_dir(dir.join("nailgun"))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let path = files[0].path();
    let bytes = std::fs::read(&path).unwrap();
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => assert!(bytes.starts_with(PNG_MAGIC)),
        Some("jpg") => assert!(bytes.starts_with(JPEG_MAGIC)),
        other => panic!("unexpected output extension: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}
