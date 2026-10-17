//! RbacIdentityPicker (spec 03 §7.7): trigger button + glass dropdown with a
//! search input and Service Accounts / Users / Groups sections.

use crate::automation::Automation;
use crate::state::AppState;
use crate::theme::{self, misc, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::scrollbar;
use crate::ui::text::caps;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::rbac::{RbacIdentity, RbacIdentityList};
use std::rc::Rc;

pub const PURPLE400: u32 = 0xc084fc;

/// `ns/name` for service accounts, else the name.
pub fn identity_label(i: &RbacIdentity) -> String {
    match i {
        RbacIdentity::ServiceAccount { name, namespace } => format!("{namespace}/{name}"),
        RbacIdentity::User { name } | RbacIdentity::Group { name } => name.clone(),
    }
}

/// Leading icon (name, color) by identity kind.
pub fn identity_icon(i: &RbacIdentity) -> (&'static str, Rgba) {
    match i {
        RbacIdentity::ServiceAccount { .. } => ("box", theme::hex(0x34d399)),
        RbacIdentity::User { .. } => ("user", misc::BLUE400),
        RbacIdentity::Group { .. } => ("users", theme::hex(PURPLE400)),
    }
}

/// `*:focus-visible` ring on focused inputs: 2px accent@40% outside the box,
/// radius forced to 4px. An overlay (a GPUI box-shadow would also fill the
/// transparent input). The parent must be `relative()`.
pub fn focus_ring(cx: &App) -> Div {
    div()
        .absolute()
        .top(px(-2.))
        .left(px(-2.))
        .right(px(-2.))
        .bottom(px(-2.))
        .rounded(px(6.))
        .border_2()
        .border_color(theme::Theme::get(cx).focus_ring())
}

/// Filtered identity lists (SAs by name or namespace; users/groups by name).
pub fn filter_identities(list: &RbacIdentityList, q: &str) -> (Vec<RbacIdentity>, Vec<RbacIdentity>, Vec<RbacIdentity>) {
    let q = q.to_lowercase();
    let sas = list
        .service_accounts
        .iter()
        .filter(|s| s.name.to_lowercase().contains(&q) || s.namespace.to_lowercase().contains(&q))
        .map(|s| RbacIdentity::ServiceAccount { name: s.name.clone(), namespace: s.namespace.clone() })
        .collect();
    let users = list.users.iter().filter(|u| u.to_lowercase().contains(&q)).map(|u| RbacIdentity::User { name: u.clone() }).collect();
    let groups = list.groups.iter().filter(|g| g.to_lowercase().contains(&q)).map(|g| RbacIdentity::Group { name: g.clone() }).collect();
    (sas, users, groups)
}

pub enum PickerEvent {
    Select(RbacIdentity),
}

pub struct IdentityPicker {
    id: SharedString,
    app: WeakEntity<AppState>,
    pub identities: Option<Rc<RbacIdentityList>>,
    pub selected: Option<RbacIdentity>,
    open: bool,
    query: String,
    input: Entity<TextInput>,
    trigger_bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _subs: Vec<Subscription>,
}

impl EventEmitter<PickerEvent> for IdentityPicker {}

impl IdentityPicker {
    pub fn new(id: &str, app: WeakEntity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "Search identities...",
                InputStyle {
                    text_color: slate::S200,
                    placeholder_color: slate::S600,
                    font_size: 14.,
                    line_height: 20.,
                    padding: (28., 12., 6., 6.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let sub = cx.subscribe(&input, |this: &mut Self, _, ev: &InputEvent, cx| {
            if let InputEvent::Change(q) = ev {
                this.query = q.clone();
                cx.notify();
            }
        });
        IdentityPicker {
            id: id.to_string().into(),
            app,
            identities: None,
            selected: None,
            open: false,
            query: String::new(),
            input,
            trigger_bounds: Default::default(),
            scroll: ScrollHandle::new(),
            gutter: Default::default(),
            _subs: vec![sub],
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Closes the dropdown (Esc; quirk 10.22, fixed).
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.set_open(false, window, cx);
        }
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        let id = format!("rbac-picker:{}", self.id);
        if let Some(app) = self.app.upgrade() {
            cx.defer(move |cx| app.update(cx, |s, cx| s.set_modal(&id, open, cx)));
        }
        if open {
            self.scroll.set_offset(point(px(0.), px(0.)));
            let input = self.input.clone();
            input.update(cx, |i, cx| i.focus(window, cx));
        } else {
            self.query.clear();
            self.input.update(cx, |i, cx| i.set_text("", cx));
            if self.input.read(cx).is_focused(window) {
                window.blur(cx);
            }
        }
        cx.notify();
    }

    fn select(&mut self, ident: RbacIdentity, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PickerEvent::Select(ident));
        self.set_open(false, window, cx);
    }

    fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.input.read(cx).is_focused(window);
        let tb = self.trigger_bounds.clone();
        let (sas, users, groups) = match &self.identities {
            Some(l) => filter_identities(l, &self.query),
            None => (vec![], vec![], vec![]),
        };
        let sel = self.selected.clone();
        let id = self.id.clone();
        let section = |title: &str, items: Vec<RbacIdentity>, cx: &mut Context<Self>| -> Option<AnyElement> {
            if items.is_empty() {
                return None;
            }
            Some(
                div()
                    .p(px(4.))
                    .child(div().px(px(8.)).py(px(4.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps(title, 10.)))
                    .children(items.into_iter().map(|ident| {
                        let label = identity_label(&ident);
                        let (ic, color) = identity_icon(&ident);
                        let is_sel = sel.as_ref() == Some(&ident);
                        let text: AnyElement = match &ident {
                            RbacIdentity::ServiceAccount { name, namespace } => div()
                                .flex()
                                .flex_row()
                                .min_w_0()
                                .overflow_hidden()
                                .child(div().text_color(slate::S500).child(format!("{namespace}/")))
                                .child(div().truncate().child(name.clone()))
                                .into_any_element(),
                            _ => div().truncate().child(label.clone()).into_any_element(),
                        };
                        let pick = ident.clone();
                        div()
                            .id(SharedString::from(format!("{id}-item-{label}")))
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(4.))
                            .text_sm_tw()
                            .cursor_pointer()
                            .map(|d| if is_sel { d.bg(theme::hexa(0x334155, 0.6)) } else { d.hover(|s| s.bg(theme::hexa(0x334155, 0.4))) })
                            .child(icon(ic, 14.).text_color(color))
                            .child(div().min_w_0().font_mono().text_color(slate::S300).child(text))
                            .on_click(cx.listener(move |this, _, window, cx| this.select(pick.clone(), window, cx)))
                            .auto_id(format!("rbac.picker.item.{label}"), "option", label.clone())
                            .selected(is_sel)
                    }))
                    .into_any_element(),
            )
        };
        let none = sas.is_empty() && users.is_empty() && groups.is_empty();
        let s1 = section("Service Accounts", sas, cx);
        let s2 = section("Users", users, cx);
        let s3 = section("Groups", groups, cx);
        let gutter = self.gutter.overflows();
        deferred(
            div()
                .id("rbac-picker-panel")
                .absolute()
                .left_0()
                .top(px(34. + 4.))
                .w(px(320.))
                .max_h(px(360.))
                .flex()
                .flex_col()
                .rounded(px(8.))
                .glass()
                .overflow_hidden()
                .occlude()
                .on_mouse_down_out(cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    if !tb.get().contains(&e.position) {
                        this.set_open(false, window, cx);
                    }
                }))
                .child(
                    div().p(px(8.)).border_b_1().border_color(slate::S800).child(
                        div()
                            .relative()
                            .h(px(32.))
                            .rounded(px(4.))
                            .when(focused, |d| d.child(focus_ring(cx)))
                            .child(self.input.clone())
                            .child(div().absolute().left(px(8.)).top_0().bottom_0().flex().items_center().child(icon("search", 14.).text_color(slate::S500))),
                    ),
                )
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_row()
                        .min_h_0()
                        .child(
                            div()
                                .id("rbac-picker-list")
                                .flex_1()
                                .min_w_0()
                                .max_h(px(300.))
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .children(s1)
                                .children(s2)
                                .children(s3)
                                .when(none, |d| d.child(div().p(px(16.)).text_center().text_sm_tw().text_color(slate::S500).child("No identities found"))),
                        )
                        .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
                        .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone())),
                )
                .auto_id(format!("rbac.picker.{}.panel", self.id), "listbox", "Identities"),
        )
        .with_priority(2)
    }
}

impl Render for IdentityPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.selected.as_ref().map(identity_label);
        let tb = self.trigger_bounds.clone();
        let trigger = div()
            .id("rbac-picker-trigger")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme::hexa(0x334155, 0.5))
            .hover(|s| s.border_color(slate::S600))
            .bg(theme::SURFACE)
            .text_sm_tw()
            .font_mono()
            .min_w(px(200.))
            .max_w(px(320.))
            .cursor_pointer()
            .text_color(if label.is_some() { slate::S200 } else { slate::S500 })
            .when_some(self.selected.as_ref(), |d, s| {
                let (ic, c) = identity_icon(s);
                d.child(icon(ic, 14.).text_color(c))
            })
            .child(div().truncate().child(label.clone().unwrap_or_else(|| "Select identity...".into())))
            .child(canvas(move |b, _, _| tb.set(b), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .on_click(cx.listener(|this, _, window, cx| {
                let next = !this.open;
                this.set_open(next, window, cx);
            }))
            .auto_id(format!("rbac.picker.{}", self.id), "combobox", label.unwrap_or_else(|| "Select identity...".into()));
        let panel = self.open.then(|| self.render_panel(window, cx));
        div().relative().flex_none().child(trigger).children(panel)
    }
}
