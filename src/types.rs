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
