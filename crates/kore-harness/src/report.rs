//! HTML + JSON summary of a scenario run.

use crate::scenario::ScenarioResult;
use anyhow::Result;
use std::path::Path;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn write(out: &Path, results: &[ScenarioResult]) -> Result<()> {
    std::fs::write(out.join("summary.json"), serde_json::to_vec_pretty(results)?)?;
    let passed = results.iter().filter(|r| r.ok).count();
    let failed = results.iter().filter(|r| !r.ok).count();
    let mut html = format!(
        "<!doctype html><meta charset=utf-8><title>Kore harness</title>\
         <style>body{{background:#0b1221;color:#e2e8f0;font:13px -apple-system,sans-serif;margin:24px}}\
         details{{border-top:1px solid #1e293b;padding:8px 0}}summary{{cursor:pointer;font-size:15px}}\
         .ok{{color:#4ade80}}.bad{{color:#f87171}}td{{padding:2px 10px;vertical-align:top}}\
         img{{max-width:900px;border:1px solid #1e293b;margin:6px 0}}code{{color:#94a3b8}}</style>\
         <h1>{passed} passed, {failed} failed, {} total</h1>",
        results.len()
    );
    for r in results {
        let cls = if r.ok { "ok" } else { "bad" };
        html.push_str(&format!(
            "<details {}><summary class={cls}>{} {} <code>{} ms</code></summary><p>{}</p><table>",
            if r.ok { "" } else { "open" },
            if r.ok { "✓" } else { "✗" },
            esc(&r.name),
            r.millis,
            esc(&r.description)
        ));
        for s in &r.steps {
            let scls = if s.ok { "ok" } else { "bad" };
            html.push_str(&format!(
                "<tr><td class={scls}>{}</td><td>{}{}</td><td><code>{} ms</code></td></tr>",
                s.index,
                esc(&s.summary),
                s.error.as_deref().map(|e| format!("<br><span class=bad>{}</span>", esc(e))).unwrap_or_default(),
                s.millis
            ));
            if let Some(shot) = &s.screenshot {
                html.push_str(&format!("<tr><td></td><td colspan=2><img src=\"{}/{}\"></td></tr>", esc(&r.name), esc(shot)));
            }
        }
        if !r.ok {
            if let Some(last) = r.steps.last() {
                html.push_str(&format!(
                    "<tr><td></td><td colspan=2><img src=\"{}/{:02}-FAILURE.png\"></td></tr>",
                    esc(&r.name),
                    last.index
                ));
            }
        }
        html.push_str("</table></details>");
    }
    std::fs::write(out.join("report.html"), html)?;
    Ok(())
}
