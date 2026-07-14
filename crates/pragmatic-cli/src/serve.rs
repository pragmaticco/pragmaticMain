//! `pragmatic serve` — the replay console as a local web app, zero
//! dependencies. Binds loopback only; every request re-reads the journal
//! directory, so the console live-follows runs as they record.
//!
//! Routes:
//! - `GET /`          run index (id, entries, chain status, dangling intents)
//! - `GET /run/<id>`  the full replay-console timeline for one run
//! - `GET /healthz`   liveness probe

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use pragmatic::journal::Journal;
use pragmatic::sha256::hex;

use crate::export;

fn open_journal(dir: &Path, run: &str, key: Option<&str>) -> Result<Journal, String> {
    let path = dir.join(format!("{run}.journal"));
    let loaded = match key {
        Some(k) => Journal::open_keyed(&path, k.as_bytes()),
        None => Journal::open(&path),
    };
    loaded.map(|l| l.journal).map_err(|e| e.to_string())
}

fn discover(dir: &Path) -> Vec<String> {
    let mut runs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("journal") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    runs.push(stem.to_string());
                }
            }
        }
    }
    runs.sort();
    runs
}

fn index_page(dir: &Path, key: Option<&str>) -> String {
    let mut rows = String::new();
    let runs = discover(dir);
    let count = runs.len();
    for run in runs {
        let (entries, chain, head, effects) = match open_journal(dir, &run, key) {
            Ok(j) => {
                let chain = if j.verify().is_ok() {
                    "<span class=\"state\">verified</span>".to_string()
                } else {
                    "<span class=\"state bad\">chain broken</span>".to_string()
                };
                let head = j
                    .head()
                    .map(|h| hex(&h)[..12].to_string())
                    .unwrap_or_else(|| "—".into());
                let d = j.dangling_intents().len();
                let effects = if d > 0 {
                    format!("<span class=\"state bad\">{d} open intent(s)</span>")
                } else {
                    "<span class=\"state dim\">settled</span>".to_string()
                };
                (j.len().to_string(), chain, head, effects)
            }
            Err(e) => (
                "—".into(),
                format!("<span class=\"state bad\">{}</span>", export::esc(&e)),
                "—".into(),
                String::new(),
            ),
        };
        rows.push_str(&format!(
            "<tr><td class=\"run\"><a href=\"/run/{run}\">{run}</a></td>\
             <td class=\"state dim\">{entries} steps</td>\
             <td>{chain}</td><td>{effects}</td>\
             <td class=\"state dim mono\">{head}</td></tr>",
            run = export::esc(&run),
        ));
    }
    if rows.is_empty() {
        rows = "<tr><td colspan=\"5\"><span class=\"state dim\">no journals here yet — \
                point an agent's runtime at this directory</span></td></tr>"
            .to_string();
    }
    let body = format!(
        r#"<div class="wrap">
  <div class="brand">{mark}<a href="/">Pragmatic</a></div>
  <div class="kicker">Console</div>
  <h1>Every run,<br>on the record.</h1>
  <div class="meta">
    <div><b>{count}</b> journaled runs in this directory</div>
    <div>refresh to follow live runs</div>
  </div>
  <table>
    <tr><th>Run</th><th>Steps</th><th>Chain</th><th>Effects</th><th>Head</th></tr>
    {rows}
  </table>
</div>"#,
        mark = export::MARK,
    );
    export::page("pragmatic console", &body)
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}; charset=utf-8\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn handle(mut stream: TcpStream, dir: &Path, key: Option<&str>) {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");

    match path {
        "/healthz" => respond(&mut stream, "200 OK", "text/plain", "ok"),
        "/" => respond(&mut stream, "200 OK", "text/html", &index_page(dir, key)),
        p if p.starts_with("/run/") => {
            // Run ids are filesystem-safe by construction; reject anything else.
            let run: String = p["/run/".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            match open_journal(dir, &run, key) {
                Ok(journal) => respond(
                    &mut stream,
                    "200 OK",
                    "text/html",
                    &export::render(&run, &journal),
                ),
                Err(e) => respond(
                    &mut stream,
                    "404 Not Found",
                    "text/plain",
                    &format!("no such run '{run}': {e}"),
                ),
            }
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

/// Serve the console until killed. Loopback-only by design: the console
/// exposes prompts and outcomes, so putting it on a network is an explicit
/// reverse-proxy decision, not a default.
pub fn serve(dir: PathBuf, key: Option<String>, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot bind 127.0.0.1:{port}: {e}"))?;
    println!(
        "pragmatic console serving {} on http://127.0.0.1:{port}",
        dir.display()
    );
    println!("^C to stop");
    for stream in listener.incoming().flatten() {
        let dir = dir.clone();
        let key = key.clone();
        std::thread::spawn(move || handle(stream, &dir, key.as_deref()));
    }
    Ok(())
}
