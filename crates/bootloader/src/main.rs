#![no_main]
#![no_std]

mod elf;
mod uefi;

use andora_boot_protocol::{
    BOOT_INFO_MAGIC, BOOT_INFO_VERSION, BootInfo, FramebufferInfo, MemoryMapInfo,
};
use core::{arch::asm, ffi::c_void, panic::PanicInfo};
use uefi::{
    BUFFER_TOO_SMALL, BootServices, FileProtocol, GOP_GUID, GraphicsOutputProtocol, Handle,
    INVALID_PARAMETER, LOADED_IMAGE_GUID, LoadedImageProtocol, MemoryDescriptor,
    SIMPLE_FILE_SYSTEM_GUID, SUCCESS, SimpleFileSystemProtocol, Status, SystemTable,
};

const MAP_CAPACITY: usize = 256 * 1024;
const FILE_CAPACITY: usize = 4 * 1024 * 1024;
const STACK_PAGES: usize = 16;

#[repr(C, align(16))]
struct Aligned<const N: usize>([u8; N]);

static mut MAP_BUFFER: Aligned<MAP_CAPACITY> = Aligned([0; MAP_CAPACITY]);
static mut FILE_BUFFER: Aligned<FILE_CAPACITY> = Aligned([0; FILE_CAPACITY]);
static mut BOOT_INFO: BootInfo = BootInfo {
    magic: BOOT_INFO_MAGIC,
    version: BOOT_INFO_VERSION,
    size: size_of::<BootInfo>() as u32,
    framebuffer: FramebufferInfo {
        base: 0,
        byte_len: 0,
        width: 0,
        height: 0,
        stride: 0,
        pixel_format: 0,
    },
    memory_map: MemoryMapInfo {
        buffer: core::ptr::null(),
        byte_len: 0,
        descriptor_size: 0,
        descriptor_version: 0,
        _reserved: 0,
    },
};

#[unsafe(export_name = "efi_main")]
pub extern "efiapi" fn efi_main(image: Handle, table: *mut SystemTable) -> Status {
    match unsafe { boot(image, table) } {
        Ok(()) => SUCCESS,
        Err(status) => status,
    }
}

unsafe fn boot(image: Handle, table: *mut SystemTable) -> Result<(), Status> {
    let table = unsafe { table.as_mut() }.ok_or(INVALID_PARAMETER)?;
    let boot = unsafe { table.boot_services.as_mut() }.ok_or(INVALID_PARAMETER)?;
    text(table, "Andora UEFI loader\r\n");
    unsafe { (boot.set_watchdog_timer)(0, 0, 0, core::ptr::null()) };

    let kernel = unsafe { read_kernel(image, boot)? };
    text(table, "kernel.elf read\r\n");
    let entry = elf::load(kernel, boot.allocate_pages)?;
    text(table, "ELF segments loaded\r\n");

    let framebuffer = unsafe { framebuffer(boot)? };
    let mut stack_base = 0x0000_0000_ffff_ffffu64;
    let status = unsafe {
        (boot.allocate_pages)(
            1, // AllocateMaxAddress: keep the bootstrap stack identity-mappable.
            uefi::LOADER_DATA,
            STACK_PAGES,
            &mut stack_base,
        )
    };
    if status != SUCCESS {
        return Err(status);
    }
    let stack_top = stack_base + (STACK_PAGES * 4096) as u64;

    text(table, "exiting UEFI boot services\r\n");
    let memory_map = unsafe { exit_boot_services(image, boot)? };
    unsafe {
        BOOT_INFO.framebuffer = framebuffer;
        BOOT_INFO.memory_map = memory_map;
        enter_kernel(entry, stack_top, &raw const BOOT_INFO)
    }
}

