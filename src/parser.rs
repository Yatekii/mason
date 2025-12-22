use anyhow::{Context as AnyhowContext, Result};
use object::{Endianness, Object, ObjectSection, ObjectSymbol};
use probe_rs::config::MemoryRegion as ProbeRsMemoryRegion;
use std::fs;
use std::path::PathBuf;

use crate::types::{
    DefmtInfo, DwarfInfo, DwarfSymbol, DwarfTag, ElfSymbol, MemoryKind, MemoryRegion,
    MemorySegment, RttBufferDesc, RttInfo,
};

pub fn get_all_targets() -> Vec<String> {
    let mut targets: Vec<String> = probe_rs::config::families()
        .into_iter()
        .flat_map(|family| {
            family
                .variants()
                .iter()
                .map(|variant| variant.name.clone())
                .collect::<Vec<_>>()
        })
        .collect();

    targets.sort();
    eprintln!("Loaded {} targets from probe-rs", targets.len());
    targets
}

pub fn load_memory_layout_from_probe_rs(target_name: &str) -> Result<Vec<MemoryRegion>> {
    // Get the target from probe-rs
    let target = probe_rs::config::get_target_by_name(target_name).context(format!(
        "Failed to find target '{}' in probe-rs",
        target_name
    ))?;

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
            ProbeRsMemoryRegion::Ram(ram) => ram.name.clone().unwrap_or_else(|| "RAM".to_string()),
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
                let (max_up, max_down, up_buffers, down_buffers) = if let Some(rtt_bytes) = rtt_data
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

            if let (Some(buffer_addr), Some(buffer_size)) =
                (read_ptr(offset + ptr_size), read_u32(offset + 2 * ptr_size))
            {
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

            if let (Some(buffer_addr), Some(buffer_size)) =
                (read_ptr(offset + ptr_size), read_u32(offset + 2 * ptr_size))
            {
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
    memory_regions: Option<&[MemoryRegion]>,
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

    // Detect conflicts (only if memory regions are provided)
    if let Some(regions) = memory_regions {
        detect_conflicts(&mut segments, regions);
    }

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

pub fn parse_elf_symbols(path: &PathBuf) -> Result<Vec<ElfSymbol>> {
    let data = fs::read(path).context("Failed to read ELF file")?;
    let obj = object::File::parse(&*data).context("Failed to parse ELF file")?;

    let mut symbols = Vec::new();

    for symbol in obj.symbols() {
        // Only include symbols with valid names and addresses
        if let Ok(name) = symbol.name() {
            let address = symbol.address();
            let size = symbol.size();

            // Skip symbols with zero address or empty names
            if address > 0 && !name.is_empty() {
                symbols.push(ElfSymbol {
                    name: name.to_string(),
                    address,
                    size,
                });
            }
        }
    }

    // Sort by address
    symbols.sort_by_key(|s| s.address);

    Ok(symbols)
}

pub fn parse_dwarf_info(path: &PathBuf) -> Result<DwarfInfo> {
    use gimli::RunTimeEndian;
    use object::{Object, ObjectSection};

    let data = fs::read(path).context("Failed to read ELF file")?;
    let obj = object::File::parse(&*data).context("Failed to parse ELF file")?;

    // Determine endianness
    let endian = if obj.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };

    // Load DWARF sections
    let load_section = |id: gimli::SectionId| -> Result<std::borrow::Cow<[u8]>, gimli::Error> {
        match obj.section_by_name(id.name()) {
            Some(section) => Ok(section
                .uncompressed_data()
                .unwrap_or(std::borrow::Cow::Borrowed(&[][..]))),
            None => Ok(std::borrow::Cow::Borrowed(&[][..])),
        }
    };

    // Load all sections
    let dwarf_cow = gimli::Dwarf::load(&load_section)?;

    // Borrow the sections for parsing
    let dwarf = dwarf_cow.borrow(|section| gimli::EndianSlice::new(&*section, endian));

    let mut compile_units = Vec::new();
    let mut total_symbols = 0;
    let mut id_counter = 0;

    // Iterate over compilation units
    let mut units = dwarf.units();
    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;

        // Get the compilation unit DIE
        let mut entries = unit.entries();

        if let Some((_, entry)) = entries.next_dfs()? {
            if entry.tag() == gimli::DW_TAG_compile_unit {
                let (cu_symbol, cu_count) =
                    parse_compile_unit(&dwarf, &unit, entry, &mut id_counter)?;
                total_symbols += cu_count;
                compile_units.push(cu_symbol);
            }
        }
    }

    Ok(DwarfInfo {
        present: !compile_units.is_empty(),
        compile_units,
        total_symbols,
    })
}

fn parse_compile_unit<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
    id_counter: &mut usize,
) -> Result<(DwarfSymbol, usize)> {
    let mut symbol_count = 1;

    // Get compilation unit name
    let name = get_string_attr(dwarf, unit, entry, gimli::DW_AT_name)
        .unwrap_or_else(|| "<unknown>".to_string());

    // Get compilation directory for full path
    let comp_dir = get_string_attr(dwarf, unit, entry, gimli::DW_AT_comp_dir);

    let file = comp_dir
        .map(|dir| format!("{}/{}", dir, name))
        .or(Some(name.clone()));

    let id = *id_counter;
    *id_counter += 1;

    // Parse children recursively using entries_tree for proper hierarchy
    let mut children = Vec::new();
    let mut tree = unit.entries_tree(None)?;
    let root = tree.root()?;

    // Iterate over direct children of the compile unit
    let mut child_iter = root.children();
    while let Some(child_node) = child_iter.next()? {
        if let Some((child_symbol, count)) =
            parse_die_recursive(dwarf, unit, child_node, id_counter)?
        {
            symbol_count += count;
            children.push(child_symbol);
        }
    }

    // Sort children: functions first (by address), then types, then variables
    children.sort_by(|a, b| {
        let tag_order = |tag: &DwarfTag| match tag {
            DwarfTag::Subprogram => 0,
            DwarfTag::Variable => 1,
            DwarfTag::StructureType | DwarfTag::UnionType | DwarfTag::EnumerationType => 2,
            DwarfTag::Typedef => 3,
            DwarfTag::Namespace => 4,
            _ => 5,
        };

        let ord = tag_order(&a.tag).cmp(&tag_order(&b.tag));
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }

        // Within same tag type, sort by address if available, then by name
        match (a.address, b.address) {
            (Some(addr_a), Some(addr_b)) => addr_a.cmp(&addr_b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        }
    });

    Ok((
        DwarfSymbol {
            id,
            name: name.clone(),
            tag: DwarfTag::CompileUnit,
            address: None,
            size: None,
            file,
            line: None,
            column: None,
            type_name: None,
            children,
            attributes: Vec::new(),
        },
        symbol_count,
    ))
}

