use anyhow::{Context as AnyhowContext, Result};
use gpui::{prelude::*, *};
use object::{Object, ObjectSection, ObjectSymbol, Endianness};
use probe_rs::config::MemoryRegion as ProbeRsMemoryRegion;
use std::env;
use std::fs;
use std::path::PathBuf;

actions!(mason, [Quit]);

#[derive(Clone, Debug)]
struct MemoryRegion {
    name: String,
    start: u64,
    size: u64,
    kind: MemoryKind,
}

#[derive(Clone, Debug, PartialEq)]
enum MemoryKind {
    Flash,
    Ram,
}

#[derive(Clone, Debug)]
struct MemorySegment {
    name: String,
    address: u64,
    size: u64,
    flags: String,
    is_load: bool,
    conflicts: Vec<String>,
}

#[derive(Clone, Debug)]
struct DefmtInfo {
    present: bool,
    sections: Vec<(String, u64)>, // (section_name, size)
}

#[derive(Clone, Debug)]
struct RttBufferDesc {
    name: String,
    buffer_address: u64,
    size: u32,
}

#[derive(Clone, Debug)]
struct RttInfo {
    present: bool,
    symbol_name: Option<String>,
    address: Option<u64>,
    size: Option<u64>,
    max_up_buffers: Option<u32>,
    max_down_buffers: Option<u32>,
    up_buffers: Vec<RttBufferDesc>,
    down_buffers: Vec<RttBufferDesc>,
}

impl MemoryRegion {
    fn contains(&self, address: u64, size: u64) -> bool {
        let end = address + size;
        let region_end = self.start + self.size;
        address >= self.start && end <= region_end
    }

    fn overlaps(&self, address: u64, size: u64) -> bool {
        let end = address + size;
        let region_end = self.start + self.size;
        !(end <= self.start || address >= region_end)
    }
}

fn load_memory_layout_from_probe_rs(target_name: &str) -> Result<Vec<MemoryRegion>> {
    // Get the target from probe-rs
    let target = probe_rs::config::get_target_by_name(target_name)
        .context(format!("Failed to find target '{}' in probe-rs", target_name))?;

    let mut regions = Vec::new();

    // Extract memory regions from the target
    for memory_region in &target.memory_map {
        let (start, size, kind) = match memory_region {
            ProbeRsMemoryRegion::Ram(ram) => {
                (ram.range.start, ram.range.end - ram.range.start, MemoryKind::Ram)
            }
            ProbeRsMemoryRegion::Nvm(nvm) => {
                (nvm.range.start, nvm.range.end - nvm.range.start, MemoryKind::Flash)
            }
            ProbeRsMemoryRegion::Generic(generic) => {
                // Try to infer type from name
                let kind = if let Some(name) = &generic.name {
                    if name.to_lowercase().contains("ram") {
                        MemoryKind::Ram
                    } else {
                        MemoryKind::Flash
                    }
                } else {
                    MemoryKind::Flash
                };
                (generic.range.start, generic.range.end - generic.range.start, kind)
            }
        };

        let name = match memory_region {
            ProbeRsMemoryRegion::Ram(ram) => ram.name.clone().unwrap_or_else(|| "RAM".to_string()),
            ProbeRsMemoryRegion::Nvm(nvm) => nvm.name.clone().unwrap_or_else(|| "FLASH".to_string()),
            ProbeRsMemoryRegion::Generic(generic) => generic.name.clone().unwrap_or_else(|| "GENERIC".to_string()),
        };

        regions.push(MemoryRegion {
            name,
            start,
            size,
            kind,
        });
    }

    if regions.is_empty() {
        anyhow::bail!("No memory regions found in target '{}'", target_name);
    }

    // Sort by start address
    regions.sort_by_key(|r| r.start);

    Ok(regions)
}


