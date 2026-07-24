use core::ptr;

use crate::uefi::{
    ALLOCATE_ADDRESS, AllocatePages, LOADER_DATA, LOAD_ERROR, Status, SUCCESS,
};

const PT_LOAD: u32 = 1;
const ELF_HEADER_SIZE: usize = 64;

#[repr(C)]
#[derive(Clone, Copy)]
struct ProgramHeader {
    kind: u32,
    flags: u32,
    offset: u64,
    virtual_address: u64,
    physical_address: u64,
    file_size: u64,
    memory_size: u64,
    align: u64,
}

pub fn load(bytes: &[u8], allocate_pages: AllocatePages) -> Result<u64, Status> {
    if bytes.len() < ELF_HEADER_SIZE
        || &bytes[0..4] != b"\x7fELF"
        || bytes[4] != 2
        || bytes[5] != 1
        || read_u16(bytes, 18)? != 0x3e
    {
        return Err(LOAD_ERROR);
    }

    let entry = read_u64(bytes, 24)?;
    let ph_offset = read_u64(bytes, 32)? as usize;
    let ph_size = read_u16(bytes, 54)? as usize;
    let ph_count = read_u16(bytes, 56)? as usize;
    if ph_size < size_of::<ProgramHeader>() {
        return Err(LOAD_ERROR);
    }

    let mut lowest = u64::MAX;
    let mut highest = 0u64;
    for index in 0..ph_count {
        let header = program_header(bytes, ph_offset, ph_size, index)?;
        if header.kind == PT_LOAD && header.memory_size != 0 {
            if header.virtual_address != header.physical_address {
                return Err(LOAD_ERROR);
            }
            lowest = lowest.min(header.physical_address & !0xfff);
            highest = highest.max(
                header
                    .physical_address
                    .checked_add(header.memory_size)
                    .ok_or(LOAD_ERROR)?
                    .checked_add(0xfff)
                    .ok_or(LOAD_ERROR)?
                    & !0xfff,
            );
        }
    }
    if lowest == u64::MAX || highest <= lowest {
        return Err(LOAD_ERROR);
    }

    let mut load_address = lowest;
    let pages = ((highest - lowest) / 4096) as usize;
    let status = unsafe {
        allocate_pages(
            ALLOCATE_ADDRESS,
            LOADER_DATA,
            pages,
            &mut load_address,
        )
    };
    if status != SUCCESS || load_address != lowest {
        return Err(if status == SUCCESS { LOAD_ERROR } else { status });
    }

    unsafe {
        ptr::write_bytes(lowest as *mut u8, 0, (highest - lowest) as usize);
    }
    for index in 0..ph_count {
        let header = program_header(bytes, ph_offset, ph_size, index)?;
        if header.kind != PT_LOAD {
            continue;
        }
        let source_end = header
            .offset
            .checked_add(header.file_size)
            .ok_or(LOAD_ERROR)? as usize;
        let source = bytes
            .get(header.offset as usize..source_end)
            .ok_or(LOAD_ERROR)?;
        if header.file_size > header.memory_size {
            return Err(LOAD_ERROR);
        }
        unsafe {
            ptr::copy_nonoverlapping(
                source.as_ptr(),
                header.physical_address as *mut u8,
                source.len(),
            );
        }
    }
    Ok(entry)
}

fn program_header(
    bytes: &[u8],
    table: usize,
    stride: usize,
    index: usize,
) -> Result<ProgramHeader, Status> {
    let offset = table
        .checked_add(index.checked_mul(stride).ok_or(LOAD_ERROR)?)
        .ok_or(LOAD_ERROR)?;
    let end = offset
        .checked_add(size_of::<ProgramHeader>())
        .ok_or(LOAD_ERROR)?;
    let raw = bytes.get(offset..end).ok_or(LOAD_ERROR)?;
    Ok(unsafe { (raw.as_ptr() as *const ProgramHeader).read_unaligned() })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Status> {
    let raw: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or(LOAD_ERROR)?
        .try_into()
        .map_err(|_| LOAD_ERROR)?;
    Ok(u16::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Status> {
    let raw: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(LOAD_ERROR)?
        .try_into()
        .map_err(|_| LOAD_ERROR)?;
    Ok(u64::from_le_bytes(raw))
}