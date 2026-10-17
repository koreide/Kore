//! Visual regression: compares screenshots of the same scenario step between two runs
//! (a known-good reference run vs a new run) and produces diff images plus an HTML report.
//!
//! Live data and glyph antialiasing make an exact pixel diff noisy.
//! We compare at reduced resolution (layout, color and spacing survive; glyph
//! antialiasing mostly averages out) and mask regions with live data.

use crate::os::ocr;
use crate::scenario::ShotMeta;
use anyhow::{Context, Result};
use image::{imageops, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Downscale factor applied before diffing (on 2x captures this is ~4 logical px per sample).
const DOWNSCALE: u32 = 8;
/// Per-channel delta above which a downscaled pixel counts as different.
const CHANNEL_THRESHOLD: u8 = 28;
const DEFAULT_TOLERANCE: f64 = 0.03;

#[derive(Debug, Serialize)]
pub struct ShotComparison {
    pub scenario: String,
    pub name: String,
    pub baseline: Option<PathBuf>,
    pub candidate: Option<PathBuf>,
    pub diff: Option<PathBuf>,
    /// Fraction of compared (unmasked) downscaled pixels that differ.
    pub mismatch: f64,
    pub tolerance: f64,
    pub ok: bool,
    pub note: String,
}

fn masks_for(path: &Path, patterns: &[String]) -> Result<Vec<[f64; 4]>> {
    if patterns.is_empty() {
        return Ok(vec![]);
    }
    let (w, h) = image::image_dimensions(path)?;
    let q: Vec<crate::driver::TextQuery> = patterns.iter().map(|p| p.as_str().into()).collect();
    Ok(ocr::recognize(path)?
        .into_iter()
        .filter(|b| q.iter().any(|q| q.matches(&b.text)))
        .map(|b| {
            // Pad masks a little: the same value renders with different widths per app.
            let pad = 6.0;
            [
                ((b.x - pad) / w as f64).max(0.0),
                ((b.y - pad) / h as f64).max(0.0),
                ((b.x + b.w + pad * 6.0) / w as f64).min(1.0),
                ((b.y + b.h + pad) / h as f64).min(1.0),
            ]
        })
        .collect())
}

fn in_masks(masks: &[[f64; 4]], nx: f64, ny: f64) -> bool {
    masks.iter().any(|[x0, y0, x1, y1]| nx >= *x0 && nx <= *x1 && ny >= *y0 && ny <= *y1)
}

pub fn compare_pair(baseline: &Path, candidate: &Path, meta: &ShotMeta, diff_out: &Path) -> Result<(f64, String)> {
    let a = image::open(baseline).with_context(|| format!("open {}", baseline.display()))?;
    let b = image::open(candidate).with_context(|| format!("open {}", candidate.display()))?;
    let mut note = String::new();
    if a.dimensions() != b.dimensions() {
        note = format!("size differs: baseline {:?} vs candidate {:?}", a.dimensions(), b.dimensions());
    }
    let (w, h) = a.dimensions();
    let b = if b.dimensions() != (w, h) { b.resize_exact(w, h, imageops::FilterType::Triangle) } else { b };

    let mut masks = meta.mask_rects.clone();
    masks.extend(masks_for(baseline, &meta.mask)?);
    masks.extend(masks_for(candidate, &meta.mask)?);

    let (sw, sh) = (w / DOWNSCALE, h / DOWNSCALE);
    let sa = imageops::resize(&a.to_rgba8(), sw, sh, imageops::FilterType::Triangle);
    let sb = imageops::resize(&b.to_rgba8(), sw, sh, imageops::FilterType::Triangle);

    let mut differing = 0u64;
    let mut compared = 0u64;
    let mut diff_small: RgbaImage = ImageBuffer::new(sw, sh);
    for y in 0..sh {
        for x in 0..sw {
            let (nx, ny) = ((x as f64 + 0.5) / sw as f64, (y as f64 + 0.5) / sh as f64);
            let pa = sa.get_pixel(x, y);
            let pb = sb.get_pixel(x, y);
            if in_masks(&masks, nx, ny) {
                diff_small.put_pixel(x, y, Rgba([40, 40, 90, 255]));
                continue;
            }
            compared += 1;
            let delta = (0..3).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0);
            if delta > CHANNEL_THRESHOLD {
                differing += 1;
                diff_small.put_pixel(x, y, Rgba([255, 40, 80, 255]));
            } else {
                let g = (pa[0] as u16 + pa[1] as u16 + pa[2] as u16) / 3 / 3;
                diff_small.put_pixel(x, y, Rgba([g as u8, g as u8, g as u8, 255]));
            }
        }
    }
    // Upscale the diff map and blend it over the candidate for an easy-to-read overlay.
    let diff_big = imageops::resize(&diff_small, w, h, imageops::FilterType::Nearest);
    let mut overlay = b.to_rgba8();
    for (x, y, p) in overlay.enumerate_pixels_mut() {
        let d = diff_big.get_pixel(x, y);
        if d[0] == 255 {
            *p = Rgba([255, (p[1] / 3), (p[2] / 3), 255]);
        } else if d[2] == 90 {
            *p = Rgba([p[0] / 2, p[1] / 2, (p[2] / 2).saturating_add(80), 255]);
        }
    }
    overlay.save(diff_out)?;
    let mismatch = if compared == 0 { 0.0 } else { differing as f64 / compared as f64 };
    Ok((mismatch, note))
}

