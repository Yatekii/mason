use anyhow::{Context as AnyhowContext, Result};
use object::{Endianness, Object, ObjectSection, ObjectSymbol};
use probe_rs::config::MemoryRegion as ProbeRsMemoryRegion;
use std::fs;
use std::path::PathBuf;

use crate::types::{
    DefmtInfo, MemoryKind, MemoryRegion, MemorySegment, RttBufferDesc, RttInfo,
};

pub fn get_all_targets() -> Vec<String> {
    let mut targets: Vec<String> = probe_rs::config::families()
        .into_iter()
        .flat_map(|family| {
            family.variants().iter().map(|variant| {
                variant.name.clone()
            }).collect::<Vec<_>>()
        })
        .collect();

    targets.sort();
    eprintln!("Loaded {} targets from probe-rs", targets.len());
    targets
}

pub fn load_memory_layout_from_probe_rs(target_name: &str) -> Result<Vec<MemoryRegion>> {
    // Get the target from probe-rs
    let target = probe_rs::config::get_target_by_name(target_name)
        .context(format!("Failed to find target '{}' in probe-rs", target_name))?;

    let mut regions = Vec::new();

    // Extract memory regions from the target
    for memory_region in &target.memory_map {
        let (start, size, kind) = match memory_region {
            ProbeRsMemoryRegion::Ram(ram) => (
                ram.range.start,
                ram.range.end - ram.range.start,
                MemoryKind::Ram,
            ),
            ProbeRsMemoryRegion::Nvm(nvm) => (
                nvm.range.start,
                nvm.range.end - nvm.range.start,
                MemoryKind::Flash,
            ),
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
                (
                    generic.range.start,
                    generic.range.end - generic.range.start,
                    kind,
                )
            }
        };

        let name = match memory_region {
            ProbeRsMemoryRegion::Ram(ram) => {
                ram.name.clone().unwrap_or_else(|| "RAM".to_string())
            }
            ProbeRsMemoryRegion::Nvm(nvm) => {
                nvm.name.clone().unwrap_or_else(|| "FLASH".to_string())
            }
            ProbeRsMemoryRegion::Generic(generic) => generic
                .name
                .clone()
                .unwrap_or_else(|| "GENERIC".to_string()),
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

pub fn parse_defmt_info(path: &PathBuf) -> Result<DefmtInfo> {
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

pub fn parse_rtt_info(path: &PathBuf) -> Result<RttInfo> {
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
                let (max_up, max_down, up_buffers, down_buffers) = if let Some(rtt_bytes) =
                    rtt_data
                {
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

fn decode_rtt_control_block(
    data: &[u8],
    ptr_size: usize,
    endian: Endianness,
) -> (
    Option<u32>,
    Option<u32>,
    Vec<RttBufferDesc>,
    Vec<RttBufferDesc>,
) {
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
        let bytes = [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ];
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
            let bytes = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ];
            match endian {
                Endianness::Little => u32::from_le_bytes(bytes) as u64,
                Endianness::Big => u32::from_be_bytes(bytes) as u64,
            }
        } else {
            let bytes = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
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

        for i in 0..max_up_count.min(16) {
            // Limit to reasonable number
            let offset = up_buffers_offset + (i as usize * buffer_desc_size);
            if offset + buffer_desc_size > data.len() {
                break;
            }

            if let (Some(buffer_addr), Some(buffer_size)) = (
                read_ptr(offset + ptr_size),
                read_u32(offset + 2 * ptr_size),
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
                read_u32(offset + 2 * ptr_size),
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

pub fn parse_elf_segments(
    path: &PathBuf,
    memory_regions: &[MemoryRegion],
) -> Result<Vec<MemorySegment>> {
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

            let name = section.name().unwrap_or("<unnamed>").to_string();

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
