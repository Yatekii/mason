use gpui::{prelude::*, *};
use crate::types::RttInfo;
use crate::utils::{detail_row, format_size};

pub struct RttSection {
    info: RttInfo,
}

impl RttSection {
    pub fn new(info: RttInfo) -> Self {
        Self { info }
    }
}

impl IntoElement for RttSection {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        if !self.info.present {
            return div();
        }

        let mut section = div()
            .flex()
            .flex_col()
            .gap_3()
            .mb_4()
            .pb_4()
            .border_b_1()
            .border_color(rgb(0x3d3d3d))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0x66ff66))
                    .mb_3()
                    .child("✓ RTT Control Block"),
            )
            .child(detail_row("Status", "Present"))
            .when_some(self.info.symbol_name.as_ref(), |div, name| {
                div.child(detail_row("Symbol", name.clone()))
            })
            .when_some(self.info.address, |div, addr| {
                div.child(detail_row("Address", format!("0x{:08x}", addr)))
            })
            .when_some(self.info.size, |div, size| {
                div.child(detail_row("Size", format_size(size)))
            });

        // Add buffer configuration info
        if let Some(max_up) = self.info.max_up_buffers {
            section = section.child(detail_row("Max Up Buffers", format!("{}", max_up)));
        }
        if let Some(max_down) = self.info.max_down_buffers {
            section = section.child(detail_row("Max Down Buffers", format!("{}", max_down)));
        }

        // Add up buffers
        if !self.info.up_buffers.is_empty() {
            section = section.child(
                div()
                    .mt_2()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xaaaaaa))
                    .child("Up Buffers:"),
            );
            for buffer in &self.info.up_buffers {
                section = section.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .ml_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x888888))
                                .child(format!("{}:", buffer.name)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0xcccccc))
                                .child(format!(
                                    "  Address: 0x{:08x}, Size: {}",
                                    buffer.buffer_address,
                                    format_size(buffer.size as u64)
                                )),
                        ),
                );
            }
        }

        // Add down buffers
        if !self.info.down_buffers.is_empty() {
            section = section.child(
                div()
                    .mt_2()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xaaaaaa))
                    .child("Down Buffers:"),
            );
            for buffer in &self.info.down_buffers {
                section = section.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .ml_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x888888))
                                .child(format!("{}:", buffer.name)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0xcccccc))
                                .child(format!(
                                    "  Address: 0x{:08x}, Size: {}",
                                    buffer.buffer_address,
                                    format_size(buffer.size as u64)
                                )),
                        ),
                );
            }
        }

        section
    }
}
