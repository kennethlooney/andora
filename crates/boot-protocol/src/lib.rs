#![no_std]

pub const BOOT_INFO_MAGIC: u64 = 0x36d7_6289_a6c3_d9f2;
pub const BOOT_INFO_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FramebufferInfo 
{
    pub base: u64,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// UEFI GOP values 0 (RGB) or 1 (BGR)
    pub pixel_format: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryMapInfo
{
    pub buffer: *const u8,
    pub byte_len: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
    pub _reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootInfo
{
    pub magic: u64,
    pub version: u32,
    pub size: u32,
    pub framebuffer: FramebufferInfo,
    pub memory_map: MemoryMapInfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryDescriptor {
    pub kind: u32,
    pub _padding: u32,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

impl MemoryMapInfo {
    pub fn descriptors(self) -> DescriptorIter {
        DescriptorIter {
            cursor: self.buffer as usize,
            end: self.buffer as usize + self.byte_len,
            stride: self.descriptor_size,
        }
    }
}

pub struct DescriptorIter {
    cursor: usize,
    end: usize,
    stride: usize,
}

impl Iterator for DescriptorIter {
    type Item = MemoryDescriptor;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stride < size_of::<MemoryDescriptor>()
            || self.cursor.saturating_add(self.stride) > self.end
        {
            return None;
        }
        let descriptor =
            unsafe { (self.cursor as *const MemoryDescriptor).read_unaligned() };
        self.cursor += self.stride;
        Some(descriptor)
    }
}
