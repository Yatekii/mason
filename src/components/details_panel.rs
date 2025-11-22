use gpui::{prelude::*, *};
use gpui_component::description_list::{DescriptionList, DescriptionItem};
use gpui_component::label::Label;
use gpui_component::{ActiveTheme, StyledExt};
use gpui_component::scroll::ScrollbarAxis;
use crate::types::{DefmtInfo, MemorySegment, RttInfo};
use crate::utils::format_size;

#[derive(IntoElement)]
pub struct DetailsPanel {
    defmt_info: DefmtInfo,
    rtt_info: RttInfo,
    segments: Vec<MemorySegment>,
    selected_segment: Option<usize>,
    total_size: u64,
}

impl DetailsPanel {
    pub fn new(
        defmt_info: DefmtInfo,
        rtt_info: RttInfo,
        segments: Vec<MemorySegment>,
        selected_segment: Option<usize>,
        total_size: u64,
    ) -> Self {
        Self {
            defmt_info,
            rtt_info,
            segments,
            selected_segment,
            total_size,
        }
    }
}

impl RenderOnce for DetailsPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .id("details_panel")
            .flex()
            .flex_col()
            .w(px(350.0))
            .h_full()
            .bg(cx.theme().sidebar)
            .border_l_1()
            .border_color(cx.theme().sidebar_border)
            .child(self.render_content(cx))
    }
}

impl DetailsPanel {
    fn render_content(self, _cx: &App) -> impl IntoElement {
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .scrollable(ScrollbarAxis::Vertical);

        // Add defmt info section if present
        if self.defmt_info.present {
            let mut defmt_list = DescriptionList::horizontal()
                .bordered(true)
                .columns(1);

            defmt_list = defmt_list.child(
                DescriptionItem::new("defmt Support")
                    .value("Yes")
                    .span(1),
            );

            // Add sections
            for (section_name, section_size) in &self.defmt_info.sections {
                defmt_list = defmt_list.child(
                    DescriptionItem::new(section_name.clone())
                        .value(format_size(*section_size))
                        .span(1),
                );
            }

            panel = panel
                .child(
                    Label::new("defmt Configuration")
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .mb_2()
                )
                .child(defmt_list);
        }

        // Add RTT info section if present
        if self.rtt_info.present {
            let mut rtt_list = DescriptionList::horizontal()
                .bordered(true)
                .columns(1);

            rtt_list = rtt_list.child(
                DescriptionItem::new("RTT Support")
                    .value("Yes")
                    .span(1),
            );

            if let Some(symbol_name) = &self.rtt_info.symbol_name {
                rtt_list = rtt_list.child(
                    DescriptionItem::new("Symbol")
                        .value(symbol_name.clone())
                        .span(1),
                );
            }

            if let Some(address) = self.rtt_info.address {
                rtt_list = rtt_list.child(
                    DescriptionItem::new("Address")
                        .value(format!("0x{:08x}", address))
                        .span(1),
                );
            }

            if let Some(size) = self.rtt_info.size {
                rtt_list = rtt_list.child(
                    DescriptionItem::new("Size")
                        .value(format_size(size))
                        .span(1),
                );
            }

            rtt_list = rtt_list
                .child(
                    DescriptionItem::new("Up Buffers")
                        .value(format!("{}", self.rtt_info.up_buffers.len()))
                        .span(1),
                )
                .child(
                    DescriptionItem::new("Down Buffers")
                        .value(format!("{}", self.rtt_info.down_buffers.len()))
                        .span(1),
                );

            if let Some(max_up) = self.rtt_info.max_up_buffers {
                rtt_list = rtt_list.child(
                    DescriptionItem::new("Max Up Buffers")
                        .value(format!("{}", max_up))
                        .span(1),
                );
            }

            if let Some(max_down) = self.rtt_info.max_down_buffers {
                rtt_list = rtt_list.child(
                    DescriptionItem::new("Max Down Buffers")
                        .value(format!("{}", max_down))
                        .span(1),
                );
            }

            panel = panel
                .child(
                    Label::new("RTT Configuration")
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .mb_2()
                )
                .child(rtt_list);
        }

        // Add selected segment details
        if let Some(idx) = self.selected_segment {
            if let Some(segment) = self.segments.get(idx) {
                let mut segment_list = DescriptionList::horizontal()
                    .bordered(true)
                    .columns(1);

                segment_list = segment_list
                    .child(
                        DescriptionItem::new("Name")
                            .value(segment.name.clone())
                            .span(1),
                    )
                    .child(
                        DescriptionItem::new("Start")
                            .value(format!("0x{:016x}", segment.address))
                            .span(1),
                    )
                    .child(
                        DescriptionItem::new("End")
                            .value(format!("0x{:016x}", segment.address + segment.size))
                            .span(1),
                    )
                    .child(
                        DescriptionItem::new("Size")
                            .value(format_size(segment.size))
                            .span(1),
                    )
                    .child(
                        DescriptionItem::new("Flags")
                            .value(segment.flags.clone())
                            .span(1),
                    )
                    .child(
                        DescriptionItem::new("Type")
                            .value(if segment.is_load { "LOAD" } else { "Non-LOAD" })
                            .span(1),
                    )
                    .child(
                        DescriptionItem::new("Percentage")
                            .value(format!(
                                "{:.2}%",
                                (segment.size as f64 / self.total_size as f64) * 100.0
                            ))
                            .span(1),
                    );

                panel = panel
                    .child(
                        Label::new("Selected Section")
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .mb_2()
                    )
                    .child(segment_list);

                // Add conflicts section if present
                if !segment.conflicts.is_empty() {
                    panel = panel.child(
                        div()
                            .mt_2()
                            .p_3()
                            .border_1()
                            .border_color(rgb(0xff4444))
                            .rounded_md()
                            .bg(rgb(0x2d1a1a))
                            .child(
                                Label::new("⚠ Conflicts")
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0xff4444))
                                    .mb_2()
                            )
                            .children(segment.conflicts.iter().map(|conflict| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(0xff8888))
                                    .mb_1()
                                    .child(format!("• {}", conflict))
                            })),
                    );
                }
            }
        }

        panel
    }
}