fn parse_defmt_info(path: &PathBuf) -> Result<DefmtInfo> {
    let data = fs::read(path).context("Failed to read ELF file")?;
    let obj = object::File::parse(&*data).context("Failed to parse ELF file")?;

    let mut defmt_sections = Vec::new();

    for section in obj.sections() {
        let name = section.name().unwrap_or("");
        // Look for defmt-related sections
        if name.starts_with(".defmt") || name.contains("defmt") {
            let size = section.size();
            if size > 0 {
                defmt_sections.push((name.to_string(), size));
            }
        }
    }

    Ok(DefmtInfo {
        present: !defmt_sections.is_empty(),
        sections: defmt_sections,
    })
}

fn parse_rtt_info(path: &PathBuf) -> Result<RttInfo> {
    let data = fs::read(path).context("Failed to read ELF file")?;
    let obj = object::File::parse(&*data).context("Failed to parse ELF file")?;

    // Determine if this is 32-bit or 64-bit
    let is_64bit = obj.is_64();
    let ptr_size = if is_64bit { 8 } else { 4 };

    // Look for RTT control block symbol
    for symbol in obj.symbols() {
        if let Ok(name) = symbol.name() {
            // Common RTT symbol names
            if name == "_SEGGER_RTT" || name == "SEGGER_RTT" || name.contains("_SEGGER_RTT") {
                let address = symbol.address();
                let size = symbol.size();

                // Try to find the section containing this address and read the data
                let mut rtt_data: Option<&[u8]> = None;
                for section in obj.sections() {
                    let section_addr = section.address();
                    let section_size = section.size();
                    if address >= section_addr && address < section_addr + section_size {
                        if let Ok(section_data) = section.data() {
                            let offset = (address - section_addr) as usize;
                            if offset < section_data.len() {
                                rtt_data = Some(&section_data[offset..]);
                            }
                        }
                        break;
                    }
                }

                // Parse RTT control block structure if we have data
                let (max_up, max_down, up_buffers, down_buffers) = if let Some(rtt_bytes) = rtt_data {
                    decode_rtt_control_block(rtt_bytes, ptr_size, obj.endianness())
                } else {
                    (None, None, Vec::new(), Vec::new())
                };

                return Ok(RttInfo {
                    present: true,
                    symbol_name: Some(name.to_string()),
                    address: Some(address),
                    size: if size > 0 { Some(size) } else { None },
                    max_up_buffers: max_up,
                    max_down_buffers: max_down,
                    up_buffers,
                    down_buffers,
                });
            }
        }
    }

    Ok(RttInfo {
        present: false,
        symbol_name: None,
        address: None,
        size: None,
        max_up_buffers: None,
        max_down_buffers: None,
        up_buffers: Vec::new(),
        down_buffers: Vec::new(),
    })
}

