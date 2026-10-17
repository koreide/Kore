//! Element registry: UI code tags elements with stable ids; each painted frame
//! records their bounds so the harness can locate and assert on them.
//!
//! Always compiled, so views can tag elements unconditionally. Nothing is
//! recorded unless the control server or a unit test enables it.
//!
//! `Tracked` is layout-neutral: it returns its child's `LayoutId` and adds no node,
//! so tagging never changes what is rendered.

use gpui_kit::*;
use kore_automation::{Bounds as ABounds, Element as AElement};
use std::cell::RefCell;

thread_local! {
    static ENABLED: RefCell<bool> = const { RefCell::new(false) };
    static CURRENT: RefCell<Vec<AElement>> = const { RefCell::new(Vec::new()) };
    static LAST: RefCell<Vec<AElement>> = const { RefCell::new(Vec::new()) };
}

#[cfg_attr(not(any(test, feature = "test-harness")), allow(dead_code))]
pub fn enable() {
    ENABLED.with(|e| *e.borrow_mut() = true);
}

pub fn enabled() -> bool {
    ENABLED.with(|e| *e.borrow())
}

pub fn begin_frame() {
    if enabled() {
        CURRENT.with(|c| c.borrow_mut().clear());
    }
}

pub fn end_frame() {
    if enabled() {
        CURRENT.with(|c| LAST.with(|l| std::mem::swap(&mut *l.borrow_mut(), &mut *c.borrow_mut())));
    }
}

#[cfg_attr(not(any(test, feature = "test-harness")), allow(dead_code))]
pub fn last_frame() -> Vec<AElement> {
    LAST.with(|l| l.borrow().clone())
}

pub struct Tracked {
    child: AnyElement,
    id: SharedString,
    role: &'static str,
    label: SharedString,
    selected: bool,
    disabled: bool,
}

impl IntoElement for Tracked {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Tracked {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if enabled() {
            let focused = false;
            CURRENT.with(|c| {
                c.borrow_mut().push(AElement {
                    id: self.id.to_string(),
                    role: self.role.to_string(),
                    label: self.label.to_string(),
                    bounds: ABounds {
                        x: bounds.origin.x.into(),
                        y: bounds.origin.y.into(),
                        w: bounds.size.width.into(),
                        h: bounds.size.height.into(),
                    },
                    selected: self.selected,
                    focused,
                    disabled: self.disabled,
                })
            });
        }
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// Tag any element for the automation registry.
pub trait Automation: IntoElement + Sized {
    fn auto_id(self, id: impl Into<SharedString>, role: &'static str, label: impl Into<SharedString>) -> Tracked {
        Tracked {
            child: self.into_any_element(),
            id: id.into(),
            role,
            label: label.into(),
            selected: false,
            disabled: false,
        }
    }
}

impl<T: IntoElement> Automation for T {}

impl Tracked {
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