/// Recursively parse a DIE and all its children
fn parse_die_recursive<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    node: gimli::EntriesTreeNode<R>,
    id_counter: &mut usize,
) -> Result<Option<(DwarfSymbol, usize)>> {
    let entry = node.entry();
    let tag = entry.tag();

    // Filter to interesting tags only
    let dwarf_tag = match tag {
        gimli::DW_TAG_subprogram => DwarfTag::Subprogram,
        gimli::DW_TAG_variable => DwarfTag::Variable,
        gimli::DW_TAG_formal_parameter => DwarfTag::FormalParameter,
        gimli::DW_TAG_lexical_block => DwarfTag::LexicalBlock,
        gimli::DW_TAG_inlined_subroutine => DwarfTag::InlinedSubroutine,
        gimli::DW_TAG_structure_type => DwarfTag::StructureType,
        gimli::DW_TAG_union_type => DwarfTag::UnionType,
        gimli::DW_TAG_enumeration_type => DwarfTag::EnumerationType,
        gimli::DW_TAG_member => DwarfTag::Member,
        gimli::DW_TAG_typedef => DwarfTag::Typedef,
        gimli::DW_TAG_namespace => DwarfTag::Namespace,
        gimli::DW_TAG_enumerator => DwarfTag::Member, // Treat enum variants as members
        _ => {
            // Skip this DIE but still process its children (they might be interesting)
            let mut child_iter = node.children();
            while let Some(_child_node) = child_iter.next()? {
                // Just consume them - uninteresting parent means we skip children too
            }
            return Ok(None);
        }
    };

    let id = *id_counter;
    *id_counter += 1;

    // Get name (with demangling)
    let raw_name = get_string_attr(dwarf, unit, entry, gimli::DW_AT_name);
    let linkage_name = get_string_attr(dwarf, unit, entry, gimli::DW_AT_linkage_name);

    let name = linkage_name
        .or(raw_name)
        .map(|n| demangle_name(&n))
        .unwrap_or_else(|| match dwarf_tag {
            DwarfTag::LexicalBlock => "<block>".to_string(),
            DwarfTag::InlinedSubroutine => "<inlined>".to_string(),
            _ => "<anonymous>".to_string(),
        });

    // Get address
    let address = get_address_attr(unit, entry, gimli::DW_AT_low_pc);

    // Get size (from high_pc - low_pc or byte_size)
    let size = get_size(unit, entry);

    // Get file/line info
    let (file, line, column) = get_file_line_info(dwarf, unit, entry);

    // Get type name
    let type_name = get_type_name(dwarf, unit, entry);

    // Build attributes list - capture ALL DWARF attributes without exception
    let mut attributes = Vec::new();

    // Iterate over all attributes in the DIE
    let mut attrs = entry.attrs();
    while let Ok(Some(attr)) = attrs.next() {
        let attr_name = attr.name().static_string().unwrap_or("Unknown");
        // Format the value, or show raw debug representation if we can't format it nicely
        let attr_value =
            format_attr_value(dwarf, unit, &attr).unwrap_or_else(|| format!("{:?}", attr.value()));
        attributes.push((attr_name.to_string(), attr_value));
    }

    // Parse children recursively
    let mut children = Vec::new();
    let mut symbol_count = 1;

    let mut child_iter = node.children();
    while let Some(child_node) = child_iter.next()? {
        if let Some((child_symbol, count)) =
            parse_die_recursive(dwarf, unit, child_node, id_counter)?
        {
            symbol_count += count;
            children.push(child_symbol);
        }
    }

    Ok(Some((
        DwarfSymbol {
            id,
            name,
            tag: dwarf_tag,
            address,
            size,
            file,
            line,
            column,
            type_name,
            children,
            attributes,
        },
        symbol_count,
    )))
}

