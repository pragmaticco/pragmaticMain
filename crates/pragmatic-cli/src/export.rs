//! Self-contained HTML replay console: one run's journal as a browsable
//! timeline. No external assets, opens anywhere, safe to attach to an
//! incident ticket.
//!
//! Design follows the Pragmatic brand: cream and ink, editorial serif
//! display, letterspaced caps for labels, hairline rules — no chrome.

use pragmatic::journal::{Event, Journal};
use pragmatic::sha256::hex;

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The Pragmatic mark: a faceted clover cross, inline SVG.
pub const MARK: &str = r#"<svg class="mark" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><path fill="currentColor" d="M38 38 L32 12 L40 4 L60 4 L68 12 L62 38 L88 32 L96 40 L96 60 L88 68 L62 62 L68 88 L60 96 L40 96 L32 88 L38 62 L12 68 L4 60 L4 40 L12 32 Z"/></svg>"#;

/// Shared stylesheet for the console pages (index and run timeline).
pub const STYLE: &str = r#"
  :root {
    --paper: #f2efe9; --panel: #eceade; --ink: #1d1b16; --muted: #8a8477;
    --hair: #d8d3c4; --serif: 'Iowan Old Style', 'Palatino Linotype', Palatino,
    Georgia, 'Times New Roman', serif;
    --sans: ui-sans-serif, system-ui, 'Helvetica Neue', Arial, sans-serif;
    --mono: ui-monospace, 'SF Mono', Menlo, monospace;
  }
  * { box-sizing: border-box; margin: 0; }
  body { background: var(--paper); color: var(--ink);
         font: 15px/1.6 var(--sans); -webkit-font-smoothing: antialiased; }
  .wrap { max-width: 960px; margin: 0 auto; padding: 56px 32px 0; }
  .brand { display: flex; align-items: center; gap: 12px; margin-bottom: 64px; }
  .mark { width: 22px; height: 22px; color: var(--ink); }
  .brand a { font: italic 20px/1 var(--serif); color: var(--ink);
             text-decoration: none; letter-spacing: .01em; }
  .kicker { font: 600 11px/1 var(--sans); letter-spacing: .22em;
            text-transform: uppercase; color: var(--muted); margin-bottom: 18px; }
  h1 { font: italic 400 clamp(34px, 5vw, 52px)/1.15 var(--serif);
       letter-spacing: -0.01em; margin-bottom: 20px; }
  h1 .id { white-space: nowrap; }
  .meta { display: flex; flex-wrap: wrap; gap: 10px 28px; padding: 18px 0;
          border-top: 1px solid var(--ink); border-bottom: 1px solid var(--hair);
          margin-bottom: 8px; }
  .meta div { font: 600 11px/1.5 var(--sans); letter-spacing: .18em;
              text-transform: uppercase; color: var(--muted); }
  .meta b { color: var(--ink); font-weight: 600; }
  .meta .alarm { color: var(--ink); border-bottom: 2px solid var(--ink); }
  .mono { font-family: var(--mono); text-transform: none; letter-spacing: .02em; }

  .entry { display: grid; grid-template-columns: 72px 1fr auto;
           gap: 4px 28px; padding: 30px 0 34px;
           border-bottom: 1px solid var(--hair); }
  .num { font: italic 400 26px/1 var(--serif); color: var(--muted);
         padding-top: 2px; }
  .entry-head { grid-column: 2; }
  .kind { font: 600 11px/1 var(--sans); letter-spacing: .2em;
          text-transform: uppercase; color: var(--ink); }
  .kind.quiet { color: var(--muted); }
  .title { font: italic 400 19px/1.4 var(--serif); margin-top: 6px; }
  .hash { grid-column: 3; font: 11px/1 var(--mono); color: var(--muted);
          padding-top: 3px; }
  .body { grid-column: 2 / -1; margin-top: 14px; }
  .lbl { font: 600 10px/1 var(--sans); letter-spacing: .2em;
         text-transform: uppercase; color: var(--muted); margin: 14px 0 6px; }
  pre { background: var(--panel); border: 1px solid var(--hair);
        padding: 12px 16px; white-space: pre-wrap; word-break: break-word;
        font: 13px/1.6 var(--mono); color: var(--ink); }
  .note { grid-column: 2 / -1; margin-top: 12px; font: italic 14px/1.5 var(--serif);
          color: var(--muted); }

  table { width: 100%; border-collapse: collapse; }
  th { font: 600 11px/1 var(--sans); letter-spacing: .2em; text-transform: uppercase;
       color: var(--muted); text-align: left; padding: 0 18px 14px 0;
       border-bottom: 1px solid var(--ink); }
  td { padding: 20px 18px 20px 0; border-bottom: 1px solid var(--hair);
       vertical-align: baseline; white-space: nowrap; }
  td.run a { font: italic 400 21px/1.2 var(--serif); color: var(--ink);
             text-decoration: none; }
  @media (max-width: 720px) { td, th { white-space: normal; } }
  td.run a:hover { border-bottom: 1px solid var(--ink); }
  .state { font: 600 11px/1.5 var(--sans); letter-spacing: .16em;
           text-transform: uppercase; color: var(--muted); }
  .state.bad { color: var(--ink); border-bottom: 2px solid var(--ink); }
  .dim { color: var(--muted); }

  footer { background: var(--ink); color: var(--paper); margin-top: 96px; }
  footer .inner { max-width: 960px; margin: 0 auto; padding: 56px 32px;
                  display: flex; justify-content: space-between;
                  align-items: baseline; flex-wrap: wrap; gap: 16px; }
  footer .line { font: italic 400 22px/1.3 var(--serif); }
  footer .credit { font: 600 10px/1 var(--sans); letter-spacing: .2em;
                   text-transform: uppercase; color: #8a8477; }
  footer .credit a { color: var(--paper); text-decoration: none; }
"#;

pub fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>{STYLE}</style>
</head>
<body>
{body}
<footer><div class="inner">
  <div class="line">Crash. Recover. Replay, exactly.</div>
  <div class="credit"><a href="https://github.com/pragmaticco/pragmaticMain">Pragmatic</a> · durable execution for agents</div>
</div></footer>
</body>
</html>
"#
    )
}

