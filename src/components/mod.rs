// mod defmt_section; // No longer used - replaced with DescriptionList
// mod rtt_section; // No longer used - replaced with DescriptionList
mod details_panel;
mod dwarf_details_panel;
mod dwarf_tree_panel;
mod memory_view;
mod regions_panel;
mod sections_panel;
pub mod symbols_panel;
// pub mod target_selector; // No longer used - replaced with gpui-component Select

// pub use defmt_section::DefmtSection;
// pub use rtt_section::RttSection;
pub use details_panel::DetailsPanel;
pub use dwarf_details_panel::DwarfDetailsPanel;
pub use dwarf_tree_panel::{DwarfSymbolSelectEvent, DwarfTreePanel};
pub use memory_view::MemoryView;
pub use regions_panel::render_regions_panel;
pub use sections_panel::render_sections_panel;
