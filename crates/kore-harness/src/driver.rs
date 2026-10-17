//! Drives a running Kore app (built with `--features test-harness`) as a black box.
//!
//! Two layers:
//! - **App (Layer 2, default).** Everything goes through the app's control
//!   socket: input is injected inside the app and screenshots are rendered
//!   off-screen. The app runs in the background and never takes focus; the
//!   person at the machine keeps their mouse and keyboard. Coordinates are
//!   "window space": content points shifted down by a virtual titlebar
//!   ([`TITLEBAR`]), so scenarios see the same geometry as in Layer 3.
//! - **Os (Layer 3).** Real CGEvent input and window-server capture: the app
//!   must be frontmost and the harness owns the mouse and keyboard. Coordinates
//!   are global screen points.
//!
//! Text is located by OCR on the screenshot in both layers (OCR never touches
//! input). The control socket also provides the element registry, view state and
//! frame timings.

use crate::os::{input, ocr, ocr::TextBox, window, window::Frame};
use anyhow::{anyhow, bail, Context, Result};
use kore_automation::{FrameStats, Request, Response, Snapshot, BACKGROUND_ENV, SOCKET_ENV};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The app is always placed at the same frame so screenshots are pixel-comparable across runs.
pub const WINDOW_FRAME: Frame = Frame { x: 40.0, y: 60.0, w: 1400.0, h: 900.0 };

/// Layer 2 mirrors Layer 3's geometry: content is sized like the placed window's
/// content, and screenshots get a titlebar-colored band of this height on top.
/// "Window space" (normalized `within` regions, `click_at`, mask rects, OCR boxes)
/// is therefore identical in both layers; input is shifted to content points
/// when sent to the app.
pub const TITLEBAR: f64 = 28.0;
const TITLEBAR_RGB: [u8; 3] = [12, 18, 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Layer 2: in-app control channel, background window, no OS input.
    #[default]
    App,
    /// Layer 3: real OS input; takes over the mouse and keyboard.
    Os,
}

#[derive(Clone)]
pub struct LaunchConfig {
    pub binary: PathBuf,
    pub kubeconfig: PathBuf,
    /// Isolated HOME for the app (settings, ~/.kore, event DB).
    pub home: PathBuf,
    /// Control socket path (Unix sockets are limited to ~100 bytes: keep it short).
    pub socket: PathBuf,
    pub layer: Layer,
    pub extra_env: Vec<(String, String)>,
    pub log_path: PathBuf,
}

/// A short, unique control socket path for a run (`$TMPDIR/kore-<pid>-<tag>.sock`).
pub fn socket_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kore-{}-{tag}.sock", std::process::id()))
}

pub struct App {
    pub layer: Layer,
    /// None when attached to an app the harness did not launch.
    child: Option<Child>,
    pub pid: i32,
    pub window_id: u32,
    pub frame: Frame,
    automation: AutomationClient,
    #[allow(dead_code)]
    last_ocr: Option<(PathBuf, Vec<TextBox>, f64)>,
    scratch: PathBuf,
    /// Kept so a scenario can restart the app mid-way (same sandbox HOME).
    launch_cfg: Option<LaunchConfig>,
}

/// A located piece of on-screen text, in global screen points.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Hit {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Text query: case-insensitive substring after OCR normalization, or `re:<regex>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextQuery {
    pub text: String,
    /// Restrict to a region of the window, normalized 0..1 `[x0, y0, x1, y1]`.
    #[serde(default)]
    pub within: Option<[f64; 4]>,
    /// Which match to use when several are visible (0-based, top-to-bottom, left-to-right).
    #[serde(default)]
    pub nth: usize,
    #[serde(default)]
    pub exact: bool,
}

impl From<&str> for TextQuery {
    fn from(s: &str) -> Self {
        TextQuery { text: s.to_string(), ..Default::default() }
    }
}

/// OCR confuses glyphs like l/I/1 and O/0 in UI fonts; compare on a folded form.
pub fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '/' | '.' | ':' | '_'))
        .map(|c| match c.to_lowercase().next().unwrap_or(c) {
            'i' | 'l' | '1' | '|' => 'l',
            'o' | '0' => 'o',
            // Vision sometimes reads short Latin words as Cyrillic homoglyphs ("Copy" -> "Соpу").
            'а' => 'a',
            'в' => 'b',
            'с' => 'c',
            'е' => 'e',
            'н' => 'h',
            'к' => 'k',
            'м' => 'm',
            'р' => 'p',
            'т' => 't',
            'х' => 'x',
            'у' => 'y',
            'о' => 'o',
            'і' => 'l',
            c => c,
        })
        .collect()
}

