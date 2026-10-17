//! RbacMatrix (spec 03 §7.3): legend, Copy CSV, resource × verb table and the
//! cell popover with the rule chain.

use super::rule_chain::rule_chain;
use super::{status_str, ActiveCell, RbacView, VERBS};
use crate::automation::Automation;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::scrollbar;
use crate::ui::text::caps;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::rbac::{PermissionMatrix, PermissionStatus};

/// Natural (max-content) widths of the design: the resource column
/// (longest row "clusterrolebindings rbac.authorization.k8s.io" + padding) and
/// a verb column (its 60px min-width). HTML auto layout scales both to fill.
const RESOURCE_NATURAL: f32 = 315.4;
const VERB_NATURAL: f32 = 60.;
pub const HEADER_H: f32 = 32.;
pub const ROW_H: f32 = 39.;

/// Column widths for a table `avail` px wide (resource, verb).
pub fn column_widths(avail: f32) -> (f32, f32) {
    let total = RESOURCE_NATURAL + VERB_NATURAL * VERBS.len() as f32;
    let f = (avail / total).max(1.);
    (RESOURCE_NATURAL * f, VERB_NATURAL * f)
}

/// (fill, ring) for a cell status.
fn status_colors(s: &PermissionStatus) -> (u32, &'static str) {
    match s {
        PermissionStatus::Allowed => (0x10b981, "Allowed"),
        PermissionStatus::Denied => (0xef4444, "Denied"),
        PermissionStatus::Conditional => (0xf59e0b, "Conditional"),
    }
}

