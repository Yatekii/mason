use gpui::*;

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn detail_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(rgb(0x888888))
                .child(format!("{}:", label.into())),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(0xcccccc))
                .font_weight(FontWeight::MEDIUM)
                .child(value.into()),
        )
}

pub fn generate_color(index: usize) -> Hsla {
    // Generate vibrant colors similar to One Dark theme using golden ratio for wide color range
    // High saturation (0.75) and medium lightness (0.55) for rich, saturated colors
    let hue = (index as f32 * 137.508) % 360.0;
    hsla(hue / 360.0, 0.75, 0.55, 1.0)
}
