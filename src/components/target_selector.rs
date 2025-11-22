use gpui::{prelude::*, *};
use gpui_component::input::{Input, InputState, InputEvent};

pub struct TargetSelector {
    current_target: String,
    all_targets: Vec<String>,
    pub search_input: Entity<InputState>,
    pub is_open: bool,
}

impl TargetSelector {
    pub fn new(current_target: String, all_targets: Vec<String>, cx: &mut App) -> Self {
        Self {
            current_target,
            all_targets,
            search_input: cx.new(|cx| InputState::new(cx).placeholder("Type to search...")),
            is_open: false,
        }
    }

    pub fn filtered_targets(&self, cx: &App) -> Vec<String> {
        let query = self.search_input.read(cx).text();
        if query.is_empty() {
            self.all_targets.clone()
        } else {
            let query_lower = query.to_lowercase();
            self.all_targets
                .iter()
                .filter(|t| t.to_lowercase().contains(&query_lower))
                .cloned()
                .collect()
        }
    }

    pub fn toggle_dropdown(&mut self) {
        self.is_open = !self.is_open;
    }

    pub fn close_dropdown(&mut self, cx: &mut App) {
        self.is_open = false;
        self.search_input.update(cx, |input, cx| {
            input.set_text("", cx);
        });
    }

    pub fn select_target(&mut self, target: String, cx: &mut App) -> bool {
        if target != self.current_target {
            self.current_target = target;
            self.close_dropdown(cx);
            true // Changed
        } else {
            self.close_dropdown(cx);
            false // No change
        }
    }

    pub fn current_target(&self) -> &str {
        &self.current_target
    }

    pub fn render_button(
        &self,
        on_toggle: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(rgb(0x3d3d3d))
            .rounded_md()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x4d4d4d)))
            .on_mouse_up(MouseButton::Left, on_toggle)
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xffffff))
                    .child(format!("Target: {}", self.current_target)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0xaaaaaa))
                    .child(if self.is_open { "▲" } else { "▼" }),
            )
    }

    pub fn render_dropdown(
        &self,
        on_select_target: impl Fn(String) -> Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static> + 'static,
        cx: &App,
    ) -> impl IntoElement {
        let filtered = self.filtered_targets(cx);
        let result_count = filtered.len();
        let display_limit = 100;
        let shown_count = filtered.len().min(display_limit);
        let current_target = self.current_target.clone();

        div()
            .id("target_dropdown")
            .absolute()
            .top(px(60.0))
            .right(px(16.0))
            .w(px(350.0))
            .max_h(px(450.0))
            .bg(rgb(0x2d2d2d))
            .border_1()
            .border_color(rgb(0x3d3d3d))
            .rounded_md()
            .shadow_lg()
            .child(
                // Search input field
                div()
                    .flex()
                    .items_center()
                    .p_3()
                    .border_b_1()
                    .border_color(rgb(0x3d3d3d))
                    .child(Input::new(&self.search_input))
            )
            .child(
                // Results count
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(0x888888))
                    .child(if result_count > display_limit {
                        format!("Showing {} of {} targets - refine search", shown_count, result_count)
                    } else {
                        format!("{} targets", result_count)
                    })
            )
            .child(
                div()
                    .id("target_dropdown_list")
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .max_h(px(350.0))
                    .children(filtered.iter().take(display_limit).map(|target| {
                        let target_clone = target.clone();
                        let is_current = target == &current_target;
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(rgb(0xffffff))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x3d3d3d)))
                            .when(is_current, |div| {
                                div.bg(rgb(0x4d4d4d))
                            })
                            .on_mouse_down(MouseButton::Left, on_select_target(target_clone))
                            .child(target.clone())
                    })),
            )
    }
}