impl RbacView {
    pub(super) fn render_matrix(&mut self, m: &PermissionMatrix, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let legend_item = |label: &str, c: u32| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(div().size(px(12.)).rounded(px(2.)).bg(theme::hexa(c, 0.2)).border_1().border_color(theme::hexa(c, 0.3)))
                .child(label.to_string())
        };
        let csv = super::matrix_csv(m);
        let top = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .text_10()
                    .text_color(slate::S500)
                    .child(legend_item("Allowed", 0x10b981))
                    .child(legend_item("Denied", 0xef4444))
                    .child(legend_item("Conditional", 0xf59e0b)),
            )
            .child(
                div()
                    .id("rbac-copy-csv")
                    .group("rbac-csv")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .text_10()
                    .text_color(slate::S500)
                    .hover(|s| s.text_color(slate::S300).bg(theme::hexa(0x334155, 0.4)))
                    .cursor_pointer()
                    .child(icon("copy", 12.).text_color(slate::S500).group_hover("rbac-csv", |s| s.text_color(slate::S300)))
                    .child("Copy CSV")
                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(csv.clone())))
                    .auto_id("rbac.copy-csv", "button", "Copy CSV"),
            );

        // Container: content width minus its 1px borders; the vertical
        // scrollbar takes 6px when the rows overflow.
        let gutter = self.matrix_gutter.overflows();
        let avail = f32::from(window.viewport_size().width) - crate::views::sidebar::WIDTH - 32. - 32. - 2. - if gutter { scrollbar::WIDTH } else { 0. };
        let (rw, vw) = column_widths(avail);
        let th = |w: f32| div().w(px(w)).flex_none().py(px(8.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500);
        let header = div()
            .flex()
            .flex_row()
            .h(px(HEADER_H))
            .bg(theme::SURFACE)
            .border_b_1()
            .border_color(slate::S800)
            .child(th(rw).px(px(12.)).child(caps("Resource", 10.)))
            .children(VERBS.iter().map(|v| th(vw).px(px(8.)).flex().justify_center().child(caps(v, 10.))));

        let bounds = self.cell_bounds.clone();
        let active = self.popover.as_ref().map(|p| (p.resource.clone(), p.verb.clone()));
        let rows = m.rows.iter().enumerate().map(|(ri, row)| {
            let resource_cell = div()
                .w(px(rw))
                .flex_none()
                .h_full()
                .px(px(12.))
                .bg(theme::SURFACE)
                .flex()
                .flex_row()
                .items_center()
                .text_xs_tw()
                .font_mono()
                .text_color(slate::S300)
                .child(row.resource.clone())
                .when(!row.api_group.is_empty(), |d| d.child(div().ml(px(4.)).text_10().text_color(slate::S600).child(row.api_group.clone())));
            let cells = VERBS.iter().enumerate().map(|(vi, verb)| {
                let td = div().w(px(vw)).flex_none().h_full().flex().justify_center().pt(px(5.5));
                match row.verbs.get(*verb) {
                    None => td.child(div().size(px(20.)).rounded(px(2.)).bg(theme::hexa(0x1e293b, 0.5))).into_any_element(),
                    Some(cell) => {
                        let (c, label) = status_colors(&cell.status);
                        let is_active = active.as_ref().map(|(r, v)| *r == row.resource && v == verb).unwrap_or(false);
                        let b = bounds.clone();
                        let info = (row.resource.clone(), verb.to_string(), cell.clone());
                        let tip = format!("{} {}: {}", row.resource, verb, label);
                        let grow = |d: Stateful<Div>| {
                            d.size(px(25.)).bg(theme::hexa(c, 0.3)).border_1().border_color(theme::hexa(c, 0.5)).rounded(px(2.5))
                        };
                        td.child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .id(SharedString::from(format!("rbac-cell-{ri}-{vi}")))
                                        .flex_none()
                                        .size(px(20.))
                                        .rounded(px(2.))
                                        .bg(theme::hexa(c, 0.2))
                                        .cursor_pointer()
                                        .map(|d| if is_active { grow(d) } else { d.hover(move |s| s.size(px(25.)).bg(theme::hexa(c, 0.3)).border_1().border_color(theme::hexa(c, 0.5)).rounded(px(2.5))) })
                                        .child(canvas(move |bnd, _, _| {
                                            b.borrow_mut().insert((ri, vi), bnd);
                                        }, |_, _, _, _| {}).size_full())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            let bnd = this.cell_bounds.borrow().get(&(ri, vi)).copied();
                                            let Some(bnd) = bnd else { return };
                                            // getBoundingClientRect of the hovered (scale 1.25) cell.
                                            let left = f32::from(bnd.left()) - 2.5;
                                            let bottom = f32::from(bnd.bottom()) + 2.5;
                                            let ww = f32::from(window.viewport_size().width);
                                            this.popover = Some(ActiveCell {
                                                resource: info.0.clone(),
                                                verb: info.1.clone(),
                                                cell: info.2.clone(),
                                                top: bottom + 8.,
                                                left: left.min(ww - 360.),
                                            });
                                            cx.notify();
                                        }))
                                        .auto_id(format!("rbac.cell.{}.{}", row.resource, verb), "button", tip)
                                        .selected(is_active),
                                ),
                        )
                        .into_any_element()
                    }
                }
            });
            div()
                .h(px(ROW_H))
                .flex_none()
                .flex()
                .flex_col()
                .child(div().flex_1().flex().flex_row().when(ri % 2 == 1, |d| d.bg(theme::hexa(0xffffff, 0.01))).child(resource_cell).children(cells))
                .child(div().h(px(1.)).flex_none().bg(theme::hexa(0x1e293b, 0.5)))
                .auto_id(format!("rbac.row.{}", row.resource), "row", row.resource.clone())
        });

        let scroll = self.matrix_scroll.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(top)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(slate::S800)
                    .overflow_hidden()
                    .relative()
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .flex_row()
                            .child(
                                div()
                                    .id("rbac-matrix-scroll")
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll)
                                    .child(div().h(px(HEADER_H)))
                                    .children(rows),
                            )
                            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none())),
                    )
                    .child(div().absolute().top_0().left_0().right(px(if gutter { scrollbar::WIDTH } else { 0. })).child(header))
                    .child(scrollbar::vertical(scroll, self.matrix_gutter.clone()))
                    .auto_id("rbac.matrix", "table", "Permission Matrix"),
            )
    }

    pub(super) fn render_popover(&self, p: &ActiveCell, cx: &mut Context<Self>) -> AnyElement {
        let (c, label) = status_colors(&p.cell.status);
        let allowed = !matches!(p.cell.status, PermissionStatus::Denied);
        let id = format!("rbac-pop-{}-{}", p.resource, p.verb);
        deferred(
            anchored().position(point(px(p.left), px(p.top))).child(
                div()
                    .id("rbac-popover")
                    .w(px(340.))
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .glass()
                    .rounded(px(8.))
                    .p(px(12.))
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.popover = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .mb(px(8.))
                            .child(div().text_xs_tw().font_mono().text_color(slate::S400).child(format!("{} / {}", p.resource, p.verb)))
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .text_10()
                                    .line_height(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .bg(theme::hexa(c, 0.15))
                                    .text_color(match p.cell.status {
                                        PermissionStatus::Allowed => theme::hex(0x34d399),
                                        PermissionStatus::Denied => theme::hex(0xf87171),
                                        PermissionStatus::Conditional => theme::hex(0xfbbf24),
                                    })
                                    .child(label),
                            ),
                    )
                    .child(rule_chain(id, "rbac.popover.chain", &p.cell.rule_chain, allowed))
                    .with_animation("rbac-pop-in", Animation::new(std::time::Duration::from_millis(150)), |el, t| {
                        el.opacity(t).mt(px(4. * (1. - t)))
                    })
                    .auto_id("rbac.popover", "dialog", format!("{} / {}: {}", p.resource, p.verb, status_str(&p.cell.status))),
            ),
        )
        .with_priority(3)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::column_widths;

    #[test]
    fn widths_match_design_at_1400() {
        // 1400 window: 1400 - 256 sidebar - 32 main padding - 32 content padding - 2 border - 6 scrollbar.
        let (r, v) = column_widths(1072.);
        assert!((r - 460.).abs() < 1.5, "resource {r}");
        assert!((v - 87.5).abs() < 0.5, "verb {v}");
    }
}
