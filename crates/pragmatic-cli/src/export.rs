//! Self-contained HTML replay console: one run's journal as a browsable
//! timeline. No external requests — the brand serif (EB Garamond, SIL OFL)
//! and the Pragmatic mark are embedded as data URIs — so an exported page
//! opens anywhere and is safe to attach to an incident ticket.

use std::sync::OnceLock;

use pragmatic::journal::{Event, Journal};
use pragmatic::sha256::hex;

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Minimal base64 (standard alphabet, padded) — keeps the CLI zero-dep.
fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

const GARAMOND: &[u8] = include_bytes!("../assets/ebgaramond.woff2");
const GARAMOND_ITALIC: &[u8] = include_bytes!("../assets/ebgaramond-italic.woff2");
/// The brand mark as shipped on the site: white on transparent (for the
/// black footer band).
const MARK_LIGHT_PNG: &[u8] = include_bytes!("../assets/mark.png");
/// The same mark recolored to ink for light surfaces (masthead, favicon).
const MARK_INK_PNG: &[u8] = include_bytes!("../assets/mark-ink.png");

struct Assets {
    fonts_css: String,
    mark_light_uri: String,
    mark_ink_uri: String,
}

fn assets() -> &'static Assets {
    static A: OnceLock<Assets> = OnceLock::new();
    A.get_or_init(|| Assets {
        fonts_css: format!(
            "@font-face {{ font-family: 'EB Garamond'; font-style: normal; \
               font-weight: 400 800; font-display: swap; \
               src: url(data:font/woff2;base64,{}) format('woff2'); }}\n\
             @font-face {{ font-family: 'EB Garamond'; font-style: italic; \
               font-weight: 400 800; font-display: swap; \
               src: url(data:font/woff2;base64,{}) format('woff2'); }}",
            b64(GARAMOND),
            b64(GARAMOND_ITALIC),
        ),
        mark_light_uri: format!("data:image/png;base64,{}", b64(MARK_LIGHT_PNG)),
        mark_ink_uri: format!("data:image/png;base64,{}", b64(MARK_INK_PNG)),
    })
}

/// The Pragmatic mark (ink-on-transparent) as a self-contained data URI.
pub fn mark_uri() -> &'static str {
    &assets().mark_ink_uri
}

