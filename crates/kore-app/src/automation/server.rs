//! Layer 2 control server (`test-harness` feature, enabled at runtime by
//! `KORE_CONTROL_SOCKET=<path>`). Newline-delimited JSON over a Unix socket; the
//! protocol is `kore_automation::Request` / `Response`.
//!
//! Socket I/O runs on plain threads. Every request is executed on the GPUI
//! foreground executor with access to the window. Input is injected inside the
//! app the way the OS would deliver it:
//! - each platform event is its own top-level update, so its effects (such as an
//!   input's Change event) are flushed;
//! - a frame is drawn before the next event.
//!
//! Nothing touches the OS cursor or keyboard, so the app can be driven while it
//! is in the background, covered or minimized. GPUI stops drawing a window that
//! isn't visible, so the server draws frames itself whenever it needs one.

#[cfg(not(unix))]
compile_error!("the test-harness control server needs Unix domain sockets (macOS / Linux)");

use super::{frames, registry, AutomationHost};
use gpui_kit::*;
use kore_automation::{id_matches, Bounds as ABounds, Condition, Element as AElement, Request, Response, Snapshot};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

type Job = (Request, std::sync::mpsc::Sender<Response>);

pub fn start<V: AutomationHost + Render>(socket: PathBuf, window: AnyWindowHandle, host: Entity<V>, cx: &mut App) {
    registry::enable();
    let (tx, rx) = async_channel::unbounded::<Job>();

    // A stale socket file from a crashed run would make bind fail.
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("control server: cannot bind {}: {e}", socket.display());
            return;
        }
    };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!("control server on {}", socket.display());
    let cleanup = socket.clone();
    cx.on_app_quit(move |_| {
        let _ = std::fs::remove_file(&cleanup);
        async {}
    })
    .detach();

    std::thread::Builder::new()
        .name("kore-control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let mut writer = match stream.try_clone() {
                        Ok(w) => w,
                        Err(_) => return,
                    };
                    for line in BufReader::new(stream).lines() {
                        let Ok(line) = line else { break };
                        let resp = match serde_json::from_str::<Request>(&line) {
                            Ok(req) => {
                                let (rtx, rrx) = std::sync::mpsc::channel();
                                if tx.send_blocking((req, rtx)).is_err() {
                                    break;
                                }
                                rrx.recv().unwrap_or_else(|_| Response::err("app shut down"))
                            }
                            Err(e) => Response::err(format!("bad request: {e}")),
                        };
                        let mut out = serde_json::to_string(&resp).unwrap_or_default();
                        out.push('\n');
                        if writer.write_all(out.as_bytes()).is_err() {
                            break;
                        }
                    }
                });
            }
        })
        .expect("spawn control thread");

    cx.spawn(async move |cx| {
        while let Ok((req, reply)) = rx.recv().await {
            let is_quit = matches!(req, Request::Quit);
            let resp = match execute(req, window, &host, cx).await {
                Ok(data) => Response::Ok { data },
                Err(e) => Response::err(format!("{e:#}")),
            };
            let _ = reply.send(resp);
            if is_quit {
                cx.update(|cx| cx.quit());
            }
        }
    })
    .detach();
}