unsafe fn read_kernel(
    image: Handle,
    boot: &mut BootServices,
) -> Result<&'static [u8], Status> {
    let mut loaded: *mut c_void = core::ptr::null_mut();
    check(unsafe {
        (boot.handle_protocol)(image, &raw const LOADED_IMAGE_GUID, &mut loaded)
    })?;
    let loaded = unsafe { loaded.cast::<LoadedImageProtocol>().as_ref() }
        .ok_or(INVALID_PARAMETER)?;

    let mut filesystem: *mut c_void = core::ptr::null_mut();
    check(unsafe {
        (boot.handle_protocol)(
            loaded.device_handle,
            &raw const SIMPLE_FILE_SYSTEM_GUID,
            &mut filesystem,
        )
    })?;
    let filesystem = unsafe { filesystem.cast::<SimpleFileSystemProtocol>().as_mut() }
        .ok_or(INVALID_PARAMETER)?;

    let mut root: *mut FileProtocol = core::ptr::null_mut();
    check(unsafe { (filesystem.open_volume)(filesystem, &mut root) })?;
    let root = unsafe { root.as_mut() }.ok_or(INVALID_PARAMETER)?;

    const PATH: [u16; 12] = [
        b'\\' as u16, b'k' as u16, b'e' as u16, b'r' as u16, b'n' as u16, b'e' as u16,
        b'l' as u16, b'.' as u16, b'e' as u16, b'l' as u16, b'f' as u16, 0,
    ];
    let mut file: *mut FileProtocol = core::ptr::null_mut();
    check(unsafe { (root.open)(root, &mut file, PATH.as_ptr(), 1, 0) })?;
    let file = unsafe { file.as_mut() }.ok_or(INVALID_PARAMETER)?;

    let mut bytes = FILE_CAPACITY;
    check(unsafe {
        (file.read)(
            file,
            &mut bytes,
            (&raw mut FILE_BUFFER.0).cast::<c_void>(),
        )
    })?;
    unsafe {
        (file.close)(file);
        (root.close)(root);
    }
    Ok(unsafe { core::slice::from_raw_parts((&raw const FILE_BUFFER.0).cast(), bytes) })
}

unsafe fn framebuffer(boot: &mut BootServices) -> Result<FramebufferInfo, Status> {
    let mut interface: *mut c_void = core::ptr::null_mut();
    check(unsafe {
        (boot.locate_protocol)(&raw const GOP_GUID, core::ptr::null(), &mut interface)
    })?;
    let gop = unsafe { interface.cast::<GraphicsOutputProtocol>().as_ref() }
        .ok_or(INVALID_PARAMETER)?;
    let mode = unsafe { gop.mode.as_ref() }.ok_or(INVALID_PARAMETER)?;
    let info = unsafe { mode.info.as_ref() }.ok_or(INVALID_PARAMETER)?;
    if info.pixel_format > 1 {
        return Err(INVALID_PARAMETER);
    }
    Ok(FramebufferInfo {
        base: mode.framebuffer_base,
        byte_len: mode.framebuffer_size,
        width: info.horizontal_resolution,
        height: info.vertical_resolution,
        stride: info.pixels_per_scan_line,
        pixel_format: info.pixel_format,
    })
}

unsafe fn exit_boot_services(
    image: Handle,
    boot: &mut BootServices,
) -> Result<MemoryMapInfo, Status> {
    let mut attempt = 0;
    loop {
        let (map, key) = unsafe { memory_map(boot)? };
        let status = unsafe { (boot.exit_boot_services)(image, key) };
        if status == SUCCESS {
            return Ok(map);
        }
        if status != INVALID_PARAMETER || attempt == 1 {
            return Err(status);
        }
        attempt += 1;
    }
}

unsafe fn memory_map(boot: &mut BootServices) -> Result<(MemoryMapInfo, usize), Status> {
    let mut bytes = MAP_CAPACITY;
    let mut key = 0;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;
    let status = unsafe {
        (boot.get_memory_map)(
            &mut bytes,
            (&raw mut MAP_BUFFER.0).cast::<MemoryDescriptor>(),
            &mut key,
            &mut descriptor_size,
            &mut descriptor_version,
        )
    };
    if status == BUFFER_TOO_SMALL || status != SUCCESS {
        return Err(status);
    }
    Ok((
        MemoryMapInfo {
            buffer: unsafe { (&raw const MAP_BUFFER.0).cast() },
            byte_len: bytes,
            descriptor_size,
            descriptor_version,
            _reserved: 0,
        },
        key,
    ))
}

unsafe fn enter_kernel(entry: u64, stack_top: u64, boot_info: *const BootInfo) -> ! {
    unsafe {
        asm!(
            "mov rsp, {stack}",
            "and rsp, -16",
            "xor rbp, rbp",
            "jmp {entry}",
            stack = in(reg) stack_top,
            entry = in(reg) entry,
            in("rdi") boot_info,
            options(noreturn)
        )
    }
}

fn check(status: Status) -> Result<(), Status> {
    if status == SUCCESS { Ok(()) } else { Err(status) }
}

fn text(table: &mut SystemTable, message: &str) {
    let Some(output) = (unsafe { table.con_out.as_mut() }) else {
        return;
    };
    let mut utf16 = [0u16; 128];
    for (index, unit) in message.encode_utf16().take(127).enumerate() {
        utf16[index] = unit;
    }
    unsafe { (output.output_string)(output, utf16.as_ptr()) };
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { asm!("cli; hlt", options(nomem, nostack)) }
    }
}