/// Shared stylesheet for the console pages (index and run timeline).
const STYLE: &str = r#"
  :root {
    --bg: #f9f8f6; --card: #ffffff; --tint: #f3f1ec; --ink: #0d0c0b;
    --muted: #777166; --faint: #a8a297; --hair: #e7e4dd; --rule: #0d0c0b;
    --serif: 'EB Garamond', Georgia, 'Times New Roman', serif;
    --sans: 'Inter', -apple-system, BlinkMacSystemFont, 'Helvetica Neue', Arial, sans-serif;
    --mono: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
  }
  * { box-sizing: border-box; margin: 0; }
  html { scroll-behavior: smooth; }
  body { background: var(--bg); color: var(--ink);
         font: 15px/1.6 var(--sans); -webkit-font-smoothing: antialiased; }
  a { color: inherit; }
  .wrap { max-width: 1020px; margin: 0 auto; padding: 0 36px; }

  /* masthead */
  .mast { display: flex; align-items: center; justify-content: space-between;
          padding: 26px 0; border-bottom: 1px solid var(--hair); }
  .mast .id { display: flex; align-items: center; gap: 12px; text-decoration: none; }
  .mast img { width: 26px; height: 26px; display: block; }
  .mast .word { font: 500 21px/1 var(--serif); letter-spacing: .01em; }
  .mast .tag { font: 600 10px/1 var(--sans); letter-spacing: .28em;
               text-transform: uppercase; color: var(--muted); }

  /* hero */
  .hero { padding: 88px 0 56px; }
  .kicker { font: 600 11px/1 var(--sans); letter-spacing: .28em;
            text-transform: uppercase; color: var(--faint); margin-bottom: 26px; }
  h1 { font: italic 400 clamp(40px, 5.4vw, 64px)/1.08 var(--serif);
       letter-spacing: -0.015em; font-feature-settings: 'liga', 'kern'; }
  h1 .id { white-space: nowrap; }
  .lede { margin-top: 22px; max-width: 560px; color: var(--muted);
          font-size: 15.5px; }

  /* stat strip */
  .stats { display: flex; flex-wrap: wrap;
           border-top: 1px solid var(--rule); border-bottom: 1px solid var(--hair);
           margin-bottom: 72px; }
  .stat { flex: 1 1 0; min-width: 150px; padding: 26px 28px 26px 0;
          white-space: nowrap; }
  .stat + .stat { padding-left: 28px; border-left: 1px solid var(--hair); }
  @media (max-width: 760px) {
    .stat { flex: 1 1 40%; }
    .stat + .stat { padding-left: 0; border-left: 0; }
  }
  .stat .n { font: 400 30px/1.1 var(--serif); letter-spacing: -.01em; }
  .stat .n.mono { font: 500 17px/1.7 var(--mono); letter-spacing: .03em; }
  .stat .n.alarm { border-bottom: 3px solid var(--ink); display: inline-block; }
  .stat .l { margin-top: 8px; font: 600 10px/1.5 var(--sans);
             letter-spacing: .22em; text-transform: uppercase; color: var(--faint); }

  /* timeline */
  .timeline { display: flex; flex-direction: column; gap: 18px;
              padding-bottom: 8px; }
  .card { background: var(--card); border: 1px solid var(--hair); }
  .entry { display: grid; grid-template-columns: 96px 1fr; }
  .entry .gutter { border-right: 1px solid var(--hair); padding: 30px 0;
                   text-align: center; }
  .entry .num { font: italic 400 30px/1 var(--serif); color: var(--faint); }
  .entry .main { padding: 28px 34px 30px; min-width: 0; }
  .entry .top { display: flex; align-items: baseline; gap: 18px; }
  .kind { font: 600 10.5px/1 var(--sans); letter-spacing: .24em;
          text-transform: uppercase; color: var(--ink); }
  .kind.quiet { color: var(--faint); }
  .hash { margin-left: auto; font: 11px/1 var(--mono); color: var(--faint); }
  .title { font: italic 400 21px/1.35 var(--serif); margin-top: 10px; }
  .io { margin-top: 20px; display: flex; flex-direction: column; gap: 16px; }
  .lbl { font: 600 10px/1 var(--sans); letter-spacing: .22em;
         text-transform: uppercase; color: var(--faint); margin-bottom: 8px; }
  pre { white-space: pre-wrap; word-break: break-word;
        font: 13px/1.65 var(--mono); }
  .block.prompt pre { background: var(--tint); padding: 14px 18px;
                      color: var(--muted); }
  .block.outcome pre { border-left: 2px solid var(--ink); padding: 4px 0 4px 18px;
                       font-size: 14px; color: var(--ink); }
  .note { margin-top: 18px; font: italic 15px/1.5 var(--serif); color: var(--muted); }

  /* run index */
  .runlist { display: flex; flex-direction: column; gap: 14px; padding-bottom: 8px; }
  .runrow { display: flex; align-items: baseline; gap: 24px;
            padding: 26px 34px; text-decoration: none;
            transition: border-color .15s ease, transform .15s ease; }
  .runrow:hover { border-color: var(--ink); }
  .runrow .name { font: italic 400 26px/1.2 var(--serif); }
  .runrow .rmeta { margin-left: auto; display: flex; gap: 22px; align-items: baseline; }
  .pill { font: 600 10px/1.6 var(--sans); letter-spacing: .2em;
          text-transform: uppercase; color: var(--muted); white-space: nowrap; }
  .pill.alarm { color: var(--ink); border-bottom: 2px solid var(--ink); }
  .pill.mono { font-family: var(--mono); letter-spacing: .04em; }
  .runrow .arrow { font: 400 22px/1 var(--serif); color: var(--faint);
                   transition: transform .15s ease, color .15s ease; }
  .runrow:hover .arrow { transform: translateX(6px); color: var(--ink); }
  .empty { padding: 48px 34px; font: italic 19px/1.6 var(--serif);
           color: var(--muted); }

  /* footer */
  footer { background: var(--ink); color: var(--bg); margin-top: 120px; }
  footer .inner { max-width: 1020px; margin: 0 auto; padding: 72px 36px;
                  display: flex; justify-content: space-between;
                  align-items: center; flex-wrap: wrap; gap: 28px; }
  footer .line { font: italic 400 clamp(24px, 3vw, 34px)/1.25 var(--serif); }
  footer .colophon { text-align: right; }
  footer img { width: 30px; height: 30px; margin-bottom: 14px; }
  footer .credit { font: 600 10px/1.8 var(--sans); letter-spacing: .24em;
                   text-transform: uppercase; color: #8a857b; }
  footer .credit a { color: var(--bg); text-decoration: none; }

  @media (max-width: 760px) {
    .entry { grid-template-columns: 1fr; }
    .entry .gutter { border-right: 0; border-bottom: 1px solid var(--hair);
                     padding: 14px 0; }
    .runrow { flex-wrap: wrap; }
    .runrow .rmeta { margin-left: 0; flex-wrap: wrap; }
    footer .colophon { text-align: left; }
  }
"#;

pub fn page(title: &str, body: &str) -> String {
    let a = assets();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="icon" type="image/png" href="{mark}">
<style>{fonts}{STYLE}</style>
</head>
<body>
{body}
<footer><div class="inner">
  <div class="line">Crash. Recover.<br>Replay, exactly.</div>
  <div class="colophon">
    <img src="{mark_light}" alt="">
    <div class="credit"><a href="https://github.com/pragmaticco/pragmaticMain">Pragmatic</a> — durable execution<br>for nondeterministic agents</div>
  </div>
</div></footer>
</body>
</html>
"#,
        fonts = a.fonts_css,
        mark = a.mark_ink_uri,
        mark_light = a.mark_light_uri,
    )
}

