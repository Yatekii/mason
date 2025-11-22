use gpui::{prelude::*, *};
use crate::types::DefmtInfo;
use crate::utils::{detail_row, format_size};

pub struct DefmtSection {
    info: DefmtInfo,
}

impl DefmtSection {
    pub fn new(info: DefmtInfo) -> Self {
        Self { info }
    }
}

impl IntoElement for DefmtSection {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        if !self.info.present {
            return div();
        }

        div()
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
                    .child("✓ defmt Debug Symbols"),
            )
            .child(detail_row("Status", "Present"))
            .child(detail_row(
                "Sections",
                format!("{}", self.info.sections.len()),
            ))
            .children(
                self.info
                    .sections
                    .iter()
                    .map(|(name, size)| detail_row(name.clone(), format_size(*size))),
            )
    }
}
