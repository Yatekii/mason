use crate::components::symbols_panel::SymbolsTableDelegate;
use crate::components::{
    render_regions_panel, render_sections_panel, DetailsPanel, DwarfDetailsPanel,
    DwarfSymbolSelectEvent, DwarfTreePanel,
};
use crate::parser::{
    get_all_targets, load_memory_layout_from_probe_rs, parse_defmt_info, parse_elf_segments,
    parse_rtt_info,
};
use crate::types::{
    DefmtInfo, DwarfInfo, DwarfSymbol, ElfSymbol, MemoryRegion, MemorySegment, RttInfo,
};
use gpui::{prelude::*, *};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_component::table::{Table, TableState};
use gpui_component::theme::{Theme, ThemeRegistry};
use gpui_component::IndexPath;
use gpui_component::TitleBar;
use gpui_component::{v_flex, ActiveTheme, Sizable};
use std::path::PathBuf;

pub struct MemoryView {
    segments: Vec<MemorySegment>,
    memory_regions: Vec<MemoryRegion>,
    symbols: Vec<ElfSymbol>,
    defmt_info: DefmtInfo,
    rtt_info: RttInfo,
    dwarf_info: DwarfInfo,
    selected_segment: Option<usize>,
    selected_dwarf_symbol: Option<DwarfSymbol>,
    symbols_table: Option<Entity<TableState<SymbolsTableDelegate>>>,
    dwarf_tree_panel: Entity<DwarfTreePanel>,
    target_select: Entity<SelectState<SearchableVec<String>>>,
    theme_select: Entity<SelectState<SearchableVec<String>>>,
    elf_path: PathBuf,
    focus_handle: FocusHandle,
}

impl Focusable for MemoryView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MemoryView {
    pub fn new(
        segments: Vec<MemorySegment>,
        memory_regions: Vec<MemoryRegion>,
        symbols: Vec<ElfSymbol>,
        defmt_info: DefmtInfo,
        rtt_info: RttInfo,
        dwarf_info: DwarfInfo,
        current_target: Option<String>,
        elf_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Build target list with "None" option at the top
        let mut all_targets = vec!["(No target)".to_string()];
        all_targets.extend(get_all_targets());
        let delegate = SearchableVec::new(all_targets.clone());

        let selected_index = if let Some(ref target) = current_target {
            all_targets
                .iter()
                .position(|t| t == target)
                .map(|row| IndexPath::default().row(row))
        } else {
            Some(IndexPath::default().row(0)) // Select "(No target)"
        };

        let target_select =
            cx.new(|cx| SelectState::new(delegate, selected_index, window, cx).searchable(true));

        cx.subscribe(&target_select, Self::on_target_select_event)
            .detach();

        // Create DWARF tree panel
        let dwarf_info_clone = dwarf_info.clone();
        let dwarf_tree_panel = cx.new(|cx| DwarfTreePanel::new(dwarf_info_clone, window, cx));

        cx.subscribe(&dwarf_tree_panel, Self::on_dwarf_symbol_select)
            .detach();

        // Create theme selector
        let theme_registry = ThemeRegistry::global(cx);
        let theme_names: Vec<String> = theme_registry
            .sorted_themes()
            .iter()
            .map(|theme| theme.name.to_string())
            .collect();

        let current_theme_name = Theme::global(cx).theme_name().to_string();
        let theme_selected_index = theme_names
            .iter()
            .position(|t| t == &current_theme_name)
            .map(|row| IndexPath::default().row(row));

        let theme_delegate = SearchableVec::new(theme_names);
        let theme_select = cx.new(|cx| {
            SelectState::new(theme_delegate, theme_selected_index, window, cx).searchable(true)
        });

        cx.subscribe(&theme_select, Self::on_theme_select_event)
            .detach();

        Self {
            segments,
            memory_regions,
            symbols,
            defmt_info,
            rtt_info,
            dwarf_info,
            selected_segment: None,
            selected_dwarf_symbol: None,
            symbols_table: None,
            dwarf_tree_panel,
            target_select,
            theme_select,
            elf_path,
            focus_handle: cx.focus_handle(),
        }
    }