fn decode_rtt_control_block(data: &[u8], ptr_size: usize, endian: Endianness) -> (Option<u32>, Option<u32>, Vec<RttBufferDesc>, Vec<RttBufferDesc>) {
    if data.len() < 24 {
        return (None, None, Vec::new(), Vec::new());
    }

    // RTT Control Block structure:
    // char acID[16];           // offset 0
    // int MaxNumUpBuffers;     // offset 16
    // int MaxNumDownBuffers;   // offset 20
    // SEGGER_RTT_BUFFER_UP aUp[...];     // offset 24
    // SEGGER_RTT_BUFFER_DOWN aDown[...]; // after up buffers

    let read_u32 = |offset: usize| -> Option<u32> {
        if offset + 4 > data.len() {
            return None;
        }
        let bytes = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
        Some(match endian {
            Endianness::Little => u32::from_le_bytes(bytes),
            Endianness::Big => u32::from_be_bytes(bytes),
        })
    };

    let read_ptr = |offset: usize| -> Option<u64> {
        if offset + ptr_size > data.len() {
            return None;
        }
        Some(if ptr_size == 4 {
            let bytes = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
            match endian {
                Endianness::Little => u32::from_le_bytes(bytes) as u64,
                Endianness::Big => u32::from_be_bytes(bytes) as u64,
            }
        } else {
            let bytes = [
                data[offset], data[offset+1], data[offset+2], data[offset+3],
                data[offset+4], data[offset+5], data[offset+6], data[offset+7]
            ];
            match endian {
                Endianness::Little => u64::from_le_bytes(bytes),
                Endianness::Big => u64::from_be_bytes(bytes),
            }
        })
    };

    let max_up = read_u32(16);
    let max_down = read_u32(20);

    let mut up_buffers = Vec::new();
    let mut down_buffers = Vec::new();

    // Parse buffer descriptors
    // Each buffer descriptor:
    // const char* sName;      // offset 0
    // char* pBuffer;          // offset ptr_size
    // unsigned int SizeOfBuffer; // offset 2*ptr_size
    // unsigned int WrOff;     // offset 2*ptr_size + 4
    // unsigned int RdOff;     // offset 2*ptr_size + 8
    // unsigned int Flags;     // offset 2*ptr_size + 12
    let buffer_desc_size = 2 * ptr_size + 16;

    if let (Some(max_up_count), Some(max_down_count)) = (max_up, max_down) {
        let up_buffers_offset = 24;

        for i in 0..max_up_count.min(16) { // Limit to reasonable number
            let offset = up_buffers_offset + (i as usize * buffer_desc_size);
            if offset + buffer_desc_size > data.len() {
                break;
            }

            if let (Some(buffer_addr), Some(buffer_size)) = (
                read_ptr(offset + ptr_size),
                read_u32(offset + 2 * ptr_size)
            ) {
                if buffer_addr != 0 && buffer_size > 0 {
                    up_buffers.push(RttBufferDesc {
                        name: format!("Up {}", i),
                        buffer_address: buffer_addr,
                        size: buffer_size,
                    });
                }
            }
        }

        let down_buffers_offset = up_buffers_offset + (max_up_count as usize * buffer_desc_size);

        for i in 0..max_down_count.min(16) {
            let offset = down_buffers_offset + (i as usize * buffer_desc_size);
            if offset + buffer_desc_size > data.len() {
                break;
            }

            if let (Some(buffer_addr), Some(buffer_size)) = (
                read_ptr(offset + ptr_size),
                read_u32(offset + 2 * ptr_size)
            ) {
                if buffer_addr != 0 && buffer_size > 0 {
                    down_buffers.push(RttBufferDesc {
                        name: format!("Down {}", i),
                        buffer_address: buffer_addr,
                        size: buffer_size,
                    });
                }
            }
        }
    }

    (max_up, max_down, up_buffers, down_buffers)
}

fn parse_elf_segments(path: &PathBuf, memory_regions: &[MemoryRegion]) -> Result<Vec<MemorySegment>> {
    let data = fs::read(path).context("Failed to read ELF file")?;
    let obj = object::File::parse(&*data).context("Failed to parse ELF file")?;

    let mut segments = Vec::new();

    for section in obj.sections() {
        let address = section.address();
        let size = section.size();

        // Only include allocated sections with non-zero size and valid addresses
        if size > 0 && address > 0 {
            let section_flags = section.flags();

            // Check if section is allocated (loaded into memory)
            let is_allocated = match section_flags {
                object::SectionFlags::Elf { sh_flags } => (sh_flags & 0x2) != 0, // SHF_ALLOC
                _ => false,
            };

            if !is_allocated {
                continue;
            }

            // Check if this section has file data that will be loaded
            // Sections like .bss have SHT_NOBITS and no file data
            let file_range = section.file_range();
            let is_load = if let Some((_, file_size)) = file_range {
                file_size > 0
            } else {
                false
            };

            // Build flags string based on section attributes
            // Extract raw flags for ELF sections
            let (is_writable, is_executable) = match section_flags {
                object::SectionFlags::Elf { sh_flags } => {
                    let writable = (sh_flags & 0x1) != 0; // SHF_WRITE
                    let executable = (sh_flags & 0x4) != 0; // SHF_EXECINSTR
                    (writable, executable)
                }
                _ => (false, false),
            };

            let flags = format!(
                "{}{}{}",
                "R", // All allocated sections are readable
                if is_writable { "W" } else { "-" },
                if is_executable { "X" } else { "-" }
            );

            let name = section
                .name()
                .unwrap_or("<unnamed>")
                .to_string();

            segments.push(MemorySegment {
                name,
                address,
                size,
                flags,
                is_load,
                conflicts: Vec::new(),
            });
        }
    }

    segments.sort_by_key(|s| s.address);

    // Detect conflicts
    detect_conflicts(&mut segments, memory_regions);

    Ok(segments)
}

