//! ResourceDetailsView (spec 02 §12): every kind other than pods and
//! deployments. Header (Back/Esc, Delete D, Name/Namespace/Kind grid), tabs
//! Describe/YAML/Events (keys 1–3), secret reveal bar, delete confirm.

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::resource_item::ResourceItem;
use crate::state::types::{KindExt, ResourceKind};
use crate::state::{AppEvent, AppState};
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::toast::{toast, ToastKind};
use crate::views::describe_content::DescribeContent;
use crate::views::detail_ui::{
    btn_icon, button, describe_skeleton, enter_anim, fade_in, info_cell, kbd_chip, tabs_bar, BtnStyle, TabSpec,
};
use crate::views::events_timeline::EventsTimeline;
use crate::views::yaml_editor::YamlEditor;
use base64::Engine;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::rc::Rc;

/// How long a describe may take before it is re-requested.
const DESCRIBE_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(4);

const TABS: [&str; 3] = ["describe", "yaml", "events"];

/// `atob` → binary string (each byte one char); `None` if not valid base64.
pub fn atob(s: &str) -> Option<String> {
    let cleaned: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    engine.decode(cleaned).ok().map(|b| b.into_iter().map(|c| c as char).collect())
}

/// `decodeSecretValues`: hidden keys → `***`, revealed → decoded value.
pub fn decode_secret_values(data: &Value, revealed: &BTreeSet<String>) -> Value {
    let mut out = data.clone();
    if let Some(map) = data.get("data").and_then(Value::as_object) {
        let mut decoded = serde_json::Map::new();
        for (k, v) in map {
            let raw = v.as_str().unwrap_or_default();
            let shown = if revealed.contains(k) { atob(raw).unwrap_or_else(|| raw.to_string()) } else { "***".into() };
            decoded.insert(k.clone(), Value::String(shown));
        }
        out["data"] = Value::Object(decoded);
    }
    out
}

/// JSON.stringify(v, null, 2).
pub fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Back button of the deployment/generic views: `ArrowLeft 16 · Back · Kbd Esc`.
pub fn back_button(id: &'static str, cx: &App) -> Stateful<Div> {
    button(id, BtnStyle::Standard, cx)
        .gap(px(8.))
        .px(px(12.))
        .py(px(6.))
        .text_sm_tw()
        .child(btn_icon("arrow-left", 16., slate::S300))
        .child("Back")
        .child(kbd_chip("Esc"))
}

pub struct ResourceDetailsView {
    app: Entity<AppState>,
    kind: ResourceKind,
    item: ResourceItem,
    namespace: String,
    tab: usize,
    prev_tab: usize,
    describe: Option<Entity<DescribeContent>>,
    raw: Option<Value>,
    loading: bool,
    revealed: BTreeSet<String>,
    yaml: Option<Entity<YamlEditor>>,
    events: Option<Entity<EventsTimeline>>,
    _load: Option<Task<()>>,
    _watchdog: Option<Task<()>>,
    load_gen: u64,
    _subs: Vec<Subscription>,
}

