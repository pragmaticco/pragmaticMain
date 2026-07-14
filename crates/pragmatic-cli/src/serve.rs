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

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

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
    for run in discover(dir) {
        let (entries, chain, head, dangling) = match open_journal(dir, &run, key) {
            Ok(j) => {
                let chain = if j.verify().is_ok() {
                    "<span class=\"ok\">verified</span>".to_string()
                } else {
                    "<span class=\"bad\">BROKEN</span>".to_string()
                };
                let head = j
                    .head()
                    .map(|h| hex(&h)[..16].to_string())
                    .unwrap_or_else(|| "—".into());
                let d = j.dangling_intents().len();
                let dangling = if d > 0 {
                    format!("<span class=\"warn\">{d} dangling</span>")
                } else {
                    "<span class=\"dim\">—</span>".to_string()
                };
                (j.len().to_string(), chain, head, dangling)
            }
            Err(e) => (
                "?".into(),
                format!("<span class=\"bad\">{}</span>", esc(&e)),
                "—".into(),
                String::new(),
            ),
        };
        rows.push_str(&format!(
            "<tr><td><a href=\"/run/{run}\">{run}</a></td><td>{entries}</td>\
             <td>{chain}</td><td>{dangling}</td><td class=\"mono dim\">{head}</td></tr>",
            run = esc(&run),
        ));
    }
    if rows.is_empty() {
        rows = "<tr><td colspan=\"5\" class=\"dim\">no journals here yet — \
                point an agent's Runtime at this directory</td></tr>"
            .to_string();
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>pragmatic console</title>
<style>
 :root {{ --bg:#0b0e14; --panel:#11151f; --line:#1e2530; --text:#d7dde8;
          --dim:#7b8496; --accent:#7aa2f7; }}
 body {{ margin:0; background:var(--bg); color:var(--text);
        font:15px/1.5 ui-sans-serif,system-ui,sans-serif; }}
 .wrap {{ max-width:880px; margin:0 auto; padding:40px 20px; }}
 h1 {{ font-size:20px; }} h1 span {{ color:var(--accent); }}
 .sub {{ color:var(--dim); font-size:13px; margin-bottom:24px; }}
 table {{ width:100%; border-collapse:collapse; background:var(--panel);
         border:1px solid var(--line); border-radius:8px; overflow:hidden; }}
 th,td {{ text-align:left; padding:10px 14px; border-bottom:1px solid var(--line);
         font-size:14px; }}
 th {{ color:var(--dim); font-size:11px; text-transform:uppercase; letter-spacing:.06em; }}
 a {{ color:var(--accent); text-decoration:none; }}
 .ok {{ color:#9ece6a; }} .bad {{ color:#f7768e; }} .warn {{ color:#e0af68; }}
 .dim {{ color:var(--dim); }} .mono {{ font-family:ui-monospace,monospace; font-size:12px; }}
</style></head><body><div class="wrap">
<h1><span>pragmatic</span> console</h1>
<div class="sub">journals in this directory · refresh to follow live runs</div>
<table><tr><th>run</th><th>entries</th><th>chain</th><th>effects</th><th>head</th></tr>
{rows}</table>
</div></body></html>"#
    )
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