fn get_string_attr<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
    attr_name: gimli::DwAt,
) -> Option<String> {
    let attr = entry.attr_value(attr_name).ok()??;
    let raw_str = dwarf.attr_string(unit, attr).ok()?;
    let cow_str = raw_str.to_string_lossy().ok()?;
    Some(cow_str.into_owned())
}

fn get_address_attr<R: gimli::Reader>(
    _unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
    attr_name: gimli::DwAt,
) -> Option<u64> {
    entry
        .attr_value(attr_name)
        .ok()
        .flatten()
        .and_then(|attr| match attr {
            gimli::AttributeValue::Addr(addr) => Some(addr),
            gimli::AttributeValue::Udata(data) => Some(data),
            _ => None,
        })
}

fn get_size<R: gimli::Reader>(
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
) -> Option<u64> {
    // Try byte_size first
    if let Some(size) = entry
        .attr_value(gimli::DW_AT_byte_size)
        .ok()
        .flatten()
        .and_then(|attr| match attr {
            gimli::AttributeValue::Udata(data) => Some(data),
            gimli::AttributeValue::Data1(data) => Some(data as u64),
            gimli::AttributeValue::Data2(data) => Some(data as u64),
            gimli::AttributeValue::Data4(data) => Some(data as u64),
            gimli::AttributeValue::Data8(data) => Some(data),
            _ => None,
        })
    {
        return Some(size);
    }

    // Try high_pc - low_pc
    let low_pc = get_address_attr(unit, entry, gimli::DW_AT_low_pc)?;
    let high_pc_attr = entry.attr_value(gimli::DW_AT_high_pc).ok().flatten()?;

    match high_pc_attr {
        gimli::AttributeValue::Addr(high_pc) => Some(high_pc - low_pc),
        gimli::AttributeValue::Udata(offset) => Some(offset),
        gimli::AttributeValue::Data1(offset) => Some(offset as u64),
        gimli::AttributeValue::Data2(offset) => Some(offset as u64),
        gimli::AttributeValue::Data4(offset) => Some(offset as u64),
        gimli::AttributeValue::Data8(offset) => Some(offset),
        _ => None,
    }
}