async fn execute<V: AutomationHost + Render>(
    req: Request,
    window: AnyWindowHandle,
    host: &Entity<V>,
    cx: &mut AsyncApp,
) -> anyhow::Result<serde_json::Value> {
    let null = serde_json::Value::Null;
    match req {
        Request::Ping => Ok(serde_json::json!("pong")),
        Request::Quit => Ok(null),
        Request::FrameStats { reset } => Ok(serde_json::to_value(frames::stats(reset))?),
        Request::Snapshot => {
            let state = cx.update(|cx| host.read(cx).automation_state(cx));
            cx.update(|cx| window.update(cx, |_, window, cx| snapshot(state, window, cx)))?
        }
        Request::Screenshot { path } => cx.update(|cx| {
            window.update(cx, |_, window, cx| -> anyhow::Result<serde_json::Value> {
                force_draw(window, cx);
                let img = window.render_to_image()?;
                img.save(&path)?;
                Ok(serde_json::json!({"width": img.width(), "height": img.height()}))
            })
        })?,
        Request::Action { name, args } => cx.update(|cx| {
            window.update(cx, |_, window, cx| host.update(cx, |host, cx| host.automation_action(&name, &args, window, cx)))
        })?,
        Request::Wait { condition, timeout_ms } => {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                let state = cx.update(|cx| host.read(cx).automation_state(cx));
                let (ok, detail) = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        if !window.is_visible() {
                            force_draw(window, cx);
                        }
                        check(&condition, &state, &registry::last_frame())
                    })
                })??;
                if ok {
                    return Ok(null);
                }
                if Instant::now() > deadline {
                    anyhow::bail!("timed out after {timeout_ms}ms waiting for {condition:?} ({detail})");
                }
                cx.background_executor().timer(Duration::from_millis(50)).await;
            }
        }
        Request::ClickElement { id, count, right, timeout_ms } => {
            let (x, y) = element_center(&id, timeout_ms, window, cx).await?;
            inject(input_events(&Request::Click { x, y, count, right })?, window, cx)?;
            Ok(null)
        }
        Request::ScrollElement { id, dy, timeout_ms } => {
            let (x, y) = element_center(&id, timeout_ms, window, cx).await?;
            inject(input_events(&Request::Scroll { x, y, dy })?, window, cx)?;
            Ok(null)
        }
        input => {
            inject(input_events(&input)?, window, cx)?;
            Ok(null)
        }
    }
}

fn snapshot(state: serde_json::Value, window: &mut Window, cx: &mut App) -> anyhow::Result<serde_json::Value> {
    let visible = window.is_visible();
    // A covered or minimized window stops drawing (GPUI pauses its display
    // link), so the registry would be stale: draw now.
    if !visible {
        force_draw(window, cx);
    }
    // Element bounds are relative to the content area, so report the content
    // area (window frame minus the titlebar) as `window`.
    let b = window.bounds();
    let vp = window.viewport_size();
    let titlebar = (b.size.height - vp.height).max(px(0.));
    let snap = Snapshot {
        title: "Kore".into(),
        window: ABounds { x: b.origin.x.into(), y: (b.origin.y + titlebar).into(), w: vp.width.into(), h: vp.height.into() },
        scale: window.scale_factor(),
        visible,
        state,
        elements: registry::last_frame(),
    };
    Ok(serde_json::to_value(snap)?)
}

fn check(condition: &Condition, state: &serde_json::Value, elements: &[AElement]) -> anyhow::Result<(bool, String)> {
    Ok(match condition {
        Condition::Element { id, label } => {
            let re = label.as_deref().map(regex::Regex::new).transpose()?;
            let found: Vec<&AElement> = elements.iter().filter(|e| id_matches(id, &e.id)).collect();
            let ok = found.iter().any(|e| re.as_ref().is_none_or(|r| r.is_match(&e.label)));
            let labels: Vec<&str> = found.iter().map(|e| e.label.as_str()).collect();
            (ok, format!("matching labels: {labels:?}"))
        }
        Condition::Gone { id } => (!elements.iter().any(|e| id_matches(id, &e.id)), String::new()),
        Condition::State { pointer, equals } => {
            let v = state.pointer(pointer).cloned().unwrap_or(serde_json::Value::Null);
            (&v == equals, format!("{pointer} = {v}"))
        }
    })
}

async fn element_center(id: &str, timeout_ms: u64, window: AnyWindowHandle, cx: &mut AsyncApp) -> anyhow::Result<(f32, f32)> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let found = cx.update(|cx| {
            window.update(cx, |_, window, cx| {
                if !window.is_visible() {
                    force_draw(window, cx);
                }
                registry::last_frame().into_iter().find(|e| id_matches(id, &e.id))
            })
        })?;
        if let Some(e) = found {
            return Ok(e.bounds.center());
        }
        if Instant::now() > deadline {
            anyhow::bail!("no element {id:?} after {timeout_ms}ms");
        }
        cx.background_executor().timer(Duration::from_millis(50)).await;
    }
}

enum InputEvent {
    Platform(PlatformInput),
    Keystroke(Keystroke),
}