fn detect_conflicts(segments: &mut [MemorySegment], memory_regions: &[MemoryRegion]) {
    // Check for overlaps between segments
    for i in 0..segments.len() {
        let mut conflicts = Vec::new();

        let seg_start = segments[i].address;
        let seg_end = seg_start + segments[i].size;

        // Check overlap with other segments
        for j in 0..segments.len() {
            if i == j {
                continue;
            }

            let other_start = segments[j].address;
            let other_end = other_start + segments[j].size;

            if !(seg_end <= other_start || seg_start >= other_end) {
                conflicts.push(format!("Overlaps with {}", segments[j].name));
            }
        }

        // Check if segment is within valid memory regions
        let mut in_valid_region = false;

        for region in memory_regions {
            if region.contains(seg_start, segments[i].size) {
                in_valid_region = true;
                break;
            } else if region.overlaps(seg_start, segments[i].size) {
                conflicts.push(format!("Partially outside {} region", region.name));
                in_valid_region = true;
            }
        }

        if !in_valid_region {
            conflicts.push("Not in any defined memory region".to_string());
        }

        segments[i].conflicts = conflicts;
    }
}

struct MemoryView {
    segments: Vec<MemorySegment>,
    memory_regions: Vec<MemoryRegion>,
    defmt_info: DefmtInfo,
    rtt_info: RttInfo,
    selected_segment: Option<usize>,
}

impl MemoryView {
    fn new(segments: Vec<MemorySegment>, memory_regions: Vec<MemoryRegion>, defmt_info: DefmtInfo, rtt_info: RttInfo) -> Self {
        Self {
            segments,
            memory_regions,
            defmt_info,
            rtt_info,
            selected_segment: None,
        }
    }

    fn generate_color(index: usize) -> Hsla {
        // Generate visually distinct colors using golden ratio
        let hue = (index as f32 * 137.508) % 360.0;
        hsla(hue / 360.0, 0.7, 0.6, 1.0)
    }

    fn on_segment_click(
        &mut self,
        idx: usize,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_segment = Some(idx);
        cx.notify();
    }
}