impl ResourceDetailsView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (kind, item) = {
            let a = app.read(cx);
            (a.kind, a.selected.clone().unwrap_or_default())
        };
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let subs = vec![
            cx.subscribe(&app, |_, app, ev: &AppEvent, cx| {
                if matches!(ev, AppEvent::Escape) {
                    app.update(cx, |s, cx| s.back(cx));
                }
            }),
            keys::on_window_key(cx, Self::on_key),
        ];
        let mut this = ResourceDetailsView {
            app,
            kind,
            item,
            namespace,
            tab: 0,
            prev_tab: 0,
            describe: None,
            raw: None,
            loading: true,
            revealed: BTreeSet::new(),
            yaml: None,
            events: None,
            _load: None,
            _watchdog: None,
            load_gen: 0,
            _subs: subs,
        };
        this.enter_tab(0, window, cx);
        this
    }

    fn on_key(&mut self, k: &keys::Key, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keys::text_entry_focused(window) {
            return false;
        }
        let (confirm_open, modal) = {
            let a = self.app.read(cx);
            (a.confirm.is_some(), a.modal_open())
        };
        if k.key == "d" && !confirm_open && !modal {
            self.request_delete(cx);
            return true;
        }
        if k.plain() && !k.shift {
            if let Some(i) = ["1", "2", "3"].iter().position(|d| *d == k.key) {
                self.enter_tab(i, window, cx);
                return true;
            }
        }
        false
    }

    fn enter_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i != self.tab {
            self.prev_tab = self.tab;
        }
        self.tab = i;
        // Drop inactive panes.
        self.yaml = None;
        self.events = None;
        // DescribeContent listens for ⌘F at window level: only keep it while shown.
        self.describe = None;
        self._load = None;
        self._watchdog = None;
        let (kind, ns, name) = (self.kind, self.namespace.clone(), self.item.name.clone());
        match TABS[i] {
            "describe" => {
                // Re-fetched every time the tab is activated.
                self.loading = true;
                self.describe = None;
                self.start_describe(0, cx);
            }
            "yaml" => {
                let app = self.app.clone();
                self.yaml = Some(cx.new(|cx| YamlEditor::new(app, kind, ns, name, window, cx)));
            }
            _ => {
                let app = self.app.clone();
                self.events = Some(cx.new(|cx| EventsTimeline::new(app, kind.as_str().to_string(), ns, name, window, cx)));
            }
        }
        cx.notify();
    }

    /// Fetch the describe JSON. A watchdog re-issues the request if no result
    /// arrived within [`DESCRIBE_WATCHDOG`] (an intermittent never-resolving
    /// describe was seen in full-suite runs and could not be reproduced in
    /// 150+ instrumented attempts; see the Task 4 report).
    fn start_describe(&mut self, attempt: u32, cx: &mut Context<Self>) {
        self.load_gen += 1;
        let gen = self.load_gen;
        let (kind, ns, name) = (self.kind, self.namespace.clone(), self.item.name.clone());
        self._load = Some(backend::run(
            cx,
            move |core| async move { core.describe_resource(kind, ns, name).await },
            move |this: &mut Self, res, cx| {
                if this.load_gen != gen {
                    return;
                }
                this._watchdog = None;
                let text = match res {
                    Ok(v) => {
                        let t = if this.kind == ResourceKind::Secrets {
                            pretty(&decode_secret_values(&v, &this.revealed))
                        } else {
                            pretty(&v)
                        };
                        this.raw = Some(v);
                        t
                    }
                    Err(e) => format!("Error: {}", backend::format_error(&e)),
                };
                this.loading = false;
                this.set_describe(text, cx);
            },
        ));
        let timer = backend::sleep(cx, DESCRIBE_WATCHDOG);
        self._watchdog = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| {
                if this.load_gen == gen && this.loading && attempt < 3 {
                    tracing::warn!(kind = this.kind.as_str(), name = %this.item.name, attempt, "describe did not return in time; retrying");
                    this.start_describe(attempt + 1, cx);
                }
            })
            .ok();
        }));
    }

    fn set_describe(&mut self, text: String, cx: &mut Context<Self>) {
        match &self.describe {
            Some(d) => d.update(cx, |d, cx| d.set_content(text, cx)),
            None => {
                let d = cx.new(|cx| DescribeContent::new(text, cx));
                self.describe = Some(d);
            }
        }
        cx.notify();
    }

    fn toggle_secret(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.revealed.remove(&key) {
            self.revealed.insert(key);
        }
        if let Some(raw) = &self.raw {
            let t = pretty(&decode_secret_values(raw, &self.revealed));
            self.set_describe(t, cx);
        }
    }

    fn request_delete(&mut self, cx: &mut Context<Self>) {
        let (kind, ns, name) = (self.kind, self.namespace.clone(), self.item.name.clone());
        let app = self.app.clone();
        let title = format!("Delete {}", kind.singular_label());
        let message = format!("Are you sure you want to delete \"{name}\" in namespace \"{ns}\"? This action cannot be undone.");
        self.app.update(cx, |s, cx| {
            s.confirm(
                ConfirmRequest {
                    title,
                    message,
                    confirm_label: "Delete".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Danger,
                    on_confirm: Rc::new(move |_, cx| {
                        let core = backend::core(cx);
                        let (app, ns, name) = (app.clone(), ns.clone(), name.clone());
                        cx.spawn(async move |cx| {
                            let res = core.delete_resource(kind, ns, name).await;
                            cx.update(|cx| match res {
                                Ok(()) => {
                                    app.update(cx, |s, cx| s.close_confirm(cx));
                                    toast(cx, "Resource deleted", ToastKind::Success);
                                    app.update(cx, |s, cx| s.back(cx));
                                }
                                Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                            });
                        })
                        .detach();
                    }),
                },
                cx,
            )
        });
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .p(px(16.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .mb(px(12.))
                    .child(
                        back_button("details-back", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.back(cx))))
                            .auto_id("details.back", "button", "Back"),
                    )
                    .child(
                        button("details-delete", BtnStyle::Danger, cx)
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(6.))
                            .text_sm_tw()
                            .child(btn_icon("trash-2", 16., red::R400))
                            .child("Delete")
                            .child(kbd_chip("D"))
                            .on_click(cx.listener(|this, _, _, cx| this.request_delete(cx)))
                            .auto_id("details.delete", "button", "Delete"),
                    ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap(px(16.))
                    .text_sm_tw()
                    .child(info_cell("Name", self.item.name.clone()))
                    .child(info_cell("Namespace", self.namespace.clone()))
                    .child(info_cell("Kind", self.kind.header_label())),
            )
    }

    fn describe_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let keys: Vec<String> = if self.kind == ResourceKind::Secrets {
            self.raw
                .as_ref()
                .and_then(|r| r.get("data"))
                .and_then(Value::as_object)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let body: AnyElement = match (&self.describe, self.loading) {
            (Some(d), false) => div().flex_1().min_h_0().child(d.clone()).into_any_element(),
            _ => describe_skeleton("res-describe", &[128., 112.], &[&[192., 224., 160.], &[256., 208.]]).into_any_element(),
        };
        div()
            .absolute()
            .top(px(16.))
            .left(px(16.))
            .right(px(16.))
            .bottom(px(16.))
            .flex()
            .flex_col()
            .bg(theme::hexa(0x101828, 0.3))
            .border_1()
            .border_color(slate::S800)
            .rounded(px(8.))
            .overflow_hidden()
            .when(!keys.is_empty(), |d| {
                let mut bar = div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(8.))
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(12.))
                    .flex_none()
                    .border_b_1()
                    .border_color(theme::hexa(0x1e293b, 0.5))
                    .child(
                        div()
                            .self_center()
                            .text_10()
                            .text_color(slate::S500)
                            .mr(px(4.))
                            .child("REVEAL:"),
                    );
                for k in keys {
                    let shown = self.revealed.contains(&k);
                    let key = k.clone();
                    let id: SharedString = format!("reveal-{k}").into();
                    bar = bar.child(
                        div()
                            .id(ElementId::Name(id))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(4.))
                            .text_xs_tw()
                            .border_1()
                            .cursor_pointer()
                            .map(|d| {
                                if shown {
                                    d.border_color(theme::PREFLIGHT_BORDER).text_color(accent)
                                } else {
                                    d.border_color(slate::S700).text_color(slate::S400).hover(|s| s.border_color(slate::S600))
                                }
                            })
                            .child(btn_icon(if shown { "eye-off" } else { "eye" }, 12., if shown { accent } else { slate::S400 }))
                            .child(k.clone())
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_secret(key.clone(), cx)))
                            .auto_id(format!("reveal.{k}"), "button", k.clone())
                            .selected(shown),
                    );
                }
                d.child(bar)
            })
            .child(body)
    }
}

