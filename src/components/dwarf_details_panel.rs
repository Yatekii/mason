use crate::types::{DwarfSymbol, DwarfTag};
use crate::utils::format_size;
use gpui::{prelude::*, *};
use gpui_component::scroll::ScrollbarAxis;
use gpui_component::ActiveTheme;
use gpui_component::StyledExt;

#[derive(IntoElement)]
pub struct DwarfDetailsPanel {
    selected_symbol: Option<DwarfSymbol>,
}

impl DwarfDetailsPanel {
    pub fn new(selected_symbol: Option<DwarfSymbol>) -> Self {
        Self { selected_symbol }
    }

    fn tag_color(tag: &DwarfTag) -> Rgba {
        match tag {
            DwarfTag::CompileUnit => rgb(0x61afef),
            DwarfTag::Subprogram => rgb(0xc678dd),
            DwarfTag::Variable => rgb(0xe5c07b),
            DwarfTag::FormalParameter => rgb(0xd19a66),
            DwarfTag::StructureType => rgb(0x98c379),
            DwarfTag::UnionType => rgb(0x98c379),
            DwarfTag::EnumerationType => rgb(0x56b6c2),
            DwarfTag::Member => rgb(0xabb2bf),
            DwarfTag::Typedef => rgb(0xe06c75),
            DwarfTag::Namespace => rgb(0x61afef),
            DwarfTag::LexicalBlock => rgb(0x5c6370),
            DwarfTag::InlinedSubroutine => rgb(0xc678dd),
            DwarfTag::Other(_) => rgb(0xabb2bf),
        }
    }
}

impl RenderOnce for DwarfDetailsPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .id("dwarf_details_panel")
            .flex()
            .flex_row()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_content(cx))
    }
}

impl DwarfDetailsPanel {
    fn render_content(self, cx: &App) -> impl IntoElement {
        if let Some(symbol) = self.selected_symbol {
            let tag_color = Self::tag_color(&symbol.tag);
            let icon = symbol.tag.icon().to_string();
            let has_children = !symbol.children.is_empty();
            let has_attributes = !symbol.attributes.is_empty();

            div()
                .flex()
                .flex_row()
                .size_full()
                .overflow_hidden()
                .child(
                    // Left panel: Symbol header + attributes
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .min_w(px(300.0))
                        .border_r_1()
                        .border_color(cx.theme().border)
                        .child(
                            // Header with symbol name and type
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .p_3()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().sidebar)
                                .child(
                                    div().flex().items_center().gap_2().child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(tag_color)
                                            .text_color(rgb(0xffffff))
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(format!(
                                                "{} {}",
                                                icon,
                                                symbol.tag.display_name()
                                            )),
                                    ),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_family("monospace")
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(symbol.name.clone()),
                                ),
                        )
                        .child(
                            // Attributes section - scrollable
                            div().flex_1().overflow_hidden().child(
                                div().size_full().scrollable(ScrollbarAxis::Vertical).child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .p_2()
                                        .gap_1()
                                        .when(has_attributes, |d| {
                                            d.child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(cx.theme().muted_foreground)
                                                    .pb_1()
                                                    .child("ATTRIBUTES"),
                                            )
                                            .children(
                                                symbol.attributes.iter().map(|(name, value)| {
                                                    div()
                                                        .flex()
                                                        .py(px(2.0))
                                                        .gap_2()
                                                        .child(
                                                            div()
                                                                .w(px(120.0))
                                                                .flex_shrink_0()
                                                                .text_xs()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(name.clone()),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .text_xs()
                                                                .font_family("monospace")
                                                                .text_color(cx.theme().foreground)
                                                                .overflow_x_hidden()
                                                                .text_ellipsis()
                                                                .child(value.clone()),
                                                        )
                                                }),
                                            )
                                        })
                                        .when(!has_attributes, |d| {
                                            d.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("No attributes"),
                                            )
                                        }),
                                ),
                            ),
                        ),
                )
                .when(has_children, |d| {
                    // Right panel: Children (members, parameters, etc.)
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(
                                // Children header
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_3()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().sidebar)
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(cx.theme().muted_foreground)
                                            .child(Self::children_header_text(&symbol.tag)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{} items", symbol.children.len())),
                                    ),
                            )
                            .child(
                                // Children list - scrollable
                                div().flex_1().overflow_hidden().child(
                                    div().size_full().scrollable(ScrollbarAxis::Both).child(
                                        div().flex().flex_col().children(
                                            symbol
                                                .children
                                                .iter()
                                                .map(|child| Self::render_child_row(child, cx)),
                                        ),
                                    ),
                                ),
                            ),
                    )
                })
        } else {
            // No symbol selected
            div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Select a DWARF symbol to view details"),
                )
        }
    }

    fn children_header_text(tag: &DwarfTag) -> &'static str {
        match tag {
            DwarfTag::StructureType => "STRUCT MEMBERS",
            DwarfTag::UnionType => "UNION MEMBERS",
            DwarfTag::EnumerationType => "ENUM VARIANTS",
            DwarfTag::Subprogram => "PARAMETERS & LOCALS",
            DwarfTag::Namespace => "CONTENTS",
            DwarfTag::CompileUnit => "SYMBOLS",
            _ => "CHILDREN",
        }
    }

    fn render_child_row(child: &DwarfSymbol, cx: &App) -> Div {
        let tag_color = Self::tag_color(&child.tag);
        let icon = child.tag.icon().to_string();

        // Extract offset from attributes if present (for struct members)
        let offset = child
            .attributes
            .iter()
            .find(|(name, _)| name == "DW_AT_data_member_location")
            .map(|(_, v)| v.clone());

        // Extract type from attributes
        let type_info = child
            .attributes
            .iter()
            .find(|(name, _)| name == "DW_AT_type")
            .map(|(_, v)| v.clone())
            .or_else(|| child.type_name.clone());

        div()
            .flex()
            .items_center()
            .w_full()
            .px_3()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .hover(|d| d.bg(cx.theme().list_hover))
            // Icon
            .child(
                div()
                    .text_xs()
                    .text_color(tag_color)
                    .w(px(16.0))
                    .child(icon),
            )
            // Offset (for struct members)
            .when_some(offset, |d, off| {
                d.child(
                    div()
                        .w(px(60.0))
                        .flex_shrink_0()
                        .text_xs()
                        .font_family("monospace")
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("+{}", off)),
                )
            })
            // Name
            .child(
                div()
                    .w(px(200.0))
                    .flex_shrink_0()
                    .text_sm()
                    .font_family("monospace")
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().foreground)
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(child.name.clone()),
            )
            // Type
            .when_some(type_info, |d, ti| {
                d.child(
                    div()
                        .flex_1()
                        .text_xs()
                        .font_family("monospace")
                        .text_color(cx.theme().muted_foreground)
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(ti),
                )
            })
            // Size
            .when_some(child.size, |d, size| {
                d.child(
                    div()
                        .w(px(60.0))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .text_right()
                        .child(format_size(size)),
                )
            })
    }
}