impl Render for MemoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = 20.0;
        let selected_segment = self.selected_segment;

        // Calculate total size of all segments
        let total_size: u64 = self.segments.iter().map(|s| s.size).sum();

        // Count gaps between segments
        let gap_count = self.segments.windows(2).filter(|pair| {
            let current_end = pair[0].address + pair[0].size;
            current_end < pair[1].address
        }).count();

        // Target total height for visualization
        let target_total_height = 600.0;
        let gap_height = 10.0;
        let min_block_height = 20.0;

        // Calculate available height after accounting for gaps
        let available_for_sections = target_total_height - (gap_count as f64 * gap_height);

        // First pass: determine which sections need minimum height with naive scale
        let naive_scale = if total_size > 0 {
            available_for_sections / total_size as f64
        } else {
            1.0
        };

        let mut small_sections_count = 0;
        let mut large_sections_total_size = 0u64;

        for segment in &self.segments {
            let naive_height = segment.size as f64 * naive_scale;
            if naive_height < min_block_height {
                small_sections_count += 1;
            } else {
                large_sections_total_size += segment.size;
            }
        }

        // Calculate final scale factor accounting for minimum heights
        let available_for_large_sections = available_for_sections - (small_sections_count as f64 * min_block_height);
        let scale_factor = if large_sections_total_size > 0 {
            available_for_large_sections / large_sections_total_size as f64
        } else {
            1.0
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .child(
                // Title bar
                div()
                    .flex()
                    .p_4()
                    .bg(rgb(0x2d2d2d))
                    .border_b_1()
                    .border_color(rgb(0x3d3d3d))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xffffff))
                            .child("ELF Memory Sections"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    .child({
                        // ELF Sections visualization panel
                        let mut sections_panel = div()
                            .id("memory_panel")
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
                                    .child("ELF Sections"),
                            );

                        for (idx, segment) in self.segments.iter().enumerate() {
                            // Calculate height based on actual memory size
                            let height = (segment.size as f64 * scale_factor).max(min_block_height) as f32;

                            let _is_selected = selected_segment == Some(idx);
                            let has_conflicts = !segment.conflicts.is_empty();
                            let color = Self::generate_color(idx);
                            let white: Hsla = rgb(0xffffff).into();

                            sections_panel = sections_panel.child(
                                div()
                                    .id(idx)
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .h(px(height))
                                    .px_3()
                                    .gap_3()
                                    .bg(color)
                                    .when(has_conflicts, |div| {
                                        div.border_2().border_color(rgb(0xff0000))
                                    })
                                    .shadow_lg()
                                    .hover(|style| style.shadow_xl().cursor_pointer())
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |view, event, window, cx| {
                                            view.on_segment_click(idx, event, window, cx)
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(white)
                                            .flex_shrink_0()
                                            .child(format!("{}", segment.name)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(white.opacity(0.9))
                                            .flex_shrink_0()
                                            .child(format!("0x{:016x}", segment.address)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(white.opacity(0.9))
                                            .flex_shrink_0()
                                            .child(format!("{}", format_size(segment.size))),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(white.opacity(0.9))
                                            .flex_shrink_0()
                                            .child(format!("{}", segment.flags)),
                                    )
                            );

                            // Check if there's a gap between this segment and the next
                            if idx + 1 < self.segments.len() {
                                let next_segment = &self.segments[idx + 1];
                                let current_end = segment.address + segment.size;
                                if current_end < next_segment.address {
                                    // There's a gap, insert a visual separator
                                    sections_panel = sections_panel.child(
                                        div()
                                            .h(px(gap_height as f32))
                                            .bg(rgb(0x1e1e1e))
                                    );
                                }
                            }
                        }

                        sections_panel
                    })
                    .child(self.render_memory_regions_panel())
                    .child(self.render_details_panel(total_size)),
            )
    }
}

