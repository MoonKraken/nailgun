//! Offline pipeline test: runs the nailgun binary end-to-end against a mock
//! Gemini API on localhost. No live requests, no cost.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// 1x1 red pixel PNG
const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nailgun-{tag}-{}-{nanos}", std::process::id()))
}

fn serve_json_once(body: String) -> u16 {
    serve_json_once_capturing(body).0
}

/// Like [`serve_json_once`], but also yields the raw request bytes received.
fn serve_json_once_capturing(body: String) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 8192];
        while !complete_request(&request) {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        request
    });
    (port, handle)
}

fn write_script(tag: &str) -> (PathBuf, PathBuf) {
    let dir = temp_dir(tag);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("script.md");
    std::fs::write(
        &script,
        "---\nnailgun:\n    concepts:\n        dot: A red dot\n---\n",
    )
    .unwrap();
    (dir, script)
}

fn assert_single_png(dir: &Path) {
    let files: Vec<_> = std::fs::read_dir(dir.join("nailgun"))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let path = files[0].path();
    assert_eq!(path.extension().unwrap(), "png");
    assert!(std::fs::read(&path).unwrap().starts_with(PNG_MAGIC));
}

#[test]
fn full_pipeline_against_mock() {
    // Mock Gemini API: responds to the single generateContent POST with a PNG part.
    let body = format!(
        r#"{{"candidates":[{{"content":{{"parts":[{{"inlineData":{{"mimeType":"image/png","data":"{PNG_B64}"}}}}]}}}}]}}"#
    );
    let port = serve_json_once(body);
    let (dir, script) = write_script("pipeline");

    let output = Command::new(env!("CARGO_BIN_EXE_nailgun"))
        .arg(&script)
        .env("GEMINI_API_KEY", "mock-key")
        .env("NAILGUN_API_BASE", format!("http://127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_single_png(&dir);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn nano_banana_2_1_pipeline_against_mock() {
    let body = format!(
        r#"{{"candidates":[{{"content":{{"parts":[{{"inlineData":{{"mimeType":"image/png","data":"{PNG_B64}"}}}}]}}}}]}}"#
    );
    let (port, request) = serve_json_once_capturing(body);
    let (dir, script) = write_script("pipeline-nb21");

    let output = Command::new(env!("CARGO_BIN_EXE_nailgun"))
        .args(["--provider", "nano-banana-2.1"])
        .arg(&script)
        .env("GEMINI_API_KEY", "mock-key")
        .env("NAILGUN_API_BASE", format!("http://127.0.0.1:{port}"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("provider: nano-banana-2.1"), "stderr: {stderr}");

    let request = String::from_utf8_lossy(&request.join().unwrap()).into_owned();
    assert!(
        request.starts_with("POST /v1beta/models/gemini-nano-banana-2.1:generateContent "),
        "request: {request}"
    );

    assert_single_png(&dir);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn gpt_image_2_pipeline_against_mock() {
    let body =
        format!(r#"{{"created":0,"output_format":"png","data":[{{"b64_json":"{PNG_B64}"}}]}}"#);
    let port = serve_json_once(body);
    let (dir, script) = write_script("pipeline-gpt");

    let output = Command::new(env!("CARGO_BIN_EXE_nailgun"))
        .args(["--provider", "gpt-image-2"])
        .arg(&script)
        .env("OPENAI_API_KEY", "mock-key")
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY")
        .env("NAILGUN_API_BASE", format!("http://127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("provider: gpt-image-2"), "stderr: {stderr}");

    assert_single_png(&dir);
    std::fs::remove_dir_all(&dir).ok();
}

fn complete_request(request: &[u8]) -> bool {
    let Some(header_end) = find_subslice(request, b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    request.len() >= header_end + 4 + content_length
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