    fn on_dwarf_symbol_select(
        &mut self,
        _: Entity<DwarfTreePanel>,
        event: &DwarfSymbolSelectEvent,
        cx: &mut Context<Self>,
    ) {
        // Clear ELF segment selection so DWARF details panel is shown
        self.selected_segment = None;
        self.symbols_table = None;
        self.selected_dwarf_symbol = Some(event.symbol.clone());
        cx.notify();
    }

    fn on_target_select_event(
        &mut self,
        _: Entity<SelectState<SearchableVec<String>>>,
        event: &SelectEvent<SearchableVec<String>>,
        cx: &mut Context<Self>,
    ) {
        if let SelectEvent::Confirm(Some(target)) = event {
            self.on_target_change((*target).clone(), cx);
        }
    }

    fn on_theme_select_event(
        &mut self,
        _: Entity<SelectState<SearchableVec<String>>>,
        event: &SelectEvent<SearchableVec<String>>,
        cx: &mut Context<Self>,
    ) {
        if let SelectEvent::Confirm(Some(theme_name)) = event {
            self.on_theme_change((*theme_name).clone(), cx);
        }
    }

    fn on_theme_change(&mut self, theme_name: String, cx: &mut Context<Self>) {
        let theme_registry = ThemeRegistry::global(cx);
        let theme_name_shared: SharedString = theme_name.into();
        if let Some(theme_config) = theme_registry.themes().get(&theme_name_shared) {
            let theme_config = theme_config.clone();
            let theme_mode = theme_config.mode;

            let theme = Theme::global_mut(cx);
            if theme_mode.is_dark() {
                theme.dark_theme = theme_config;
            } else {
                theme.light_theme = theme_config;
            }
            Theme::change(theme_mode, None, cx);
            cx.notify();
        }
    }

    fn on_target_change(&mut self, target: String, cx: &mut Context<Self>) {
        if target == "(No target)" {
            // Clear target selection but keep segments
            self.memory_regions.clear();
            // Re-parse segments without conflict detection
            if let Ok(segments) = parse_elf_segments(&self.elf_path, None) {
                self.segments = segments;
            }
            // Clear segment-related conflicts
            for segment in &mut self.segments {
                segment.conflicts.clear();
            }
            self.selected_segment = None;
            self.symbols_table = None;
            cx.notify();
            return;
        }

        if let Ok(memory_regions) = load_memory_layout_from_probe_rs(&target) {
            if let Ok(segments) = parse_elf_segments(&self.elf_path, Some(&memory_regions)) {
                self.memory_regions = memory_regions;
                self.segments = segments;
                self.selected_segment = None;
                self.symbols_table = None;

                // Reload defmt and RTT info
                if let Ok(defmt_info) = parse_defmt_info(&self.elf_path) {
                    self.defmt_info = defmt_info;
                }
                if let Ok(rtt_info) = parse_rtt_info(&self.elf_path) {
                    self.rtt_info = rtt_info;
                }
                cx.notify();
            }
        }
    }

    fn on_segment_click(
        &mut self,
        idx: usize,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Toggle the selected segment (click again to close)
        if self.selected_segment == Some(idx) {
            self.selected_segment = None;
            self.symbols_table = None;
        } else {
            self.selected_segment = Some(idx);

            // Filter symbols for the selected segment
            if let Some(segment) = self.segments.get(idx) {
                let segment_start = segment.address;
                let segment_end = segment.address + segment.size;
                let filtered_symbols: Vec<ElfSymbol> = self
                    .symbols
                    .iter()
                    .filter(|s| s.address >= segment_start && s.address < segment_end)
                    .cloned()
                    .collect();

                // Create or update the table with the filtered symbols
                let delegate = SymbolsTableDelegate::new(filtered_symbols);
                self.symbols_table = Some(cx.new(|cx| {
                    TableState::new(delegate, window, cx)
                        .row_selectable(false)
                        .col_selectable(false)
                        .sortable(true)
                }));
            }
        }
        cx.notify();
    }