impl MemoryView {
    fn render_memory_regions_panel(&self) -> impl IntoElement {
        let padding = 20.0;

        // Calculate total size of all regions
        let total_size: u64 = self.memory_regions.iter().map(|r| r.size).sum();

        // Count gaps between regions
        let gap_count = self.memory_regions.windows(2).filter(|pair| {
            let current_end = pair[0].start + pair[0].size;
            current_end < pair[1].start
        }).count();

        let target_total_height = 600.0;
        let gap_height = 10.0;
        let min_block_height = 20.0;

        // Calculate available height after accounting for gaps
        let available_for_regions = target_total_height - (gap_count as f64 * gap_height);

        // First pass: determine which regions need minimum height with naive scale
        let naive_scale = if total_size > 0 {
            available_for_regions / total_size as f64
        } else {
            1.0
        };

        let mut small_regions_count = 0;
        let mut large_regions_total_size = 0u64;

        for region in &self.memory_regions {
            let naive_height = region.size as f64 * naive_scale;
            if naive_height < min_block_height {
                small_regions_count += 1;
            } else {
                large_regions_total_size += region.size;
            }
        }

        // Calculate final scale factor accounting for minimum heights
        let available_for_large_regions = available_for_regions - (small_regions_count as f64 * min_block_height);
        let scale_factor = if large_regions_total_size > 0 {
            available_for_large_regions / large_regions_total_size as f64
        } else {
            1.0
        };

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

        for (i, region) in self.memory_regions.iter().enumerate() {
            let height = (region.size as f64 * scale_factor).max(min_block_height) as f32;

            let color = match region.kind {
                MemoryKind::Flash => hsla(30.0 / 360.0, 0.7, 0.5, 1.0), // Orange
                MemoryKind::Ram => hsla(200.0 / 360.0, 0.7, 0.5, 1.0),   // Blue
            };

            let white: Hsla = rgb(0xffffff).into();

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
                            .text_color(white)
                            .flex_shrink_0()
                            .child(format!("{}", region.name)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(white.opacity(0.9))
                            .flex_shrink_0()
                            .child(format!("0x{:08x}", region.start)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(white.opacity(0.9))
                            .flex_shrink_0()
                            .child(format!("{}", format_size(region.size))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(white.opacity(0.9))
                            .flex_shrink_0()
                            .child(format!("{:?}", region.kind)),
                    )
            );

            // Check if there's a gap between this region and the next
            if i + 1 < self.memory_regions.len() {
                let next_region = &self.memory_regions[i + 1];
                let current_end = region.start + region.size;
                if current_end < next_region.start {
                    // There's a gap, insert a visual separator
                    panel = panel.child(
                        div()
                            .h(px(gap_height as f32))
                            .bg(rgb(0x1e1e1e))
                    );
                }
            }
        }

        panel
    }

    fn render_details_panel(&self, total_size: u64) -> impl IntoElement {
        let mut panel = div()
            .id("details_panel")
            .flex()
            .flex_col()
            .w(px(350.0))
            .h_full()
            .bg(rgb(0x252525))
            .border_l_1()
            .border_color(rgb(0x3d3d3d))
            .p_4()
            .overflow_y_scroll();

        // Add defmt info section if present
        if self.defmt_info.present {
            panel = panel.child(
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
                    .child(detail_row(
                        "Status",
                        "Present"
                    ))
                    .child(detail_row(
                        "Sections",
                        format!("{}", self.defmt_info.sections.len())
                    ))
                    .children(self.defmt_info.sections.iter().map(|(name, size)| {
                        detail_row(name.clone(), format_size(*size))
                    }))
            );
        }

        // Add RTT info section if present
        if self.rtt_info.present {
            let mut rtt_section = div()
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
                .child(detail_row(
                    "Status",
                    "Present"
                ))
                .when_some(self.rtt_info.symbol_name.as_ref(), |parent, name| {
                    parent.child(detail_row("Symbol", name.clone()))
                })
                .when_some(self.rtt_info.address, |parent, addr| {
                    parent.child(detail_row("Address", format!("0x{:08x}", addr)))
                })
                .when_some(self.rtt_info.size, |parent, size| {
                    parent.child(detail_row("Size", format_size(size)))
                });

            // Add buffer configuration info
            if let Some(max_up) = self.rtt_info.max_up_buffers {
                rtt_section = rtt_section.child(detail_row("Max Up Buffers", format!("{}", max_up)));
            }
            if let Some(max_down) = self.rtt_info.max_down_buffers {
                rtt_section = rtt_section.child(detail_row("Max Down Buffers", format!("{}", max_down)));
            }

            // Add up buffers
            if !self.rtt_info.up_buffers.is_empty() {
                rtt_section = rtt_section.child(
                    div()
                        .mt_2()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xaaaaaa))
                        .child("Up Buffers:")
                );
                for buffer in &self.rtt_info.up_buffers {
                    rtt_section = rtt_section.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .ml_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x888888))
                                    .child(format!("{}:", buffer.name))
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0xcccccc))
                                    .child(format!("  Address: 0x{:08x}, Size: {}", buffer.buffer_address, format_size(buffer.size as u64)))
                            )
                    );
                }
            }

            // Add down buffers
            if !self.rtt_info.down_buffers.is_empty() {
                rtt_section = rtt_section.child(
                    div()
                        .mt_2()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xaaaaaa))
                        .child("Down Buffers:")
                );
                for buffer in &self.rtt_info.down_buffers {
                    rtt_section = rtt_section.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .ml_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x888888))
                                    .child(format!("{}:", buffer.name))
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0xcccccc))
                                    .child(format!("  Address: 0x{:08x}, Size: {}", buffer.buffer_address, format_size(buffer.size as u64)))
                            )
                    );
                }
            }

            panel = panel.child(rtt_section);
        }

        panel = panel.when_some(self.selected_segment, |panel, idx| {
                let segment = &self.segments[idx];
                panel.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xffffff))
                                .mb_3()
                                .child("Selected Section"),
                        )
                        .child(detail_row("Name", segment.name.clone()))
                        .child(detail_row(
                            "Start",
                            format!("0x{:016x}", segment.address),
                        ))
                        .child(detail_row(
                            "End",
                            format!("0x{:016x}", segment.address + segment.size),
                        ))
                        .child(detail_row("Size", format_size(segment.size)))
                        .child(detail_row("Flags", segment.flags.clone()))
                        .child(detail_row("Type", if segment.is_load { "LOAD" } else { "Non-LOAD" }))
                        .child(detail_row(
                            "Percentage",
                            format!("{:.2}%", (segment.size as f64 / total_size as f64) * 100.0),
                        ))
                        .when(!segment.conflicts.is_empty(), |parent| {
                            parent.child(
                                div()
                                    .mt_3()
                                    .pt_3()
                                    .border_t_1()
                                    .border_color(rgb(0xff0000))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0xff4444))
                                            .mb_2()
                                            .child("⚠ Conflicts"),
                                    )
                                    .children(segment.conflicts.iter().map(|conflict| {
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0xff8888))
                                            .mb_1()
                                            .child(format!("• {}", conflict))
                                    })),
                            )
                        }),
                )
            });

        panel
    }
}