/// The platform events an input request stands for.
fn input_events(req: &Request) -> anyhow::Result<Vec<InputEvent>> {
    let none = Modifiers::default();
    let mv = |x: f32, y: f32, pressed_button: Option<MouseButton>| {
        InputEvent::Platform(PlatformInput::MouseMove(MouseMoveEvent { position: point(px(x), px(y)), modifiers: none, pressed_button }))
    };
    Ok(match req {
        Request::Click { x, y, count, right } => {
            let position = point(px(*x), px(*y));
            let button = if *right { MouseButton::Right } else { MouseButton::Left };
            let mut evs = vec![mv(*x, *y, None)];
            for n in 1..=(*count).max(1) as usize {
                evs.push(InputEvent::Platform(PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    modifiers: none,
                    button,
                    click_count: n,
                    first_mouse: false,
                })));
                evs.push(InputEvent::Platform(PlatformInput::MouseUp(MouseUpEvent { position, modifiers: none, button, click_count: n })));
            }
            evs
        }
        Request::MouseMove { x, y } => vec![mv(*x, *y, None)],
        // Delivered in 40px chunks, like a trackpad (and Layer 3's CGEvent scroll):
        // per-event handlers such as the graph's zoom step once per event.
        Request::Scroll { x, y, dy } => {
            const CHUNK: f32 = 40.;
            let mut evs = vec![mv(*x, *y, None)];
            let mut remaining = *dy;
            while remaining.abs() > f32::EPSILON {
                let step = remaining.signum() * remaining.abs().min(CHUNK);
                evs.push(InputEvent::Platform(PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: point(px(*x), px(*y)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-step))),
                    modifiers: none,
                    touch_phase: TouchPhase::Moved,
                })));
                remaining -= step;
            }
            evs
        }
        Request::Drag { from, to } => {
            let button = MouseButton::Left;
            let mut evs = vec![
                mv(from[0], from[1], None),
                InputEvent::Platform(PlatformInput::MouseDown(MouseDownEvent {
                    position: point(px(from[0]), px(from[1])),
                    modifiers: none,
                    button,
                    click_count: 1,
                    first_mouse: false,
                })),
            ];
            const STEPS: usize = 8;
            for i in 1..=STEPS {
                let t = i as f32 / STEPS as f32;
                evs.push(mv(from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, Some(button)));
            }
            evs.push(InputEvent::Platform(PlatformInput::MouseUp(MouseUpEvent {
                position: point(px(to[0]), px(to[1])),
                modifiers: none,
                button,
                click_count: 1,
            })));
            evs
        }
        Request::Key { chord } => {
            let k = Keystroke::parse(&chord.replace('+', "-")).map_err(|e| anyhow::anyhow!("bad chord {chord:?}: {e:?}"))?;
            vec![InputEvent::Keystroke(k)]
        }
        Request::Type { text } => text
            .chars()
            .map(|ch| {
                let key = ch.to_string();
                InputEvent::Keystroke(Keystroke { modifiers: none, key: key.clone(), key_char: Some(key) })
            })
            .collect(),
        other => anyhow::bail!("not an input request: {other:?}"),
    })
}

/// Deliver events one per top-level update, each followed by a frame.
fn inject(events: Vec<InputEvent>, window: AnyWindowHandle, cx: &mut AsyncApp) -> anyhow::Result<()> {
    for ev in events {
        cx.update(|cx| {
            window.update(cx, |_, window, cx| match ev {
                InputEvent::Platform(e) => {
                    window.dispatch_event(e, cx);
                }
                InputEvent::Keystroke(k) => {
                    window.dispatch_keystroke(k, cx);
                }
            })
        })?;
        cx.update(|cx| window.update(cx, |_, window, cx| force_draw(window, cx)))?;
    }
    Ok(())
}

/// Build a fresh frame (layout + paint, refreshing the element registry) without
/// waiting for the display link, which is stopped while the window is not visible.
/// Also run after every injected event: focus changes and other per-frame work
/// apply at the next frame, as they would between two real keystrokes.
fn force_draw(window: &mut Window, cx: &mut App) {
    window.refresh();
    window.draw(cx).clear(cx);
}
