// mod defmt_section; // No longer used - replaced with DescriptionList
// mod rtt_section; // No longer used - replaced with DescriptionList
mod details_panel;
mod sections_panel;
mod regions_panel;
pub mod symbols_panel;
mod memory_view;
// pub mod target_selector; // No longer used - replaced with gpui-component Select

// pub use defmt_section::DefmtSection;
// pub use rtt_section::RttSection;
pub use details_panel::DetailsPanel;
pub use sections_panel::render_sections_panel;
pub use regions_panel::render_regions_panel;
pub use memory_view::MemoryView;