fn get_file_line_info<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
) -> (Option<String>, Option<u32>, Option<u32>) {
    let file = entry
        .attr_value(gimli::DW_AT_decl_file)
        .ok()
        .flatten()
        .and_then(|attr| match attr {
            gimli::AttributeValue::FileIndex(idx) => {
                if let Some(ref line_program) = unit.line_program {
                    let header = line_program.header();
                    if idx > 0 {
                        header.file(idx).and_then(|file_entry| {
                            let raw_str = dwarf.attr_string(unit, file_entry.path_name()).ok()?;
                            let cow_str = raw_str.to_string_lossy().ok()?;
                            Some(cow_str.into_owned())
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        });

    let line = entry
        .attr_value(gimli::DW_AT_decl_line)
        .ok()
        .flatten()
        .and_then(|attr| match attr {
            gimli::AttributeValue::Udata(line) => Some(line as u32),
            gimli::AttributeValue::Data1(line) => Some(line as u32),
            gimli::AttributeValue::Data2(line) => Some(line as u32),
            gimli::AttributeValue::Data4(line) => Some(line),
            _ => None,
        });

    let column = entry
        .attr_value(gimli::DW_AT_decl_column)
        .ok()
        .flatten()
        .and_then(|attr| match attr {
            gimli::AttributeValue::Udata(col) => Some(col as u32),
            gimli::AttributeValue::Data1(col) => Some(col as u32),
            gimli::AttributeValue::Data2(col) => Some(col as u32),
            _ => None,
        });

    (file, line, column)
}

fn get_type_name<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    entry: &gimli::DebuggingInformationEntry<R>,
) -> Option<String> {
    // Follow DW_AT_type reference to get type name
    let type_offset = entry
        .attr_value(gimli::DW_AT_type)
        .ok()
        .flatten()
        .and_then(|attr| match attr {
            gimli::AttributeValue::UnitRef(offset) => Some(offset),
            _ => None,
        })?;

    let mut tree = unit.entries_tree(Some(type_offset)).ok()?;
    let root = tree.root().ok()?;
    let type_entry = root.entry();

    // Get the type's name
    get_string_attr(dwarf, unit, type_entry, gimli::DW_AT_name)
}

fn demangle_name(name: &str) -> String {
    // Try Rust demangling
    for lang in [
        gimli::DW_LANG_Rust,
        gimli::DW_LANG_C_plus_plus,
        gimli::DW_LANG_C_plus_plus_03,
        gimli::DW_LANG_C_plus_plus_11,
        gimli::DW_LANG_C_plus_plus_14,
    ] {
        if let Some(demangled) = addr2line::demangle(name, lang) {
            return demangled;
        }
    }
    name.to_string()
}

/// Format a DWARF attribute value to a human-readable string
fn format_attr_value<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    attr: &gimli::Attribute<R>,
) -> Option<String> {
    use gimli::ReaderOffset;
    let value = attr.value();
    match value {
        gimli::AttributeValue::Addr(addr) => Some(format!("0x{:08x}", addr)),
        gimli::AttributeValue::Block(data) => {
            let bytes: Vec<String> = data
                .to_slice()
                .ok()?
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            if bytes.len() <= 16 {
                Some(format!("[{}]", bytes.join(" ")))
            } else {
                Some(format!(
                    "[{} ... ({} bytes)]",
                    bytes[..8].join(" "),
                    bytes.len()
                ))
            }
        }
        gimli::AttributeValue::Data1(val) => Some(val.to_string()),
        gimli::AttributeValue::Data2(val) => Some(val.to_string()),
        gimli::AttributeValue::Data4(val) => Some(val.to_string()),
        gimli::AttributeValue::Data8(val) => Some(val.to_string()),
        gimli::AttributeValue::Sdata(val) => Some(val.to_string()),
        gimli::AttributeValue::Udata(val) => Some(val.to_string()),
        gimli::AttributeValue::Exprloc(expr) => {
            // Format DWARF expression
            let bytes: Vec<String> = expr
                .0
                .to_slice()
                .ok()?
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            if bytes.is_empty() {
                Some("<empty expr>".to_string())
            } else if bytes.len() <= 16 {
                Some(format!("expr[{}]", bytes.join(" ")))
            } else {
                Some(format!(
                    "expr[{} ... ({} bytes)]",
                    bytes[..8].join(" "),
                    bytes.len()
                ))
            }
        }
        gimli::AttributeValue::Flag(val) => Some(if val { "true" } else { "false" }.to_string()),
        gimli::AttributeValue::SecOffset(offset) => {
            Some(format!("offset 0x{:x}", offset.into_u64()))
        }
        gimli::AttributeValue::UnitRef(offset) => {
            // Try to resolve the reference to get type name
            if let Ok(mut tree) = unit.entries_tree(Some(offset)) {
                if let Ok(root) = tree.root() {
                    let ref_entry = root.entry();
                    if let Some(name) = get_string_attr(dwarf, unit, ref_entry, gimli::DW_AT_name) {
                        return Some(demangle_name(&name));
                    }
                    // If no name, show the tag
                    return Some(format!(
                        "<{}> @ 0x{:x}",
                        ref_entry.tag().static_string().unwrap_or("?"),
                        offset.0.into_u64()
                    ));
                }
            }
            Some(format!("ref 0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugInfoRef(offset) => {
            Some(format!(".debug_info+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugInfoRefSup(offset) => {
            Some(format!(".debug_info.sup+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugLineRef(offset) => {
            Some(format!(".debug_line+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugLocListsBase(offset) => {
            Some(format!(".debug_loclists+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugLocListsIndex(index) => {
            Some(format!("loclist[{}]", index.0.into_u64()))
        }
        gimli::AttributeValue::DebugMacinfoRef(offset) => {
            Some(format!(".debug_macinfo+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugMacroRef(offset) => {
            Some(format!(".debug_macro+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugRngListsBase(offset) => {
            Some(format!(".debug_rnglists+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugRngListsIndex(index) => {
            Some(format!("rnglist[{}]", index.0.into_u64()))
        }
        gimli::AttributeValue::DebugStrRef(offset) => {
            // Resolve the string from .debug_str section
            if let Ok(s) = dwarf.debug_str.get_str(offset) {
                if let Ok(cow) = s.to_string_lossy() {
                    return Some(cow.into_owned());
                }
            }
            Some(format!(".debug_str+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugStrRefSup(offset) => {
            // Supplementary debug strings are in a separate file, just show the offset
            Some(format!(".debug_str.sup+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugStrOffsetsBase(offset) => {
            Some(format!(".debug_str_offsets+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugStrOffsetsIndex(index) => {
            // Try to resolve string via string offsets table
            if let Ok(offset) = dwarf.debug_str_offsets.get_str_offset(
                gimli::Format::Dwarf32,
                unit.str_offsets_base,
                index,
            ) {
                if let Ok(s) = dwarf.debug_str.get_str(offset) {
                    if let Ok(cow) = s.to_string_lossy() {
                        return Some(cow.into_owned());
                    }
                }
            }
            Some(format!("str[{}]", index.0.into_u64()))
        }
        gimli::AttributeValue::DebugTypesRef(sig) => Some(format!("type_sig 0x{:016x}", sig.0)),
        gimli::AttributeValue::DebugAddrBase(offset) => {
            Some(format!(".debug_addr+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::DebugAddrIndex(index) => {
            Some(format!("addr[{}]", index.0.into_u64()))
        }
        gimli::AttributeValue::DebugLineStrRef(offset) => {
            // Resolve the string from .debug_line_str section
            if let Ok(s) = dwarf.debug_line_str.get_str(offset) {
                if let Ok(cow) = s.to_string_lossy() {
                    return Some(cow.into_owned());
                }
            }
            Some(format!(".debug_line_str+0x{:x}", offset.0.into_u64()))
        }
        gimli::AttributeValue::String(s) => s.to_string_lossy().ok().map(|s| s.into_owned()),
        gimli::AttributeValue::Encoding(enc) => {
            Some(format!("{}", enc.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::DecimalSign(sign) => {
            Some(format!("{}", sign.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Endianity(end) => {
            Some(format!("{}", end.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Accessibility(acc) => {
            Some(format!("{}", acc.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Visibility(vis) => {
            Some(format!("{}", vis.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Virtuality(virt) => {
            Some(format!("{}", virt.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Language(lang) => {
            Some(format!("{}", lang.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::AddressClass(class) => Some(format!("addr_class({})", class.0)),
        gimli::AttributeValue::IdentifierCase(case) => {
            Some(format!("{}", case.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::CallingConvention(cc) => {
            Some(format!("{}", cc.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Inline(inl) => {
            Some(format!("{}", inl.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::Ordering(ord) => {
            Some(format!("{}", ord.static_string().unwrap_or("?")))
        }
        gimli::AttributeValue::FileIndex(idx) => {
            // Resolve file index to actual filename
            if let Some(ref line_program) = unit.line_program {
                let header = line_program.header();
                if idx > 0 {
                    if let Some(file_entry) = header.file(idx) {
                        if let Ok(raw_str) = dwarf.attr_string(unit, file_entry.path_name()) {
                            if let Ok(s) = raw_str.to_string_lossy() {
                                return Some(s.into_owned());
                            }
                        }
                    }
                }
            }
            Some(format!("file[{}]", idx))
        }
        gimli::AttributeValue::DwoId(id) => Some(format!("dwo_id 0x{:016x}", id.0)),
        _ => None, // Skip unknown attribute types
    }
}