/// The shared masthead (logo, wordmark, section tag).
pub fn masthead(tag: &str) -> String {
    format!(
        r#"<div class="mast"><a class="id" href="/"><img src="{}" alt="Pragmatic">
        <span class="word">Pragmatic</span></a><span class="tag">{tag}</span></div>"#,
        mark_uri()
    )
}

struct Row {
    kind: &'static str,
    quiet: bool,
    title: String,
    io: String,
    note: String,
}

fn block(class: &str, label: &str, text: &str) -> String {
    format!(
        "<div class=\"block {class}\"><div class=\"lbl\">{label}</div><pre>{}</pre></div>",
        esc(text)
    )
}

fn row(event: &Event) -> Row {
    match event {
        Event::Program { name, hash } => Row {
            kind: "program",
            quiet: true,
            title: format!("<em>{}</em> enters the record.", esc(name)),
            io: String::new(),
            note: format!(
                "source hash {} — a changed program refuses to replay this journal",
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
            io: format!(
                "{}{}",
                block("prompt", "Prompt", &prompt.as_str()),
                block("outcome", "Realized outcome", &outcome.as_str())
            ),
            note: esc(provenance),
        },
        Event::ChannelRecv { channel, value } => Row {
            kind: "receive",
            quiet: false,
            title: format!("A message arrives on <em>{}</em>.", esc(channel)),
            io: block("outcome", "Value", &value.as_str()),
            note: String::new(),
        },
        Event::EffectIntent { name, arg } => Row {
            kind: "effect — intent",
            quiet: true,
            title: format!("<em>{}</em>, declared before the world changes.", esc(name)),
            io: block("prompt", "Argument", &arg.as_str()),
            note: "write-ahead: the journal knows about this effect before it runs".to_string(),
        },
        Event::EffectCommit { name, result } => Row {
            kind: "effect — committed",
            quiet: false,
            title: format!("<em>{}</em> completed.", esc(name)),
            io: block("outcome", "Result", &result.as_str()),
            note: String::new(),
        },
        Event::EffectCompensated { name } => Row {
            kind: "effect — compensated",
            quiet: false,
            title: format!("<em>{}</em> undone.", esc(name)),
            io: String::new(),
            note: "saga rollback — replay will never double-undo".to_string(),
        },
        Event::Clock { nanos } => Row {
            kind: "clock",
            quiet: true,
            title: "Time enters the record.".to_string(),
            io: block("prompt", "Reading", &format!("{nanos} ns since epoch")),
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
    let draws = journal
        .entries()
        .iter()
        .filter(|e| matches!(e.event, Event::OracleDraw { .. }))
        .count();

    let mut entries = String::new();
    for e in journal.entries() {
        let r = row(&e.event);
        let hash = hex(&e.hash);
        entries.push_str(&format!(
            "<article class=\"card entry\">\
               <div class=\"gutter\"><div class=\"num\">{cursor:02}</div></div>\
               <div class=\"main\">\
                 <div class=\"top\">\
                   <span class=\"kind{quiet}\">{kind}</span>\
                   <span class=\"hash\" title=\"{hash}\">{hash12}</span>\
                 </div>\
                 <div class=\"title\">{title}</div>\
                 {io}{note}\
               </div>\
             </article>\n",
            cursor = e.cursor,
            quiet = if r.quiet { " quiet" } else { "" },
            kind = r.kind,
            title = r.title,
            hash = hash,
            hash12 = &hash[..12],
            io = if r.io.is_empty() {
                String::new()
            } else {
                format!("<div class=\"io\">{}</div>", r.io)
            },
            note = if r.note.is_empty() {
                String::new()
            } else {
                format!("<div class=\"note\">{}</div>", r.note)
            },
        ));
    }

    let chain_stat = if verified {
        "<div class=\"n\">Verified</div><div class=\"l\">Hash chain</div>"
    } else {
        "<div class=\"n alarm\">Broken</div><div class=\"l\">Hash chain</div>"
    };
    let effect_stat = if dangling > 0 {
        format!(
            "<div class=\"stat\"><div class=\"n alarm\">{dangling} open</div>\
             <div class=\"l\">Effect intents</div></div>"
        )
    } else {
        String::new()
    };

    let body = format!(
        r#"<div class="wrap">
  {mast}
  <div class="hero">
    <div class="kicker">The record of a run</div>
    <h1>run <span class="id">{run}</span></h1>
    <div class="lede">Every externally determined outcome this agent consumed,
    in order, hash-chained. Replaying this journal reproduces the run exactly —
    the model is never called.</div>
  </div>
  <div class="stats">
    <div class="stat"><div class="n">{count}</div><div class="l">Journaled steps</div></div>
    <div class="stat"><div class="n">{draws}</div><div class="l">Oracle draws</div></div>
    <div class="stat">{chain_stat}</div>
    {effect_stat}
    <div class="stat"><div class="n mono" title="{head}">{head_short}</div><div class="l">Chain head</div></div>
  </div>
  <div class="timeline">
  {entries}
  </div>
</div>"#,
        mast = masthead("Replay console"),
        run = esc(run_id),
        count = journal.len(),
        head_short = &head[..head.len().min(12)],
    );
    page(&format!("{} — pragmatic", esc(run_id)), &body)
}
