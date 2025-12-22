#[derive(Clone, Debug)]
pub struct MemoryRegion {
    pub name: String,
    pub start: u64,
    pub size: u64,
    pub kind: MemoryKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MemoryKind {
    Flash,
    Ram,
}

#[derive(Clone, Debug)]
pub struct MemorySegment {
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub flags: String,
    pub is_load: bool,
    pub conflicts: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ElfSymbol {
    pub name: String,
    pub address: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct DefmtInfo {
    pub present: bool,
    pub sections: Vec<(String, u64)>, // (section_name, size)
}

#[derive(Clone, Debug)]
pub struct RttBufferDesc {
    pub name: String,
    pub buffer_address: u64,
    pub size: u32,
}

#[derive(Clone, Debug)]
pub struct RttInfo {
    pub present: bool,
    pub symbol_name: Option<String>,
    pub address: Option<u64>,
    pub size: Option<u64>,
    pub max_up_buffers: Option<u32>,
    pub max_down_buffers: Option<u32>,
    pub up_buffers: Vec<RttBufferDesc>,
    pub down_buffers: Vec<RttBufferDesc>,
}

/// Represents a DWARF debug symbol with hierarchical structure
#[derive(Clone, Debug)]
pub struct DwarfSymbol {
    /// Unique identifier for this symbol
    pub id: usize,
    /// Symbol name (demangled if possible)
    pub name: String,
    /// The DWARF tag type (function, variable, struct, etc.)
    pub tag: DwarfTag,
    /// Memory address if applicable
    pub address: Option<u64>,
    /// Size in bytes if known
    pub size: Option<u64>,
    /// Source file path
    pub file: Option<String>,
    /// Line number in source file
    pub line: Option<u32>,
    /// Column number in source file
    pub column: Option<u32>,
    /// Type information (for variables, parameters, etc.)
    pub type_name: Option<String>,
    /// Child symbols (nested scopes, members, parameters, etc.)
    pub children: Vec<DwarfSymbol>,
    /// Additional attributes for display
    pub attributes: Vec<(String, String)>,
}

/// DWARF tag types we care about displaying
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DwarfTag {
    CompileUnit,
    Subprogram, // Function
    Variable,
    FormalParameter,
    LexicalBlock,
    InlinedSubroutine,
    StructureType,
    UnionType,
    EnumerationType,
    Member,
    Typedef,
    Namespace,
    Other(String),
}

impl DwarfTag {
    pub fn display_name(&self) -> &str {
        match self {
            DwarfTag::CompileUnit => "Compile Unit",
            DwarfTag::Subprogram => "Function",
            DwarfTag::Variable => "Variable",
            DwarfTag::FormalParameter => "Parameter",
            DwarfTag::LexicalBlock => "Block",
            DwarfTag::InlinedSubroutine => "Inlined",
            DwarfTag::StructureType => "Struct",
            DwarfTag::UnionType => "Union",
            DwarfTag::EnumerationType => "Enum",
            DwarfTag::Member => "Member",
            DwarfTag::Typedef => "Typedef",
            DwarfTag::Namespace => "Namespace",
            DwarfTag::Other(s) => s.as_str(),
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            DwarfTag::CompileUnit => "📦",
            DwarfTag::Subprogram => "ƒ",
            DwarfTag::Variable => "𝑥",
            DwarfTag::FormalParameter => "→",
            DwarfTag::LexicalBlock => "{ }",
            DwarfTag::InlinedSubroutine => "⤵",
            DwarfTag::StructureType => "◈",
            DwarfTag::UnionType => "◇",
            DwarfTag::EnumerationType => "▤",
            DwarfTag::Member => "•",
            DwarfTag::Typedef => "≡",
            DwarfTag::Namespace => ":::",
            DwarfTag::Other(_) => "?",
        }
    }
}

/// Information about parsed DWARF debug info
#[derive(Clone, Debug)]
pub struct DwarfInfo {
    pub present: bool,
    pub compile_units: Vec<DwarfSymbol>,
    pub total_symbols: usize,
}

impl Default for DwarfInfo {
    fn default() -> Self {
        Self {
            present: false,
            compile_units: Vec::new(),
            total_symbols: 0,
        }
    }
}

impl MemoryRegion {
    pub fn contains(&self, address: u64, size: u64) -> bool {
        let end = address + size;
        let region_end = self.start + self.size;
        address >= self.start && end <= region_end
    }

    pub fn overlaps(&self, address: u64, size: u64) -> bool {
        let end = address + size;
        let region_end = self.start + self.size;
        !(end <= self.start || address >= region_end)
    }
}
