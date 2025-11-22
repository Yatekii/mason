use gpui::{prelude::*, *};
use crate::types::{MemoryKind, MemoryRegion};
use crate::utils::format_size;

pub fn render_regions_panel(
    regions: &[MemoryRegion],
    scale_factor: f64,
    min_block_height: f64,
    gap_height: f64,
    padding: f32,
) -> impl IntoElement {
    let mut panel = div()
        .id("regions_panel")
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
                .child("Memory Regions"),
        );

    for (i, region) in regions.iter().enumerate() {
        let height = (region.size as f64 * scale_factor).max(min_block_height) as f32;

        // Vibrant colors similar to One Dark theme for memory regions
        let color = match region.kind {
            MemoryKind::Flash => hsla(30.0 / 360.0, 0.75, 0.55, 1.0), // Orange
            MemoryKind::Ram => hsla(200.0 / 360.0, 0.75, 0.55, 1.0),   // Blue
        };

        // Light text for better contrast
        let text_color: Hsla = rgb(0xffffff).into();

        panel = panel.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .h(px(height))
                .px_3()
                .gap_3()
                .bg(color)
                .shadow_lg()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(text_color)
                        .flex_shrink_0()
                        .child(format!("{}", region.name)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color.opacity(0.85))
                        .flex_shrink_0()
                        .child(format!("0x{:08x}", region.start)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color.opacity(0.85))
                        .flex_shrink_0()
                        .child(format!("{}", format_size(region.size))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color.opacity(0.85))
                        .flex_shrink_0()
                        .child(format!("{:?}", region.kind)),
                ),
        );

        // Check if there's a gap between this region and the next
        if i + 1 < regions.len() {
            let next_region = &regions[i + 1];
            let current_end = region.start + region.size;
            if current_end < next_region.start {
                // There's a gap, insert a visual separator
                panel = panel.child(div().h(px(gap_height as f32)).bg(rgb(0x1e1e1e)));
            }
        }
    }

    panel
}
