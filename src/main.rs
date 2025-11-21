use anyhow::{Context as AnyhowContext, Result};
use gpui::{prelude::*, *};
use object::{Object, ObjectSection};
use probe_rs::config::MemoryRegion as ProbeRsMemoryRegion;
use std::env;
use std::fs;
use std::path::PathBuf;

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
    min_address: u64,
    max_address: u64,
    selected_segment: Option<usize>,
}

impl MemoryView {
    fn new(segments: Vec<MemorySegment>, memory_regions: Vec<MemoryRegion>) -> Self {
        let min_address = segments.iter().map(|s| s.address).min().unwrap_or(0);
        let max_address = segments
            .iter()
            .map(|s| s.address + s.size)
            .max()
            .unwrap_or(0);

        Self {
            segments,
            memory_regions,
            min_address,
            max_address,
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
        // Calculate total size as sum of all section sizes (not address range)
        let total_size: u64 = self.segments.iter().map(|s| s.size).sum();
        let min_height_px = 40.0; // Minimum clickable height
        let padding = 20.0;
        let selected_segment = self.selected_segment;

        // Calculate dynamic scale factor to prevent overflow
        // First pass: count sections that would be below minimum height
        let target_total_height = 600.0;
        let mut sections_at_min = 0;
        let mut remaining_proportion = 0.0;

        for segment in &self.segments {
            let proportion = if total_size > 0 {
                segment.size as f64 / total_size as f64
            } else {
                0.0
            };
            let test_height = proportion * target_total_height;
            if test_height < min_height_px {
                sections_at_min += 1;
            } else {
                remaining_proportion += proportion;
            }
        }

        // Calculate scale factor: remaining space divided by remaining proportion
        let used_by_min = sections_at_min as f64 * min_height_px;
        let available_for_large = target_total_height - used_by_min;
        let scale_factor = if remaining_proportion > 0.0 {
            available_for_large / remaining_proportion
        } else {
            target_total_height
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
                    .child(
                        // ELF Sections visualization panel
                        div()
                            .id("memory_panel")
                            .flex()
                            .flex_col()
                            .w(relative(0.5))
                            .p(px(padding))
                            .gap_2()
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0xaaaaaa))
                                    .mb_3()
                                    .child("ELF Sections"),
                            )
                            .children(self.segments.iter().enumerate().map(|(idx, segment)| {
                                // Calculate proportion based on actual section size
                                let proportion = if total_size > 0 {
                                    segment.size as f64 / total_size as f64
                                } else {
                                    0.0
                                };
                                let calculated_height = proportion * scale_factor;
                                let height = calculated_height.max(min_height_px) as f32;

                                let _is_selected = selected_segment == Some(idx);
                                let has_conflicts = !segment.conflicts.is_empty();
                                let color = Self::generate_color(idx);
                                let white: Hsla = rgb(0xffffff).into();

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
                            })),
                    )
                    .child(self.render_memory_regions_panel())
                    .child(self.render_details_panel(total_size)),
            )
    }
}

impl MemoryView {
    fn render_memory_regions_panel(&self) -> impl IntoElement {
        let total_size: u64 = self.memory_regions.iter().map(|r| r.size).sum();
        let min_height_px = 40.0;
        let padding = 20.0;

        // Calculate dynamic scale factor
        let target_total_height = 600.0;
        let mut regions_at_min = 0;
        let mut remaining_proportion = 0.0;

        for region in &self.memory_regions {
            let proportion = if total_size > 0 {
                region.size as f64 / total_size as f64
            } else {
                0.0
            };
            let test_height = proportion * target_total_height;
            if test_height < min_height_px {
                regions_at_min += 1;
            } else {
                remaining_proportion += proportion;
            }
        }

        let used_by_min = regions_at_min as f64 * min_height_px;
        let available_for_large = target_total_height - used_by_min;
        let scale_factor = if remaining_proportion > 0.0 {
            available_for_large / remaining_proportion
        } else {
            target_total_height
        };

        div()
            .id("regions_panel")
            .flex()
            .flex_col()
            .w(relative(0.5))
            .p(px(padding))
            .gap_2()
            .overflow_y_scroll()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xaaaaaa))
                    .mb_3()
                    .child("Memory Regions"),
            )
            .children(self.memory_regions.iter().map(|region| {
                let proportion = if total_size > 0 {
                    region.size as f64 / total_size as f64
                } else {
                    0.0
                };
                let calculated_height = proportion * scale_factor;
                let height = calculated_height.max(min_height_px) as f32;

                let color = match region.kind {
                    MemoryKind::Flash => hsla(30.0 / 360.0, 0.7, 0.5, 1.0), // Orange
                    MemoryKind::Ram => hsla(200.0 / 360.0, 0.7, 0.5, 1.0),   // Blue
                };

                let white: Hsla = rgb(0xffffff).into();

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
            }))
    }

    fn render_details_panel(&self, total_size: u64) -> impl IntoElement {
        div()
            .id("details_panel")
            .w(px(350.0))
            .bg(rgb(0x252525))
            .border_l_1()
            .border_color(rgb(0x3d3d3d))
            .p_4()
            .overflow_y_scroll()
            .when_some(self.selected_segment, |panel, idx| {
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
            })
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

    let title = format!(
        "ELF Memory Viewer - {}",
        elf_path.file_name().unwrap().to_string_lossy()
    );

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from(title.clone())),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| MemoryView::new(segments.clone(), memory_regions.clone()))
            },
        )
        .unwrap();
        cx.activate(true);
    });

    Ok(())
}