/// Compares every screenshot present in `baseline_run` against `candidate_run`.
pub fn compare_runs(baseline_run: &Path, candidate_run: &Path, out: &Path) -> Result<Vec<ShotComparison>> {
    std::fs::create_dir_all(out)?;
    let mut results = Vec::new();
    let mut scenarios: Vec<PathBuf> = std::fs::read_dir(baseline_run)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "home" && n != "compare"))
        .collect();
    scenarios.sort();
    for sdir in scenarios {
        let scenario = sdir.file_name().unwrap().to_string_lossy().into_owned();
        let mut metas: Vec<PathBuf> = std::fs::read_dir(&sdir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json") && !p.ends_with("result.json"))
            .collect();
        metas.sort();
        for meta_path in metas {
            let meta: ShotMeta = match serde_json::from_slice(&std::fs::read(&meta_path)?) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let base_png = sdir.join(format!("{}.png", meta.name));
            let cand_png = candidate_run.join(&scenario).join(format!("{}.png", meta.name));
            let tolerance = meta.tolerance.unwrap_or(DEFAULT_TOLERANCE);
            if !cand_png.exists() {
                results.push(ShotComparison {
                    scenario: scenario.clone(),
                    name: meta.name.clone(),
                    baseline: Some(base_png),
                    candidate: None,
                    diff: None,
                    mismatch: 1.0,
                    tolerance,
                    ok: false,
                    note: "missing in candidate run".into(),
                });
                continue;
            }
            let diff = out.join(format!("{scenario}__{}.diff.png", meta.name));
            let (mismatch, note) = compare_pair(&base_png, &cand_png, &meta, &diff)?;
            results.push(ShotComparison {
                scenario: scenario.clone(),
                name: meta.name.clone(),
                baseline: Some(base_png),
                candidate: Some(cand_png),
                diff: Some(diff),
                mismatch,
                tolerance,
                ok: mismatch <= tolerance && note.is_empty(),
                note,
            });
        }
    }
    std::fs::write(out.join("compare.json"), serde_json::to_vec_pretty(&results)?)?;
    std::fs::write(out.join("index.html"), render_html(&results))?;
    Ok(results)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn render_html(results: &[ShotComparison]) -> String {
    let passed = results.iter().filter(|r| r.ok).count();
    let mut html = format!(
        "<!doctype html><meta charset=utf-8><title>Kore visual parity</title>\
         <style>body{{background:#0b1221;color:#e2e8f0;font:14px -apple-system,sans-serif;margin:24px}}\
         .row{{margin:28px 0;border-top:1px solid #1e293b;padding-top:12px}}\
         .imgs{{display:grid;grid-template-columns:1fr 1fr 1fr;gap:8px}}img{{width:100%;border:1px solid #1e293b}}\
         .ok{{color:#4ade80}}.bad{{color:#f87171}}small{{color:#94a3b8}}</style>\
         <h1>Visual parity: {passed}/{} within tolerance</h1>",
        results.len()
    );
    for r in results {
        let img = |p: &Option<PathBuf>| {
            p.as_ref()
                .map(|p| format!("<img src=\"file://{}\">", esc(&p.display().to_string())))
                .unwrap_or_else(|| "<div>missing</div>".into())
        };
        html.push_str(&format!(
            "<div class=row><h3 class={}>{} / {} — {:.2}% differ (tolerance {:.1}%)</h3><small>{}</small>\
             <div class=imgs><div><small>baseline</small>{}</div><div><small>candidate</small>{}</div><div><small>diff</small>{}</div></div></div>",
            if r.ok { "ok" } else { "bad" },
            esc(&r.scenario),
            esc(&r.name),
            r.mismatch * 100.0,
            r.tolerance * 100.0,
            esc(&r.note),
            img(&r.baseline),
            img(&r.candidate),
            img(&r.diff),
        ));
    }
    html
}
