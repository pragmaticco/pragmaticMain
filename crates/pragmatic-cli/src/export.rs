//! Self-contained HTML replay console: one run's journal as a browsable
//! timeline. No external assets, opens anywhere, safe to attach to an
//! incident ticket.

use pragmatic::journal::{Event, Journal};
use pragmatic::sha256::hex;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct Row {
    kind: &'static str,
    title: String,
    body: String,
    meta: String,
}

fn row(event: &Event) -> Row {
    match event {
        Event::Program { name, hash } => Row {
            kind: "program",
            title: format!("program <code>{}</code>", esc(name)),
            body: String::new(),
            meta: format!("source hash {}", esc(hash)),
        },
        Event::OracleDraw {
            prompt,
            outcome,
            provenance,
        } => Row {
            kind: "oracle",
            title: "oracle draw".to_string(),
            body: format!(
                "<div class=\"lbl\">prompt</div><pre>{}</pre>\
                 <div class=\"lbl\">outcome</div><pre>{}</pre>",
                esc(&prompt.as_str()),
                esc(&outcome.as_str())
            ),
            meta: esc(provenance),
        },
        Event::ChannelRecv { channel, value } => Row {
            kind: "recv",
            title: format!("receive on <code>{}</code>", esc(channel)),
            body: format!("<pre>{}</pre>", esc(&value.as_str())),
            meta: String::new(),
        },
        Event::EffectIntent { name, arg } => Row {
            kind: "intent",
            title: format!("effect intent <code>{}</code>", esc(name)),
            body: format!("<pre>{}</pre>", esc(&arg.as_str())),
            meta: "write-ahead: journaled before the world changed".to_string(),
        },
        Event::EffectCommit { name, result } => Row {
            kind: "commit",
            title: format!("effect committed <code>{}</code>", esc(name)),
            body: format!("<pre>{}</pre>", esc(&result.as_str())),
            meta: String::new(),
        },
        Event::EffectCompensated { name } => Row {
            kind: "compensated",
            title: format!("effect compensated <code>{}</code>", esc(name)),
            body: String::new(),
            meta: "saga rollback — replay will not double-undo".to_string(),
        },
        Event::Clock { nanos } => Row {
            kind: "clock",
            title: "clock read".to_string(),
            body: format!("<pre>{nanos} ns since epoch</pre>"),
            meta: String::new(),
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

    let mut rows = String::new();
    for e in journal.entries() {
        let r = row(&e.event);
        let hash = hex(&e.hash);
        rows.push_str(&format!(
            "<div class=\"entry {kind}\">\
               <div class=\"head\">\
                 <span class=\"cursor\">#{cursor}</span>\
                 <span class=\"badge {kind}\">{kind}</span>\
                 <span class=\"title\">{title}</span>\
                 <span class=\"hash\" title=\"{hash}\">{hash8}</span>\
               </div>\
               {body}\
               {meta}\
             </div>\n",
            kind = r.kind,
            cursor = e.cursor,
            title = r.title,
            hash = hash,
            hash8 = &hash[..16],
            body = if r.body.is_empty() {
                String::new()
            } else {
                format!("<div class=\"body\">{}</div>", r.body)
            },
            meta = if r.meta.is_empty() {
                String::new()
            } else {
                format!("<div class=\"meta\">{}</div>", r.meta)
            },
        ));
    }

    let chain_badge = if verified {
        "<span class=\"chain ok\">chain verified</span>"
    } else {
        "<span class=\"chain bad\">CHAIN BROKEN</span>"
    };
    let dangling_badge = if dangling > 0 {
        format!(
            "<span class=\"chain warn\">{dangling} dangling intent(s) — crashed mid-effect</span>"
        )
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{run} — pragmatic replay</title>
<style>
  :root {{
    --bg: #0b0e14; --panel: #11151f; --line: #1e2530; --text: #d7dde8;
    --dim: #7b8496; --accent: #7aa2f7;
    --oracle: #7aa2f7; --intent: #e0af68; --commit: #9ece6a; --recv: #bb9af7;
    --compensated: #f7768e; --clock: #565f89; --program: #2ac3de;
  }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; background: var(--bg); color: var(--text);
         font: 15px/1.5 ui-sans-serif, system-ui, -apple-system, sans-serif; }}
  .wrap {{ max-width: 880px; margin: 0 auto; padding: 40px 20px 80px; }}
  h1 {{ font-size: 20px; margin: 0 0 4px; }}
  h1 code {{ color: var(--accent); }}
  .sub {{ color: var(--dim); font-size: 13px; margin-bottom: 8px; word-break: break-all; }}
  .chips {{ margin: 14px 0 28px; display: flex; gap: 8px; flex-wrap: wrap; }}
  .chain {{ font-size: 12px; padding: 3px 10px; border-radius: 999px; font-weight: 600; }}
  .chain.ok {{ background: #1c2b1e; color: #9ece6a; }}
  .chain.bad {{ background: #34191f; color: #f7768e; }}
  .chain.warn {{ background: #332a18; color: #e0af68; }}
  .entry {{ background: var(--panel); border: 1px solid var(--line);
            border-left: 3px solid var(--dim); border-radius: 8px;
            padding: 12px 16px; margin-bottom: 10px; }}
  .entry.oracle {{ border-left-color: var(--oracle); }}
  .entry.intent {{ border-left-color: var(--intent); }}
  .entry.commit {{ border-left-color: var(--commit); }}
  .entry.recv {{ border-left-color: var(--recv); }}
  .entry.compensated {{ border-left-color: var(--compensated); }}
  .entry.clock {{ border-left-color: var(--clock); }}
  .entry.program {{ border-left-color: var(--program); }}
  .head {{ display: flex; align-items: baseline; gap: 10px; }}
  .cursor {{ color: var(--dim); font-family: ui-monospace, monospace; font-size: 12px; min-width: 34px; }}
  .badge {{ font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: .05em; }}
  .badge.oracle {{ color: var(--oracle); }} .badge.intent {{ color: var(--intent); }}
  .badge.commit {{ color: var(--commit); }} .badge.recv {{ color: var(--recv); }}
  .badge.compensated {{ color: var(--compensated); }} .badge.clock {{ color: var(--clock); }}
  .badge.program {{ color: var(--program); }}
  .title {{ flex: 1; }} .title code {{ color: var(--text); }}
  .hash {{ color: var(--dim); font-family: ui-monospace, monospace; font-size: 11px; }}
  .body {{ margin-top: 10px; }}
  .lbl {{ color: var(--dim); font-size: 11px; text-transform: uppercase;
          letter-spacing: .06em; margin: 8px 0 2px; }}
  pre {{ background: #0d1118; border: 1px solid var(--line); border-radius: 6px;
        padding: 8px 12px; margin: 2px 0; white-space: pre-wrap;
        word-break: break-word; font: 13px/1.5 ui-monospace, monospace; }}
  .meta {{ color: var(--dim); font-size: 12px; margin-top: 8px; }}
  footer {{ color: var(--dim); font-size: 12px; margin-top: 40px; text-align: center; }}
  footer a {{ color: var(--accent); text-decoration: none; }}
</style>
</head>
<body>
<div class="wrap">
  <h1>run <code>{run}</code></h1>
  <div class="sub">{count} journaled steps · chain head {head}</div>
  <div class="chips">{chain_badge}{dangling_badge}</div>
  {rows}
  <footer>append-only, hash-chained journal · exported by
    <a href="https://github.com/pragmaticco/pragmaticMain">pragmatic</a> —
    durable execution for nondeterministic agents</footer>
</div>
</body>
</html>
"#,
        run = esc(run_id),
        count = journal.len(),
        head = head,
        chain_badge = chain_badge,
        dangling_badge = dangling_badge,
        rows = rows,
    )
}