impl TextQuery {
    pub fn matches(&self, candidate: &str) -> bool {
        if let Some(re) = self.text.strip_prefix("re:") {
            return regex::Regex::new(re).map(|r| r.is_match(candidate)).unwrap_or(false);
        }
        let (q, c) = (fold(&self.text), fold(candidate));
        if q.is_empty() {
            return false;
        }
        if self.exact {
            c == q
        } else {
            c.contains(&q)
        }
    }
}

impl App {
    pub fn launch(cfg: &LaunchConfig, scratch: &Path) -> Result<App> {
        std::fs::create_dir_all(&cfg.home)?;
        std::fs::create_dir_all(scratch)?;
        // Always pass an absolute HOME: the app runs with the sandbox as cwd.
        let home = std::fs::canonicalize(&cfg.home)?;
        let log = std::fs::File::create(&cfg.log_path)?;
        let _ = std::fs::remove_file(&cfg.socket);
        let mut cmd = Command::new(&cfg.binary);
        cmd.env("HOME", &home)
            .env("CFFIXED_USER_HOME", &home)
            .env("KUBECONFIG", &cfg.kubeconfig)
            .env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
            .env(SOCKET_ENV, &cfg.socket)
            .current_dir(&cfg.home)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        if cfg.layer == Layer::App {
            cmd.env(BACKGROUND_ENV, "1");
        }
        for (k, v) in &cfg.extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("launching {}", cfg.binary.display()))?;
        let pid = child.id() as i32;

