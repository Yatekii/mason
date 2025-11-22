use gpui::{prelude::*, *};
use crate::types::MemorySegment;
use crate::utils::{format_size, generate_color};

pub fn render_sections_panel(
    segments: &[MemorySegment],
    _selected_segment: Option<usize>,
    scale_factor: f64,
    min_block_height: f64,
    gap_height: f64,
    padding: f32,
    on_click: impl Fn(usize) -> Box<dyn Fn(&MouseUpEvent, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let mut panel = div()
        .id("memory_panel")
        .flex()
        .flex_col()
        .w(relative(0.5))
        .h_full()
        .p(px(padding))
        .overflow_y_scroll()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xaaaaaa))
                .mb_3()
                .child("ELF Sections"),
        );

    for (idx, segment) in segments.iter().enumerate() {
        let height = (segment.size as f64 * scale_factor).max(min_block_height) as f32;

        let has_conflicts = !segment.conflicts.is_empty();
        let color = generate_color(idx);
        // Light text for better contrast
        let text_color: Hsla = rgb(0xffffff).into();

        panel = panel.child(
            div()
                .id(idx)
                .flex()
                .flex_row()
                .items_center()
                .h(px(height))
                .px_3()
                .gap_3()
                .bg(color)
                .when(has_conflicts, |div| {
                    div.border_2().border_color(rgb(0xff0000))
                })
                .shadow_lg()
                .hover(|style| style.shadow_xl().cursor_pointer())
                .on_mouse_up(MouseButton::Left, on_click(idx))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(text_color)
                        .flex_shrink_0()
                        .child(format!("{}", segment.name)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color.opacity(0.85))
                        .flex_shrink_0()
                        .child(format!("0x{:016x}", segment.address)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color.opacity(0.85))
                        .flex_shrink_0()
                        .child(format!("{}", format_size(segment.size))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color.opacity(0.85))
                        .flex_shrink_0()
                        .child(format!("{}", segment.flags)),
                ),
        );

        // Check if there's a gap between this segment and the next
        if idx + 1 < segments.len() {
            let next_segment = &segments[idx + 1];
            let current_end = segment.address + segment.size;
            if current_end < next_segment.address {
                // There's a gap, insert a visual separator
                panel = panel.child(div().h(px(gap_height as f32)).bg(rgb(0x1e1e1e)));
            }
        }
    }

    panel
}