fn detail_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> impl IntoElement {
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

fn format_size(bytes: u64) -> String {
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

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <elf-file> --target <probe-rs-target>", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} firmware.elf --target STM32F407VGTx", args[0]);
        eprintln!();
        eprintln!("To list available probe-rs targets, run:");
        eprintln!("  probe-rs chip list");
        std::process::exit(1);
    }

    let elf_path = PathBuf::from(&args[1]);

    if !elf_path.exists() {
        eprintln!("Error: File '{}' does not exist", elf_path.display());
        std::process::exit(1);
    }

    // Parse target argument
    if args[2] != "--target" && args[2] != "-t" {
        eprintln!("Error: Expected --target flag");
        eprintln!("Usage: {} <elf-file> --target <probe-rs-target>", args[0]);
        std::process::exit(1);
    }

    if args.len() < 4 {
        eprintln!("Error: --target requires a target name");
        std::process::exit(1);
    }

    let memory_regions = load_memory_layout_from_probe_rs(&args[3])
        .context("Failed to load target from probe-rs")?;

    let segments =
        parse_elf_segments(&elf_path, &memory_regions).context("Failed to parse ELF segments")?;

    if segments.is_empty() {
        eprintln!("Warning: No loadable segments found in ELF file");
    }

    let defmt_info = parse_defmt_info(&elf_path).context("Failed to parse defmt info")?;
    let rtt_info = parse_rtt_info(&elf_path).context("Failed to parse RTT info")?;

    let title = format!(
        "ELF Memory Viewer - {}",
        elf_path.file_name().unwrap().to_string_lossy()
    );

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);

        // Set up keyboard bindings
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);

        // Handle quit action
        cx.on_action(|_: &Quit, cx| cx.quit());

        let window = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from(title.clone())),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| MemoryView::new(segments.clone(), memory_regions.clone(), defmt_info.clone(), rtt_info.clone()))
            },
        )
        .unwrap();

        // Get the view entity and observe when it's released (window closed)
        let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        cx.observe_release(&view, |_, cx| cx.quit()).detach();

        cx.activate(true);
    });

    Ok(())
}