struct Row {
    kind: &'static str,
    quiet: bool,
    title: String,
    body: String,
    note: String,
}

fn row(event: &Event) -> Row {
    match event {
        Event::Program { name, hash } => Row {
            kind: "program",
            quiet: true,
            title: format!("{} enters the record", esc(name)),
            body: String::new(),
            note: format!(
                "source hash {} — replay refuses a changed program",
                esc(hash)
            ),
        },
        Event::OracleDraw {
            prompt,
            outcome,
            provenance,
        } => Row {
            kind: "oracle draw",
            quiet: false,
            title: "One call to the model, journaled once.".to_string(),
            body: format!(
                "<div class=\"lbl\">Prompt</div><pre>{}</pre>\
                 <div class=\"lbl\">Realized outcome</div><pre>{}</pre>",
                esc(&prompt.as_str()),
                esc(&outcome.as_str())
            ),
            note: esc(provenance),
        },
        Event::ChannelRecv { channel, value } => Row {
            kind: "receive",
            quiet: false,
            title: format!("Received on {}", esc(channel)),
            body: format!("<pre>{}</pre>", esc(&value.as_str())),
            note: String::new(),
        },
        Event::EffectIntent { name, arg } => Row {
            kind: "effect · intent",
            quiet: true,
            title: format!("{} — declared before the world changed", esc(name)),
            body: format!("<pre>{}</pre>", esc(&arg.as_str())),
            note: "write-ahead: the journal knows about this effect before it runs".to_string(),
        },
        Event::EffectCommit { name, result } => Row {
            kind: "effect · committed",
            quiet: false,
            title: format!("{} completed", esc(name)),
            body: format!("<pre>{}</pre>", esc(&result.as_str())),
            note: String::new(),
        },
        Event::EffectCompensated { name } => Row {
            kind: "effect · compensated",
            quiet: false,
            title: format!("{} undone", esc(name)),
            body: String::new(),
            note: "saga rollback — replay will never double-undo".to_string(),
        },
        Event::Clock { nanos } => Row {
            kind: "clock",
            quiet: true,
            title: "Time entered the record.".to_string(),
            body: format!("<pre>{nanos} ns since epoch</pre>"),
            note: String::new(),
        },
    }
}

pub fn render(run_id: &str, journal: &Journal) -> String {
    let verified = journal.verify().is_ok();
    let head = journal
        .head()
        .map(|h| hex(&h))
        .unwrap_or_else(|| "—".to_string());
    let dangling = journal.dangling_intents().len();

    let mut entries = String::new();
    for e in journal.entries() {
        let r = row(&e.event);
        let hash = hex(&e.hash);
        let quiet = if r.quiet { " quiet" } else { "" };
        entries.push_str(&format!(
            "<div class=\"entry\">\
               <div class=\"num\">{cursor:02}</div>\
               <div class=\"entry-head\">\
                 <div class=\"kind{quiet}\">{kind}</div>\
                 <div class=\"title\">{title}</div>\
               </div>\
               <div class=\"hash\" title=\"{hash}\">{hash12}</div>\
               {body}\
               {note}\
             </div>\n",
            cursor = e.cursor,
            kind = r.kind,
            title = r.title,
            hash = hash,
            hash12 = &hash[..12],
            body = if r.body.is_empty() {
                String::new()
            } else {
                format!("<div class=\"body\">{}</div>", r.body)
            },
            note = if r.note.is_empty() {
                String::new()
            } else {
                format!("<div class=\"note\">{}</div>", r.note)
            },
        ));
    }

    let chain = if verified {
        "chain <b>verified</b>".to_string()
    } else {
        "<span class=\"alarm\">chain broken</span>".to_string()
    };
    let effects = if dangling > 0 {
        format!("<div><span class=\"alarm\">{dangling} open intent(s) — crashed mid-effect</span></div>")
    } else {
        String::new()
    };

    let body = format!(
        r#"<div class="wrap">
  <div class="brand">{MARK}<a href="/">Pragmatic</a></div>
  <div class="kicker">The record of a run</div>
  <h1>run <span class="id">{run}</span></h1>
  <div class="meta">
    <div><b>{count}</b> journaled steps</div>
    <div>{chain}</div>
    {effects}
    <div>head <span class="mono">{head_short}</span></div>
  </div>
  {entries}
</div>"#,
        run = esc(run_id),
        count = journal.len(),
        head_short = &head[..head.len().min(16)],
    );
    page(&format!("{} — pragmatic", esc(run_id)), &body)
}
