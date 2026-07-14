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
    let runs = discover(dir);
    let count = runs.len();
    let mut total_steps: u64 = 0;
    let mut verified = 0usize;
    let mut open_intents = 0usize;
    let mut rows = String::new();

    for run in &runs {
        match open_journal(dir, run, key) {
            Ok(j) => {
                total_steps += j.len();
                let chain_ok = j.verify().is_ok();
                if chain_ok {
                    verified += 1;
                }
                let d = j.dangling_intents().len();
                open_intents += d;
                let chain = if chain_ok {
                    "<span class=\"pill\">verified</span>".to_string()
                } else {
                    "<span class=\"pill alarm\">chain broken</span>".to_string()
                };
                let effects = if d > 0 {
                    format!("<span class=\"pill alarm\">{d} open intent(s)</span>")
                } else {
                    String::new()
                };
                let head = j
                    .head()
                    .map(|h| hex(&h)[..10].to_string())
                    .unwrap_or_else(|| "—".into());
                rows.push_str(&format!(
                    "<a class=\"card runrow\" href=\"/run/{run}\">\
                       <span class=\"name\">{run}</span>\
                       <span class=\"rmeta\">\
                         <span class=\"pill\">{steps} steps</span>\
                         {chain}{effects}\
                         <span class=\"pill mono\">{head}</span>\
                       </span>\
                       <span class=\"arrow\">→</span>\
                     </a>\n",
                    run = export::esc(run),
                    steps = j.len(),
                ));
            }
            Err(e) => {
                rows.push_str(&format!(
                    "<div class=\"card runrow\"><span class=\"name\">{}</span>\
                     <span class=\"rmeta\"><span class=\"pill alarm\">{}</span></span></div>\n",
                    export::esc(run),
                    export::esc(&e),
                ));
            }
        }
    }
    if rows.is_empty() {
        rows = "<div class=\"card empty\">No journals here yet — point an agent's \
                runtime at this directory and its runs will appear as they record.</div>"
            .to_string();
    }

    let effect_stat = if open_intents > 0 {
        format!(
            "<div class=\"stat\"><div class=\"n alarm\">{open_intents} open</div>\
             <div class=\"l\">Effect intents</div></div>"
        )
    } else {
        String::new()
    };

    let body = format!(
        r#"<div class="wrap">
  {mast}
  <div class="hero">
    <div class="kicker">Console</div>
    <h1>Every run,<br>on the record.</h1>
    <div class="lede">Each journal below is an append-only, hash-chained account
    of what one agent actually did. Refresh to follow live runs.</div>
  </div>
  <div class="stats">
    <div class="stat"><div class="n">{count}</div><div class="l">Journaled runs</div></div>
    <div class="stat"><div class="n">{total_steps}</div><div class="l">Recorded steps</div></div>
    <div class="stat"><div class="n">{verified} / {count}</div><div class="l">Chains verified</div></div>
    {effect_stat}
  </div>
  <div class="runlist">
  {rows}
  </div>
</div>"#,
        mast = export::masthead("Console"),
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