    fn calculate_scale_factor(
        &self,
        total_size: u64,
        gap_count: usize,
        target_total_height: f64,
        gap_height: f64,
        min_block_height: f64,
    ) -> f64 {
        // Calculate available height after accounting for gaps
        let available = target_total_height - (gap_count as f64 * gap_height);

        // First pass: determine which sections need minimum height with naive scale
        let naive_scale = if total_size > 0 {
            available / total_size as f64
        } else {
            1.0
        };

        let mut small_count = 0;
        let mut large_total_size = 0u64;

        for segment in &self.segments {
            let naive_height = segment.size as f64 * naive_scale;
            if naive_height < min_block_height {
                small_count += 1;
            } else {
                large_total_size += segment.size;
            }
        }

        // Calculate final scale factor accounting for minimum heights
        let available_for_large = available - (small_count as f64 * min_block_height);
        if large_total_size > 0 {
            available_for_large / large_total_size as f64
        } else {
            1.0
        }
    }

    fn calculate_region_scale_factor(
        &self,
        total_size: u64,
        gap_count: usize,
        target_total_height: f64,
        gap_height: f64,
        min_block_height: f64,
    ) -> f64 {
        let available = target_total_height - (gap_count as f64 * gap_height);
        let naive_scale = if total_size > 0 {
            available / total_size as f64
        } else {
            1.0
        };

        let mut small_count = 0;
        let mut large_total_size = 0u64;

        for region in &self.memory_regions {
            let naive_height = region.size as f64 * naive_scale;
            if naive_height < min_block_height {
                small_count += 1;
            } else {
                large_total_size += region.size;
            }
        }

        let available_for_large = available - (small_count as f64 * min_block_height);
        if large_total_size > 0 {
            available_for_large / large_total_size as f64
        } else {
            1.0
        }
    }
}