        let automation = match AutomationClient::connect(&cfg.socket, Duration::from_secs(30)) {
            Ok(a) => a,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.context(format!(
                    "no control socket: is {} built with `--features test-harness`? (log: {})",
                    cfg.binary.display(),
                    cfg.log_path.display()
                )));
            }
        };
        let mut app = App {
            layer: cfg.layer,
            child: Some(child),
            pid,
            window_id: 0,
            frame: Frame { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
            automation,
            last_ocr: None,
            scratch: scratch.to_path_buf(),
            launch_cfg: Some(cfg.clone()),
        };
        match cfg.layer {
            Layer::App => {
                let args = serde_json::json!({"width": WINDOW_FRAME.w, "height": WINDOW_FRAME.h - TITLEBAR});
                app.automation.call(&Request::Action { name: "resize".into(), args })?;
                app.refresh_frame()?;
            }
            Layer::Os => {
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    match window::main_window(pid) {
                        Ok(w) if w.frame.w > 200.0 => break,
                        _ if Instant::now() > deadline => bail!("app window did not appear within 30s"),
                        _ => std::thread::sleep(Duration::from_millis(250)),
                    }
                }
                window::place(pid, WINDOW_FRAME)?;
                std::thread::sleep(Duration::from_millis(400));
                app.refresh_frame()?;
                app.focus()?;
            }
        }
        Ok(app)
    }

    /// Attaches to an already running app (for interactive driving / debugging).
    pub fn attach(pid: i32, socket: &Path, layer: Layer, scratch: &Path) -> Result<App> {
        std::fs::create_dir_all(scratch)?;
        let automation = AutomationClient::connect(socket, Duration::from_secs(5))?;
        let mut app = App {
            layer,
            child: None,
            pid,
            window_id: 0,
            frame: Frame { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
            automation,
            last_ocr: None,
            scratch: scratch.to_path_buf(),
            launch_cfg: None,
        };
        app.refresh_frame()?;
        Ok(app)
    }

    /// Quits and relaunches the app with the same config and sandbox (persisted state survives).
    pub fn restart(&mut self) -> Result<()> {
        let cfg = self
            .launch_cfg
            .clone()
            .ok_or_else(|| anyhow!("restart is only possible for apps launched by the harness"))?;
        let _ = self.automation.call(&Request::Quit);
        if let Some(c) = self.child.as_mut() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline && matches!(c.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(100));
            }
            if matches!(c.try_wait(), Ok(None)) {
                let _ = c.kill();
            }
            let _ = c.wait();
        }
        self.child = None;
        // Let the window server forget the old window.
        std::thread::sleep(Duration::from_millis(500));
        let scratch = self.scratch.clone();
        *self = App::launch(&cfg, &scratch)?;
        Ok(())
    }

    /// Fraction-free color probe: counts pixels of the last OCR screenshot inside `hit`
    /// (padded by `pad` points) that are within `max_dist` (RGB euclidean) of any `colors`.
    pub fn count_color_pixels(&mut self, hit: &Hit, colors: &[[u8; 3]], pad: f64, max_dist: f64) -> Result<usize> {
        let (shot, _, scale) = self
            .last_ocr
            .as_ref()
            .ok_or_else(|| anyhow!("no screenshot to sample colors from"))?;
        let img = image::open(shot)?.to_rgb8();
        let x0 = ((hit.x - pad - self.frame.x) * scale).max(0.0) as u32;
        let y0 = ((hit.y - pad - self.frame.y) * scale).max(0.0) as u32;
        let x1 = (((hit.x + hit.w + pad - self.frame.x) * scale) as u32).min(img.width());
        let y1 = (((hit.y + hit.h + pad - self.frame.y) * scale) as u32).min(img.height());
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = img.get_pixel(x, y).0;
                if colors.iter().any(|c| {
                    let d: f64 = (0..3).map(|i| (p[i] as f64 - c[i] as f64).powi(2)).sum();
                    d.sqrt() <= max_dist
                }) {
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// Detach without killing the app.
    pub fn detach(mut self) {
        self.child = None;
    }

    pub fn focus(&mut self) -> Result<()> {
        if self.layer == Layer::App {
            return Ok(());
        }
        window::activate(self.pid)?;
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }

    pub fn refresh_frame(&mut self) -> Result<()> {
        match self.layer {
            Layer::App => {
                // Content coordinates: OCR boxes, clicks and element bounds share one space.
                let snap = self.automation.snapshot()?;
                self.frame = Frame { x: 0.0, y: 0.0, w: snap.window.w as f64, h: snap.window.h as f64 + TITLEBAR };
            }
            Layer::Os => {
                let info = window::main_window(self.pid)?;
                self.window_id = info.id;
                self.frame = info.frame;
            }
        }
        Ok(())
    }

    pub fn screenshot(&mut self, out: &Path) -> Result<()> {
        self.last_ocr = None;
        if self.layer == Layer::App {
            let path = std::path::absolute(out)?;
            self.automation.call(&Request::Screenshot { path: path.to_string_lossy().into_owned() })?;
            // Add the titlebar band so the image has Layer 3's geometry.
            let content = image::open(&path)?.to_rgba8();
            let band = (TITLEBAR * content.width() as f64 / self.frame.w).round() as u32;
            let [r, g, b] = TITLEBAR_RGB;
            let mut img = image::RgbaImage::from_pixel(content.width(), content.height() + band, image::Rgba([r, g, b, 255]));
            image::imageops::overlay(&mut img, &content, 0, band as i64);
            img.save(&path)?;
            return Ok(());
        }
        self.refresh_frame()?;
        // screencapture occasionally fails transiently (window server busy); retry once.
        if window::capture(self.window_id, out).is_err() {
            std::thread::sleep(Duration::from_millis(500));
            self.refresh_frame()?;
            window::capture(self.window_id, out)?;
        }
        Ok(())
    }

    /// Screenshot + OCR. Boxes are returned in global screen points.
    pub fn read_screen(&mut self) -> Result<Vec<Hit>> {
        let shot = self.scratch.join("ocr.png");
        self.screenshot(&shot)?;
        let boxes = ocr::recognize(&shot)?;
        let (img_w, _) = image::image_dimensions(&shot)?;
        let scale = img_w as f64 / self.frame.w;
        let hits = boxes
            .iter()
            .map(|b| Hit {
                text: b.text.clone(),
                x: self.frame.x + b.x / scale,
                y: self.frame.y + b.y / scale,
                w: b.w / scale,
                h: b.h / scale,
            })
            .collect();
        self.last_ocr = Some((shot, boxes, scale));
        Ok(hits)
    }

    /// Like [`find`] but OCRs only the `within` region (padded by 2% of the
    /// window): full-window Vision passes sometimes drop lines that a crop reads.
    pub fn find_cropped(&mut self, q: &TextQuery) -> Result<Vec<Hit>> {
        let Some([x0, y0, x1, y1]) = q.within else { return self.find(q) };
        let shot = self.scratch.join("ocr.png");
        self.screenshot(&shot)?;
        let img = image::open(&shot)?;
        let (iw, ih) = (img.width() as f64, img.height() as f64);
        let pad = 0.02;
        let (cx0, cy0) = (((x0 - pad).max(0.) * iw) as u32, ((y0 - pad).max(0.) * ih) as u32);
        let (cx1, cy1) = (((x1 + pad).min(1.) * iw) as u32, ((y1 + pad).min(1.) * ih) as u32);
        let crop = img.crop_imm(cx0, cy0, cx1.saturating_sub(cx0).max(1), cy1.saturating_sub(cy0).max(1));
        let crop_path = self.scratch.join("ocr-crop.png");
        crop.save(&crop_path)?;
        let boxes = ocr::recognize(&crop_path)?;
        let scale = iw / self.frame.w;
        let frame = self.frame;
        let hits: Vec<Hit> = boxes
            .iter()
            .map(|b| Hit {
                text: b.text.clone(),
                x: frame.x + (b.x + cx0 as f64) / scale,
                y: frame.y + (b.y + cy0 as f64) / scale,
                w: b.w / scale,
                h: b.h / scale,
            })
            .filter(|h| q.matches(&h.text))
            .filter(|h| {
                let (cx, cy) = h.center();
                let nx = (cx - frame.x) / frame.w;
                let ny = (cy - frame.y) / frame.h;
                nx >= x0 && nx <= x1 && ny >= y0 && ny <= y1
            })
            .collect();
        Ok(hits)
    }

    pub fn find(&mut self, q: &TextQuery) -> Result<Vec<Hit>> {
        let frame = self.frame;
        let mut hits: Vec<Hit> = self
            .read_screen()?
            .into_iter()
            .filter(|h| q.matches(&h.text))
            .filter(|h| match q.within {
                None => true,
                Some([x0, y0, x1, y1]) => {
                    let (cx, cy) = h.center();
                    let nx = (cx - frame.x) / frame.w;
                    let ny = (cy - frame.y) / frame.h;
                    nx >= x0 && nx <= x1 && ny >= y0 && ny <= y1
                }
            })
            .collect();
        hits.sort_by(|a, b| {
            (a.y.round() as i64 / 4, a.x as i64).cmp(&(b.y.round() as i64 / 4, b.x as i64))
        });
        Ok(hits)
    }

    pub fn wait_for(&mut self, q: &TextQuery, timeout: Duration) -> Result<Hit> {
        let deadline = Instant::now() + timeout;
        loop {
            let hits = self.find(q)?;
            if let Some(h) = hits.into_iter().nth(q.nth) {
                return Ok(h);
            }
            if Instant::now() > deadline {
                bail!("text {:?} not visible after {:?}", q.text, timeout);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    pub fn wait_gone(&mut self, q: &TextQuery, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.find(q)?.is_empty() {
                return Ok(());
            }
            if Instant::now() > deadline {
                bail!("text {:?} still visible after {:?}", q.text, timeout);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    pub fn click_text(&mut self, q: &TextQuery, timeout: Duration, count: i64) -> Result<Hit> {
        let hit = self.wait_for(q, timeout)?;
        let (x, y) = hit.center();
        self.click(x, y, count, false)?;
        Ok(hit)
    }

    /// Click at a point in this layer's coordinates (content points / screen points).
    pub fn click(&mut self, x: f64, y: f64, count: i64, right: bool) -> Result<()> {
        match self.layer {
            Layer::App => {
                let y = y - TITLEBAR;
                self.automation.call(&Request::Click { x: x as f32, y: y as f32, count: count.max(1) as u32, right })?;
                Ok(())
            }
            Layer::Os => {
                self.focus_if_needed()?;
                if right {
                    input::right_click(x, y)
                } else {
                    input::click(x, y, count)
                }
            }
        }
    }

    /// Move the mouse to a point (hover).
    pub fn hover(&mut self, x: f64, y: f64) -> Result<()> {
        match self.layer {
            Layer::App => self.automation.call(&Request::MouseMove { x: x as f32, y: (y - TITLEBAR) as f32 }).map(|_| ()),
            Layer::Os => {
                self.focus_if_needed()?;
                input::move_to(x, y)
            }
        }
    }

    /// Click at a point relative to the window, normalized 0..1.
    pub fn click_at(&mut self, nx: f64, ny: f64) -> Result<()> {
        self.click(self.frame.x + nx * self.frame.w, self.frame.y + ny * self.frame.h, 1, false)
    }

    pub fn key(&mut self, chord: &str) -> Result<()> {
        match self.layer {
            Layer::App => self.automation.call(&Request::Key { chord: chord.to_string() }).map(|_| ()),
            Layer::Os => {
                self.focus_if_needed()?;
                input::key(chord)
            }
        }
    }

    pub fn type_text(&mut self, text: &str) -> Result<()> {
        match self.layer {
            Layer::App => self.automation.call(&Request::Type { text: text.to_string() }).map(|_| ()),
            Layer::Os => {
                self.focus_if_needed()?;
                input::type_text(text)
            }
        }
    }

    pub fn scroll_at(&mut self, x: f64, y: f64, dy: i32) -> Result<()> {
        match self.layer {
            Layer::App => self.automation.call(&Request::Scroll { x: x as f32, y: (y - TITLEBAR) as f32, dy: dy as f32 }).map(|_| ()),
            Layer::Os => {
                self.focus_if_needed()?;
                input::scroll(x, y, dy)
            }
        }
    }

    pub fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> Result<()> {
        match self.layer {
            Layer::App => self
                .automation
                .call(&Request::Drag {
                    from: [from.0 as f32, (from.1 - TITLEBAR) as f32],
                    to: [to.0 as f32, (to.1 - TITLEBAR) as f32],
                })
                .map(|_| ()),
            Layer::Os => {
                self.focus_if_needed()?;
                input::drag(from, to)
            }
        }
    }

    fn focus_if_needed(&mut self) -> Result<()> {
        // Re-activating on every event is slow; only do it if something stole focus.
        let front = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to unix id of first process whose frontmost is true"])
            .output()?;
        let front_pid: i32 = String::from_utf8_lossy(&front.stdout).trim().parse().unwrap_or(-1);
        if front_pid != self.pid {
            self.focus()?;
        }
        Ok(())
    }

    pub fn window_title(&mut self) -> Result<String> {
        match self.layer {
            Layer::App => Ok(self.automation.snapshot()?.title),
            Layer::Os => window::ax_window_title(self.pid),
        }
    }

    pub fn automation(&mut self) -> &mut AutomationClient {
        &mut self.automation
    }

    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => self.automation.call(&Request::Ping).is_ok(),
        }
    }

    pub fn quit(mut self) -> Result<()> {
        let _ = self.automation.call(&Request::Quit);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        Ok(())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            if matches!(c.try_wait(), Ok(None)) {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

pub struct AutomationClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl AutomationClient {
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(s) => break s,
                Err(e) if Instant::now() > deadline => {
                    bail!("control socket {} unavailable: {e}", socket.display())
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        };
        // Waits run in-app (up to their own timeout), so allow long reads.
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        let mut client = AutomationClient { reader: BufReader::new(stream.try_clone()?), writer: stream };
        client.call(&Request::Ping)?;
        Ok(client)
    }

    pub fn call(&mut self, req: &Request) -> Result<serde_json::Value> {
        let mut line = serde_json::to_string(req)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        let mut resp = String::new();
        self.reader.read_line(&mut resp)?;
        if resp.is_empty() {
            bail!("automation socket closed");
        }
        match serde_json::from_str::<Response>(&resp)? {
            Response::Ok { data } => Ok(data),
            Response::Error { message } => bail!("automation error: {message}"),
        }
    }

    pub fn snapshot(&mut self) -> Result<Snapshot> {
        Ok(serde_json::from_value(self.call(&Request::Snapshot)?)?)
    }

    pub fn frame_stats(&mut self, reset: bool) -> Result<FrameStats> {
        Ok(serde_json::from_value(self.call(&Request::FrameStats { reset })?)?)
    }
}