impl Render for ResourceDetailsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = [
            TabSpec { id: "describe", label: "Describe", dot: false },
            TabSpec { id: "yaml", label: "YAML", dot: false },
            TabSpec { id: "events", label: "Events", dot: false },
        ];
        let this = cx.entity().downgrade();
        let bar = tabs_bar(&tabs, self.tab, self.prev_tab, window, cx, move |i, window, cx| {
            this.update(cx, |v, cx| v.enter_tab(i, window, cx)).ok();
        });
        let content: AnyElement = match TABS[self.tab] {
            "describe" => self.describe_pane(cx).into_any_element(),
            "yaml" => self.yaml.clone().map(|y| y.into_any_element()).unwrap_or_else(|| div().into_any_element()),
            _ => self.events.clone().map(|e| e.into_any_element()).unwrap_or_else(|| div().into_any_element()),
        };
        let header = self.header(cx);
        enter_anim(
            "resource-details-enter",
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(theme::BACKGROUND)
                .child(header)
                .child(bar)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .overflow_hidden()
                        .child(fade_in(format!("res-tab-{}", self.tab), content)),
                ),
        )
        .auto_id("view.details", "region", self.item.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{atob, decode_secret_values};
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn secret_decode() {
        assert_eq!(atob("a29yZS1oYXJuZXNzLXRva2Vu").as_deref(), Some("kore-harness-token"));
        assert_eq!(atob("!!"), None);
        let v = json!({"data": {"A": "aGk=", "B": "eA=="}});
        let mut r = BTreeSet::new();
        r.insert("A".to_string());
        let d = decode_secret_values(&v, &r);
        assert_eq!(d["data"]["A"], "hi");
        assert_eq!(d["data"]["B"], "***");
    }
}