impl Render for MemoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = 20.0;
        let selected_segment = self.selected_segment;

        // Calculate total size of all segments
        let total_size: u64 = self.segments.iter().map(|s| s.size).sum();

        // Count gaps between segments
        let gap_count = self
            .segments
            .windows(2)
            .filter(|pair| {
                let current_end = pair[0].address + pair[0].size;
                current_end < pair[1].address
            })
            .count();

        // Target total height for visualization
        let target_total_height = 600.0;
        let gap_height = 10.0;
        let min_block_height = 20.0;

        let scale_factor = self.calculate_scale_factor(
            total_size,
            gap_count,
            target_total_height,
            gap_height,
            min_block_height,
        );

        // Calculate scale factor for memory regions
        let region_total_size: u64 = self.memory_regions.iter().map(|r| r.size).sum();
        let region_gap_count = self
            .memory_regions
            .windows(2)
            .filter(|pair| {
                let current_end = pair[0].start + pair[0].size;
                current_end < pair[1].start
            })
            .count();

        let region_scale_factor = self.calculate_region_scale_factor(
            region_total_size,
            region_gap_count,
            target_total_height,
            gap_height,
            min_block_height,
        );

        // Check if we have a bottom panel to show
        let has_bottom_panel = self.symbols_table.is_some() || self.selected_dwarf_symbol.is_some();

        // Check if we have a target selected (i.e., memory regions to show)
        let has_target = !self.memory_regions.is_empty();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .relative()
            .child(
                TitleBar::new()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .w_full()
                            .child(
                                div()
                                    .w(px(200.0))
                                    .pr(px(5.0))
                                    .child(
                                        Select::new(&self.theme_select)
                                            .small()
                                            .placeholder("Select theme...")
                                            .search_placeholder("Search themes...")
                                    )
                            )
                    )
            )
            .child(
                // Outer vertical resizable: main content + bottom panel
                v_resizable("main-v-resizable")
                    .child(
                        // Main content area (grows to fill space)
                        resizable_panel()
                            .child(
                                // Inner horizontal resizable: left sidebar + content
                                h_resizable("main-h-resizable")
                                    .child(
                                        // Left sidebar with target selector and DWARF tree
                                        resizable_panel()
                                            .size(px(320.0))
                                            .size_range(px(200.0)..px(500.0))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .size_full()
                                                    .overflow_hidden()
                                                    .bg(cx.theme().sidebar)
                                                    .text_color(cx.theme().sidebar_foreground)
                                                    .border_r_1()
                                                    .border_color(cx.theme().sidebar_border)
                                                    .child(
                                                        div()
                                                            .p_4()
                                                            .border_b_1()
                                                            .border_color(cx.theme().border)
                                                            .child(
                                                                v_flex()
                                                                    .gap_2()
                                                                    .w_full()
                                                                    .child(
                                                                        div()
                                                                            .text_sm()
                                                                            .font_weight(FontWeight::SEMIBOLD)
                                                                            .child("Target Chip")
                                                                    )
                                                                    .child(
                                                                        Select::new(&self.target_select)
                                                                            .small()
                                                                            .placeholder("Select target...")
                                                                            .search_placeholder("Search targets...")
                                                                    )
                                                            )
                                                    )
                                                    .child(
                                                        // DWARF tree panel takes remaining space
                                                        div()
                                                            .flex_1()
                                                            .overflow_hidden()
                                                            .child(self.dwarf_tree_panel.clone())
                                                    )
                                            )
                                    )
                                    .child(
                                        // Main content panels (sections, regions, details)
                                        resizable_panel()
                                            .child(
                                                div()
                                                    .flex()
                                                    .size_full()
                                                    .child(render_sections_panel(
                                                        &self.segments,
                                                        selected_segment,
                                                        scale_factor,
                                                        min_block_height,
                                                        gap_height,
                                                        padding,
                                                        |idx| {
                                                            Box::new(cx.listener(move |view: &mut MemoryView, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<MemoryView>| {
                                                                view.on_segment_click(idx, event, window, cx);
                                                            }))
                                                        },
                                                    ))
                                                    .when(has_target, |d| {
                                                        d.child(render_regions_panel(
                                                            &self.memory_regions,
                                                            region_scale_factor,
                                                            min_block_height,
                                                            gap_height,
                                                            padding,
                                                        ))
                                                    })
                                                    .child(DetailsPanel::new(
                                                        self.defmt_info.clone(),
                                                        self.rtt_info.clone(),
                                                        self.segments.clone(),
                                                        selected_segment,
                                                        total_size,
                                                    ))
                                            )
                                    )
                            )
                    )
                    // Bottom panel: show ELF symbols table OR DWARF symbol details
                    .when(has_bottom_panel, |group| {
                        if let Some(table_state) = self.symbols_table.as_ref() {
                            // ELF segment selected - show symbols table
                            let segment = self.selected_segment
                                .and_then(|idx| self.segments.get(idx))
                                .unwrap();
                            let symbols_count = table_state.read(cx).delegate().symbols.len();

                            group.child(
                                resizable_panel()
                                    .size(px(400.0))
                                    .size_range(px(400.0)..px(800.0))
                                    .child(
                                        gpui_component::v_flex()
                                            .size_full()
                                            .border_t_1()
                                            .border_color(cx.theme().border)
                                            .child(
                                                // Header
                                                gpui::div()
                                                    .px_3()
                                                    .py_2()
                                                    .border_b_1()
                                                    .border_color(cx.theme().border)
                                                    .bg(cx.theme().sidebar)
                                                    .child(
                                                        gpui::div()
                                                            .text_sm()
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!("Symbols in {} ({} total)", segment.name, symbols_count))
                                                    )
                                            )
                                            .child(
                                                Table::new(table_state).stripe(true).bordered(false)
                                            )
                                    )
                            )
                        } else if self.selected_dwarf_symbol.is_some() {
                            // DWARF symbol selected - show details panel at bottom
                            group.child(
                                resizable_panel()
                                    .size(px(400.0))
                                    .size_range(px(400.0)..px(800.0))
                                    .child(
                                        gpui_component::v_flex()
                                            .size_full()
                                            .border_t_1()
                                            .border_color(cx.theme().border)
                                            .child(DwarfDetailsPanel::new(
                                                self.selected_dwarf_symbol.clone(),
                                            ))
                                    )
                            )
                        } else {
                            group
                        }
                    })
            )
    }
}
