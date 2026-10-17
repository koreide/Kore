//! Declarative YAML scenarios, and the runner that executes them.
//!
//! ```yaml
//! name: pods-list
//! description: Pods view lists the seeded pods with status badges
//! steps:
//!   - click: {text: Pods, within: [0, 0, 0.2, 1]}
//!   - wait: {text: multi-container, timeout: 15}
//!   - screenshot: {name: pods-list, mask: ["re:^\\d+[smhd]$"]}
//!   - perf: {action: scroll_table, max_p95_ms: 16.7}
//! ```

use crate::driver::{App, Layer, TextQuery};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// `app` (Layer 2, default): in-app control channel. `os` (Layer 3): real OS
    /// input; lives in `harness/scenarios/os/` and runs only when named explicitly.
    #[serde(default)]
    pub layer: Layer,
    /// Extra env for the app process (e.g. pointing the AI provider at the mock server).
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Restart the app before this scenario (otherwise scenarios share one app session).
    #[serde(default)]
    pub fresh_app: bool,
    pub steps: Vec<Step>,
    #[serde(skip)]
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Free-form note shown in the report.
    #[serde(default)]
    pub note: Option<String>,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Click(ClickSpec),
    DoubleClick(ClickSpec),
    RightClick(ClickSpec),
    /// Click at normalized window coordinates `[x, y]`.
    ClickAt([f64; 2]),
    /// Move the mouse over an element (`{id: ...}`) or text.
    Hover(ClickSpec),
    Key(String),
    /// Several chords in order, e.g. `[j, j, l]`.
    Keys(Vec<String>),
    Type(String),
    Wait(WaitSpec),
    WaitGone(WaitSpec),
    /// All texts must be visible (single OCR pass, after optional retry window).
    Expect(ExpectSpec),
    ExpectNot(ExpectSpec),
    Screenshot(ShotSpec),
    Scroll(ScrollSpec),
    Drag(DragSpec),
    Sleep(u64),
    /// Run a shell command with the harness KUBECONFIG. Fails the step on non-zero exit.
    Shell(ShellSpec),
    /// Assert view state fields via the automation snapshot (JSON pointer -> value).
    State(Vec<(String, serde_json::Value)>),
    /// Measure frame timings while performing inner steps.
    Perf(PerfSpec),
    WindowTitle(String),
    /// Splice in steps from `harness/scenarios/_lib/<name>.yaml` (expanded at load time).
    Include(String),
    /// Quit and relaunch the app in the same sandbox (`restart_app: {}`), e.g. to check persistence.
    RestartApp(RestartSpec),
    /// The first text matching `text` must be drawn (at least `min_pixels` pixels) in one of `colors`.
    ExpectColor(ColorSpec),
    /// An automation element with this id exists (in the last frame) and, if
    /// given, its label matches the `label` regex. For values OCR cannot read reliably.
    Element(ElementSpec),
    /// No automation element matches this id (within `timeout`).
    ElementGone(ElementSpec),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementSpec {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestartSpec {
    /// After relaunch, wait for this text (default: none).
    #[serde(default)]
    pub wait: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorSpec {
    pub text: String,
    #[serde(default)]
    pub within: Option<[f64; 4]>,
    #[serde(default)]
    pub nth: usize,
    #[serde(default)]
    pub exact: bool,
    /// Hex colors, e.g. `"#f59e0b"`; any of them counts.
    pub colors: Vec<String>,
    #[serde(default = "default_min_pixels")]
    pub min_pixels: usize,
    /// Max RGB euclidean distance for a pixel to count as the color.
    #[serde(default = "default_color_dist")]
    pub max_dist: f64,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
}

fn default_min_pixels() -> usize {
    6
}

fn default_color_dist() -> f64 {
    60.0
}

fn parse_hex(c: &str) -> Result<[u8; 3]> {
    let h = c.trim_start_matches('#');
    if h.len() != 6 {
        bail!("bad color {c:?}");
    }
    let v = u32::from_str_radix(h, 16).with_context(|| format!("bad color {c:?}"))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClickSpec {
    Text(String),
    /// Target an automation element by id (`*` suffix = prefix match): stable
    /// across layout changes, unlike OCR'd text.
    Id(IdSpec),
    Query(QuerySpec),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdSpec {
    pub id: String,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    /// After clicking, wait for this text (as for text clicks).
    #[serde(default)]
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySpec {
    pub text: String,
    #[serde(default)]
    pub within: Option<[f64; 4]>,
    #[serde(default)]
    pub nth: usize,
    #[serde(default)]
    pub exact: bool,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    /// Offset from the text center in points, e.g. to hit an icon left of a label.
    #[serde(default)]
    pub offset: Option<[f64; 2]>,
    /// After clicking, wait for this text; re-click (up to 3 attempts) if it doesn't appear.
    #[serde(default)]
    pub until: Option<String>,
    /// Click at this normalized window x on the matched text's row (e.g. an icon column of a
    /// table row located by its name).
    #[serde(default)]
    pub at_x: Option<f64>,
}

fn default_timeout() -> f64 {
    10.0
}

impl ClickSpec {
    fn query(&self) -> QuerySpec {
        match self {
            ClickSpec::Id(i) => QuerySpec {
                text: format!("#{}", i.id),
                within: None,
                nth: 0,
                exact: false,
                timeout: i.timeout,
                offset: None,
                until: i.until.clone(),
                at_x: None,
            },
            ClickSpec::Text(t) => QuerySpec {
                text: t.clone(),
                within: None,
                nth: 0,
                exact: false,
                timeout: default_timeout(),
                offset: None,
                until: None,
                at_x: None,
            },
            ClickSpec::Query(q) => q.clone(),
        }
    }
}

impl QuerySpec {
    fn text_query(&self) -> TextQuery {
        TextQuery { text: self.text.clone(), within: self.within, nth: self.nth, exact: self.exact }
    }
}

pub type WaitSpec = ClickSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectSpec {
    pub texts: Vec<String>,
    #[serde(default)]
    pub within: Option<[f64; 4]>,
    /// Keep re-reading the screen for up to this many seconds before failing.
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    /// OCR only the `within` region (some full-window passes drop lines).
    #[serde(default)]
    pub crop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotSpec {
    pub name: String,
    /// Text patterns whose OCR boxes are excluded from visual comparison (ages, live metrics...).
    #[serde(default)]
    pub mask: Vec<String>,
    /// Normalized rects `[x0, y0, x1, y1]` excluded from visual comparison.
    #[serde(default)]
    pub mask_rects: Vec<[f64; 4]>,
    /// Max fraction of differing (downscaled) pixels tolerated vs the reference run.
    #[serde(default)]
    pub tolerance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollSpec {
    /// Where to scroll: normalized window point.
    pub at: [f64; 2],
    pub dy: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragSpec {
    pub from: [f64; 2],
    pub to: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShellSpec {
    Cmd(String),
    Full {
        cmd: String,
        #[serde(default)]
        expect_output: Option<String>,
        /// Retry until success for up to this many seconds.
        #[serde(default)]
        retry: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfSpec {
    pub steps: Vec<Step>,
    #[serde(default = "default_p95")]
    pub max_p95_ms: f64,
    #[serde(default)]
    pub max_over_budget_ratio: Option<f64>,
}

fn default_p95() -> f64 {
    16.7
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub index: usize,
    pub summary: String,
    pub ok: bool,
    pub error: Option<String>,
    pub millis: u128,
    pub screenshot: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub name: String,
    pub description: String,
    pub ok: bool,
    pub steps: Vec<StepResult>,
    pub millis: u128,
}

/// Screenshot metadata persisted next to each PNG for the comparer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotMeta {
    pub scenario: String,
    pub name: String,
    pub mask: Vec<String>,
    pub mask_rects: Vec<[f64; 4]>,
    pub tolerance: Option<f64>,
}

pub fn load_dir(dir: &Path, filter: &[String]) -> Result<Vec<Scenario>> {
    let mut files: Vec<PathBuf> = if dir.is_file() {
        vec![dir.to_path_buf()]
    } else {
        std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "yaml" || e == "yml"))
            .collect()
    };
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f)?;
        let mut sc: Scenario = match serde_yaml::from_str(&text) {
            Ok(sc) => sc,
            // With a filter, a broken file that is clearly unrelated (file name matches no filter)
            // should not block the run (other scenarios may be mid-edit).
            Err(e) if !filter.is_empty()
                && !filter.iter().any(|flt| f.file_name().is_some_and(|n| n.to_string_lossy().contains(flt.as_str()))) =>
            {
                eprintln!("warning: skipping unparsable {}: {e}", f.display());
                continue;
            }
            Err(e) => return Err(anyhow::Error::new(e).context(format!("parsing {}", f.display()))),
        };
        sc.source = f.clone();
        // `_lib` next to the scenario, else the shared one (`os/` scenarios use `../_lib`).
        let parent = f.parent().unwrap_or(Path::new("."));
        let lib = [parent.join("_lib"), parent.join("../_lib")].into_iter().find(|p| p.is_dir()).unwrap_or_else(|| parent.join("_lib"));
        sc.steps = expand_includes(&sc.steps, &lib, 0)?;
        let matches = filter.is_empty()
            || filter.iter().any(|flt| sc.name.contains(flt.as_str()) || sc.tags.iter().any(|t| t == flt));
        if matches {
            out.push(sc);
        }
    }
    Ok(out)
}

fn expand_includes(steps: &[Step], lib: &Path, depth: usize) -> Result<Vec<Step>> {
    if depth > 8 {
        bail!("include nesting too deep");
    }
    let mut out = Vec::new();
    for step in steps {
        match &step.action {
            Action::Include(name) => {
                let path = lib.join(format!("{name}.yaml"));
                let text = std::fs::read_to_string(&path).with_context(|| format!("include {}", path.display()))?;
                let inner: Vec<Step> =
                    serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
                out.extend(expand_includes(&inner, lib, depth + 1)?);
            }
            _ => out.push(step.clone()),
        }
    }
    Ok(out)
}

pub struct Runner<'a> {
    pub app: &'a mut App,
    pub out_dir: PathBuf,
    pub kubeconfig: PathBuf,
}

impl Runner<'_> {
    pub fn run(&mut self, sc: &Scenario) -> ScenarioResult {
        let start = Instant::now();
        let dir = self.out_dir.join(&sc.name);
        let _ = std::fs::create_dir_all(&dir);
        let mut results = Vec::new();
        let mut ok = true;
        for (i, step) in sc.steps.iter().enumerate() {
            let r = self.run_step(sc, i, step, &dir);
            let failed = !r.ok;
            eprintln!(
                "  {} [{:>2}] {}{}",
                if r.ok { "✓" } else { "✗" },
                i,
                r.summary,
                r.error.as_deref().map(|e| format!("  → {e}")).unwrap_or_default()
            );
            results.push(r);
            if failed {
                ok = false;
                // Capture the failure state for the report, then stop this scenario.
                let path = dir.join(format!("{i:02}-FAILURE.png"));
                let _ = self.app.screenshot(&path);
                break;
            }
        }
        let result = ScenarioResult {
            name: sc.name.clone(),
            description: sc.description.clone(),
            ok,
            steps: results,
            millis: start.elapsed().as_millis(),
        };
        let _ = std::fs::write(dir.join("result.json"), serde_json::to_vec_pretty(&result).unwrap_or_default());
        result
    }

    fn run_step(&mut self, sc: &Scenario, index: usize, step: &Step, dir: &Path) -> StepResult {
        let summary = summarize(step);
        let t = Instant::now();
        let mut shot = None;
        let mut data = serde_json::Value::Null;
        let res = self.exec(sc, &step.action, dir, &mut shot, &mut data);
        StepResult {
            index,
            summary,
            ok: res.is_ok(),
            error: res.err().map(|e| format!("{e:#}")),
            millis: t.elapsed().as_millis(),
            screenshot: shot,
            data,
        }
    }

    fn exec(
        &mut self,
        sc: &Scenario,
        action: &Action,
        dir: &Path,
        shot: &mut Option<String>,
        data: &mut serde_json::Value,
    ) -> Result<()> {
        let app = &mut *self.app;
        match action {
            Action::Click(c) | Action::DoubleClick(c) | Action::RightClick(c) => {
                let q = c.query();
                let attempts = if q.until.is_some() { 3 } else { 1 };
                for attempt in 1..=attempts {
                    let (mut x, mut y) = match c {
                        ClickSpec::Id(i) => element_center(app, &i.id, i.timeout)?,
                        _ => app.wait_for(&q.text_query(), Duration::from_secs_f64(q.timeout))?.center(),
                    };
                    if let Some(nx) = q.at_x {
                        x = app.frame.x + nx * app.frame.w;
                    }
                    if let Some([dx, dy]) = q.offset {
                        x += dx;
                        y += dy;
                    }
                    match action {
                        Action::DoubleClick(_) => app.click(x, y, 2, false)?,
                        Action::RightClick(_) => app.click(x, y, 1, true)?,
                        _ => app.click(x, y, 1, false)?,
                    }
                    settle(app, 250);
                    let Some(until) = &q.until else { break };
                    match app.wait_for(&until.as_str().into(), Duration::from_secs(3)) {
                        Ok(_) => break,
                        Err(e) if attempt == attempts => return Err(e.context(format!("after {attempts} clicks"))),
                        Err(_) => {}
                    }
                }
            }
            Action::Hover(c) => {
                let q = c.query();
                let (x, y) = match c {
                    ClickSpec::Id(i) => element_center(app, &i.id, i.timeout)?,
                    _ => app.wait_for(&q.text_query(), Duration::from_secs_f64(q.timeout))?.center(),
                };
                app.hover(x, y)?;
                settle(app, 250);
            }
            Action::ClickAt([x, y]) => {
                app.click_at(*x, *y)?;
                settle(app, 250);
            }
            Action::Key(k) => {
                app.key(k)?;
                settle(app, 200);
            }
            Action::Keys(ks) => {
                for k in ks {
                    app.key(k)?;
                    settle(app, 120);
                }
                settle(app, 150);
            }
            Action::Type(t) => {
                app.type_text(t)?;
                settle(app, 200);
            }
            Action::Wait(w) => {
                let q = w.query();
                app.wait_for(&q.text_query(), Duration::from_secs_f64(q.timeout))?;
            }
            Action::WaitGone(w) => {
                let q = w.query();
                app.wait_gone(&q.text_query(), Duration::from_secs_f64(q.timeout))?;
            }
            Action::Expect(e) | Action::ExpectNot(e) => {
                let want_visible = matches!(action, Action::Expect(_));
                let deadline = Instant::now() + Duration::from_secs_f64(e.timeout);
                loop {
                    let mut wrong = Vec::new();
                    for t in &e.texts {
                        let q = TextQuery { text: t.clone(), within: e.within, nth: 0, exact: false };
                        let visible = if e.crop { !app.find_cropped(&q)?.is_empty() } else { !app.find(&q)?.is_empty() };
                        if visible != want_visible {
                            wrong.push(t.clone());
                        }
                    }
                    if wrong.is_empty() {
                        break;
                    }
                    if Instant::now() > deadline {
                        bail!(
                            "{} {:?}",
                            if want_visible { "not visible:" } else { "unexpectedly visible:" },
                            wrong
                        );
                    }
                    std::thread::sleep(Duration::from_millis(400));
                }
            }
            Action::Screenshot(s) => {
                // Let animations (tab underline springs, fades) settle before capture.
                std::thread::sleep(Duration::from_millis(500));
                let png = dir.join(format!("{}.png", s.name));
                app.screenshot(&png)?;
                let meta = ShotMeta {
                    scenario: sc.name.clone(),
                    name: s.name.clone(),
                    mask: s.mask.clone(),
                    mask_rects: s.mask_rects.clone(),
                    tolerance: s.tolerance,
                };
                std::fs::write(dir.join(format!("{}.json", s.name)), serde_json::to_vec_pretty(&meta)?)?;
                *shot = Some(png.file_name().unwrap().to_string_lossy().into_owned());
            }
            Action::Scroll(s) => {
                let f = app.frame;
                app.scroll_at(f.x + s.at[0] * f.w, f.y + s.at[1] * f.h, s.dy)?;
                settle(app, 250);
            }
            Action::Drag(d) => {
                let f = app.frame;
                app.drag(
                    (f.x + d.from[0] * f.w, f.y + d.from[1] * f.h),
                    (f.x + d.to[0] * f.w, f.y + d.to[1] * f.h),
                )?;
                settle(app, 250);
            }
            Action::Sleep(ms) => std::thread::sleep(Duration::from_millis(*ms)),
            Action::Shell(s) => {
                let (cmd, expect, retry) = match s {
                    ShellSpec::Cmd(c) => (c.clone(), None, 0.0),
                    ShellSpec::Full { cmd, expect_output, retry } => (cmd.clone(), expect_output.clone(), *retry),
                };
                let deadline = Instant::now() + Duration::from_secs_f64(retry);
                // The app's sandboxed HOME (settings, events DB), for steps that seed or inspect state.
                let home = self.out_dir.join("home");
                let home = home.canonicalize().unwrap_or(home);
                loop {
                    let out = Command::new("bash")
                        .args(["-c", &cmd])
                        .env("KUBECONFIG", &self.kubeconfig)
                        .env("HARNESS_APP_HOME", &home)
                        .output()?;
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let good = out.status.success()
                        && expect.as_ref().is_none_or(|e| stdout.contains(e.as_str()));
                    if good {
                        *data = serde_json::json!({ "stdout": stdout });
                        break;
                    }
                    if Instant::now() > deadline {
                        bail!(
                            "shell failed ({}): {cmd}\nstdout: {stdout}\nstderr: {}",
                            out.status,
                            String::from_utf8_lossy(&out.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Action::Element(e) => {
                let re = match &e.label {
                    Some(l) => Some(regex::Regex::new(l).with_context(|| format!("bad label regex {l:?}"))?),
                    None => None,
                };
                let deadline = Instant::now() + Duration::from_secs_f64(e.timeout);
                loop {
                    let snap = app.automation().snapshot()?;
                    // `id` ending in `*` matches any element id with that prefix
                    // (e.g. `toast.*`); the first one whose label matches wins.
                    let id_ok = |id: &str| kore_automation::id_matches(&e.id, id);
                    let label_ok = |l: &str| re.as_ref().map(|re| re.is_match(l)).unwrap_or(true);
                    let candidates: Vec<&String> = snap.elements.iter().filter(|el| id_ok(&el.id)).map(|el| &el.label).collect();
                    let hit = candidates.iter().find(|l| label_ok(l)).map(|l| (*l).clone());
                    let ok = hit.is_some();
                    let found = hit.or_else(|| candidates.first().map(|l| (*l).clone()));
                    if ok {
                        *data = serde_json::json!({ "label": found });
                        break;
                    }
                    if Instant::now() > deadline {
                        bail!("element {}: expected label {:?}, got {:?}", e.id, e.label, found);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            Action::ElementGone(e) => {
                let deadline = Instant::now() + Duration::from_secs_f64(e.timeout);
                loop {
                    let snap = app.automation().snapshot()?;
                    let left: Vec<&str> =
                        snap.elements.iter().filter(|el| kore_automation::id_matches(&e.id, &el.id)).map(|el| el.label.as_str()).collect();
                    if left.is_empty() {
                        break;
                    }
                    if Instant::now() > deadline {
                        bail!("element {} still present (labels {left:?})", e.id);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            Action::State(expected) => {
                let snap = app.automation().snapshot()?;
                for (ptr, want) in expected {
                    let got = snap.state.pointer(ptr).cloned().unwrap_or(serde_json::Value::Null);
                    if &got != want {
                        bail!("state {ptr}: expected {want}, got {got}");
                    }
                }
                *data = snap.state;
            }
            Action::Perf(p) => {
                app.automation().frame_stats(true)?;
                for (i, inner) in p.steps.iter().enumerate() {
                    self.exec(sc, &inner.action, dir, shot, data)
                        .with_context(|| format!("perf inner step {i}"))?;
                }
                let stats = self.app.automation().frame_stats(false)?;
                *data = serde_json::to_value(&stats)?;
                if stats.frames == 0 {
                    bail!("no frames rendered during perf window");
                }
                if stats.p95_ms > p.max_p95_ms {
                    bail!("p95 frame time {:.2}ms > {:.2}ms ({stats:?})", stats.p95_ms, p.max_p95_ms);
                }
                if let Some(r) = p.max_over_budget_ratio {
                    let ratio = stats.over_budget as f64 / stats.frames as f64;
                    if ratio > r {
                        bail!("{:.1}% frames over budget > {:.1}%", ratio * 100.0, r * 100.0);
                    }
                }
            }
            Action::Include(n) => bail!("unexpanded include {n}"),
            Action::RestartApp(r) => {
                app.restart()?;
                if let Some(t) = &r.wait {
                    app.wait_for(&t.as_str().into(), Duration::from_secs(30))?;
                }
            }
            Action::ExpectColor(c) => {
                let colors = c.colors.iter().map(|h| parse_hex(h)).collect::<Result<Vec<_>>>()?;
                let q = TextQuery { text: c.text.clone(), within: c.within, nth: c.nth, exact: c.exact };
                let deadline = Instant::now() + Duration::from_secs_f64(c.timeout);
                loop {
                    let hit = app.find(&q)?.into_iter().nth(q.nth);
                    let (n, seen) = match &hit {
                        Some(h) => (app.count_color_pixels(h, &colors, 1.0, c.max_dist)?, h.text.clone()),
                        None => (0, String::from("<not visible>")),
                    };
                    *data = serde_json::json!({ "text": seen, "matching_pixels": n });
                    if n >= c.min_pixels {
                        break;
                    }
                    if Instant::now() > deadline {
                        bail!("text {:?} ({seen}) has {n} pixels in {:?}, need {}", c.text, c.colors, c.min_pixels);
                    }
                    std::thread::sleep(Duration::from_millis(400));
                }
            }
            Action::WindowTitle(t) => {
                let got = app.window_title()?;
                if &got != t {
                    bail!("window title {got:?} != {t:?}");
                }
            }
        }
        Ok(())
    }
}

/// Center of the first element matching `id`, in the app's coordinate space
/// (Layer 2 window space; Layer 3 screen points).
fn element_center(app: &mut App, id: &str, timeout: f64) -> Result<(f64, f64)> {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    loop {
        let snap = app.automation().snapshot()?;
        if let Some(e) = snap.elements.iter().find(|e| kore_automation::id_matches(id, &e.id)) {
            let (cx, cy) = e.bounds.center();
            return Ok(match app.layer {
                Layer::App => (cx as f64, cy as f64 + crate::driver::TITLEBAR),
                Layer::Os => (snap.window.x as f64 + cx as f64, snap.window.y as f64 + cy as f64),
            });
        }
        if Instant::now() > deadline {
            bail!("no element {id:?} after {timeout}s");
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Pause after an input step. Layer 3 needs real time for the OS event queue;
/// Layer 2 input is synchronous (the response arrives after the frame), so only
/// a short pause for async work (spawned tasks, animations) is needed.
fn settle(app: &App, os_ms: u64) {
    let ms = match app.layer {
        Layer::App => os_ms.min(80),
        Layer::Os => os_ms,
    };
    std::thread::sleep(Duration::from_millis(ms));
}

fn summarize(step: &Step) -> String {
    let s = match &step.action {
        Action::Click(c) => format!("click {:?}", c.query().text),
        Action::DoubleClick(c) => format!("double-click {:?}", c.query().text),
        Action::RightClick(c) => format!("right-click {:?}", c.query().text),
        Action::ClickAt(p) => format!("click at {p:?}"),
        Action::Hover(c) => format!("hover {:?}", c.query().text),
        Action::Key(k) => format!("key {k}"),
        Action::Keys(k) => format!("keys {}", k.join(" ")),
        Action::Type(t) => format!("type {t:?}"),
        Action::Wait(w) => format!("wait for {:?}", w.query().text),
        Action::WaitGone(w) => format!("wait until {:?} gone", w.query().text),
        Action::Expect(e) => format!("expect {:?}", e.texts),
        Action::ExpectNot(e) => format!("expect not {:?}", e.texts),
        Action::Screenshot(s) => format!("screenshot {}", s.name),
        Action::Scroll(s) => format!("scroll {} at {:?}", s.dy, s.at),
        Action::Drag(d) => format!("drag {:?} → {:?}", d.from, d.to),
        Action::Sleep(ms) => format!("sleep {ms}ms"),
        Action::Shell(ShellSpec::Cmd(c)) | Action::Shell(ShellSpec::Full { cmd: c, .. }) => format!("shell {c}"),
        Action::Element(e) => format!("element {} label {:?}", e.id, e.label),
        Action::ElementGone(e) => format!("element {} gone", e.id),
        Action::State(s) => format!("state {:?}", s.iter().map(|(k, _)| k).collect::<Vec<_>>()),
        Action::Perf(p) => format!("perf ({} steps, p95 ≤ {}ms)", p.steps.len(), p.max_p95_ms),
        Action::WindowTitle(t) => format!("window title {t:?}"),
        Action::Include(n) => format!("include {n}"),
        Action::RestartApp(_) => "restart app".to_string(),
        Action::ExpectColor(c) => format!("expect {:?} colored {:?}", c.text, c.colors),
    };
    match &step.note {
        Some(n) => format!("{s} — {n}"),
        None => s,
    }
}
