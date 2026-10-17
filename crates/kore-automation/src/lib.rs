//! Wire protocol of Kore's Layer 2 control server.
//!
//! The server exists only in builds with the `test-harness` cargo feature, and
//! starts only when `KORE_CONTROL_SOCKET` names a Unix socket path. Transport is
//! newline-delimited JSON: one [`Request`] per line in, one [`Response`] per line
//! out, strictly in order.
//!
//! Input is injected inside the app (GPUI event dispatch), never through the OS,
//! so the app can be driven while it is in the background, covered or minimized.

use serde::{Deserialize, Serialize};

pub const SOCKET_ENV: &str = "KORE_CONTROL_SOCKET";
/// Set (to anything) to open the window without focusing or activating the app.
pub const BACKGROUND_ENV: &str = "KORE_BACKGROUND";

/// Rectangle in window content coordinates (logical points, origin top-left of the content area).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bounds {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// An interactive or text-bearing element registered by the UI during paint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    /// Stable id, e.g. `sidebar.kind.pods`, `table.row.kore-demo/web-abc`, `tab.logs`.
    pub id: String,
    /// `button`, `row`, `tab`, `textbox`, `text`, `menuitem`, `toggle`, ...
    pub role: String,
    pub label: String,
    pub bounds: Bounds,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub disabled: bool,
}

/// `id` matching used by every element-targeted request: exact, or a prefix when it ends with `*`.
pub fn id_matches(pattern: &str, id: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => id.starts_with(prefix),
        None => id == pattern,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    /// Window, view state and the element registry (a fresh frame is drawn first
    /// when the window is not visible).
    Snapshot,
    /// Click at window content coordinates.
    Click {
        x: f32,
        y: f32,
        #[serde(default = "one")]
        count: u32,
        #[serde(default)]
        right: bool,
    },
    /// Click the center of the first element matching `id` (waits up to `timeout_ms` for it).
    ClickElement {
        id: String,
        #[serde(default = "one")]
        count: u32,
        #[serde(default)]
        right: bool,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Move the mouse to window content coordinates (hover).
    MouseMove { x: f32, y: f32 },
    /// Wheel scroll at window content coordinates; positive `dy` scrolls content down.
    Scroll { x: f32, y: f32, dy: f32 },
    /// Wheel scroll over the center of the first element matching `id`.
    ScrollElement {
        id: String,
        dy: f32,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Press at `from`, move in steps, release at `to` (window content coordinates).
    Drag { from: [f32; 2], to: [f32; 2] },
    /// A chord such as `cmd-k`, `shift-enter`, `escape` (`+` is accepted as separator).
    Key { chord: String },
    /// Text typed character by character into whatever has focus.
    Type { text: String },
    /// Dispatch a named app action, e.g. `navigate` with `{"view":"pods"}`.
    Action {
        name: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Block (in-app) until `condition` holds or `timeout_ms` passes (error).
    Wait {
        condition: Condition,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Render the window off-screen (works while it is covered, minimized or
    /// unfocused) and write a PNG to `path`. Returns `{width, height}` in device pixels.
    Screenshot { path: String },
    /// Frame timing collected since the last reset.
    FrameStats {
        #[serde(default)]
        reset: bool,
    },
    Quit,
}

/// What [`Request::Wait`] waits for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    /// An element matching `id` exists; if `label` is given, its label matches that regex.
    Element {
        id: String,
        #[serde(default)]
        label: Option<String>,
    },
    /// No element matches `id`.
    Gone { id: String },
    /// The view state value at JSON pointer `pointer` equals `equals`.
    State { pointer: String, equals: serde_json::Value },
}

fn one() -> u32 {
    1
}

fn default_timeout_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FrameStats {
    pub frames: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    /// Frames whose draw time exceeded 16.7ms (a dropped frame at 60Hz).
    pub over_budget: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub title: String,
    /// Window content bounds in global screen points.
    pub window: Bounds,
    pub scale: f32,
    /// Whether the window is currently visible on screen (not covered, minimized or hidden).
    #[serde(default)]
    pub visible: bool,
    /// App-defined view state (current view, kind, namespace, selection, open dialogs...).
    pub state: serde_json::Value,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok {
        #[serde(default)]
        data: serde_json::Value,
    },
    Error {
        message: String,
    },
}

impl Response {
    pub fn ok(data: impl Serialize) -> Self {
        Response::Ok { data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null) }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Response::Error { message: message.into() }
    }
}
