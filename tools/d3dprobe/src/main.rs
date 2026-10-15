//! d3dprobe: creates a Direct3D 11 device and prints what it got.
//!
//! Vinter runs this inside a bottle (with Wine's DLL-load trace on) to check
//! that the chosen graphics backend actually works. Output, one per line:
//!
//! ```text
//! d3d11_create=0x00000000
//! feature_level=0xb000
//! adapter=Apple M4 Pro
//! vendor=0x106b
//! ```
//!
//! Exit code 0 if a device was created, 1 otherwise.
//! `no_std` so it builds on macOS/Linux with only `rustup target add
//! x86_64-pc-windows-msvc`: see README.md.

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::fmt::{self, Write};
use core::mem::MaybeUninit;
use core::ptr::null_mut;

type Hresult = i32;
type Handle = *mut c_void;

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// IID_IDXGIFactory1 {770aae78-f26f-4dba-a829-253c83d1b387}
const IID_IDXGI_FACTORY1: Guid = Guid {
    data1: 0x770aae78,
    data2: 0xf26f,
    data3: 0x4dba,
    data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
};

#[repr(C)]
struct AdapterDesc1 {
    description: [u16; 128],
    vendor_id: u32,
    device_id: u32,
    sub_sys_id: u32,
    revision: u32,
    dedicated_video_memory: usize,
    dedicated_system_memory: usize,
    shared_system_memory: usize,
    luid_low: u32,
    luid_high: i32,
    flags: u32,
}

const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const D3D_DRIVER_TYPE_HARDWARE: u32 = 1;
const D3D11_SDK_VERSION: u32 = 7;

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetStdHandle(which: u32) -> Handle;
    fn WriteFile(file: Handle, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
    fn ExitProcess(code: u32) -> !;
}

#[link(name = "d3d11", kind = "raw-dylib")]
unsafe extern "system" {
    fn D3D11CreateDevice(
        adapter: *mut c_void,
        driver_type: u32,
        software: Handle,
        flags: u32,
        feature_levels: *const u32,
        num_levels: u32,
        sdk_version: u32,
        device: *mut *mut c_void,
        feature_level: *mut u32,
        context: *mut *mut c_void,
    ) -> Hresult;
}

#[link(name = "dxgi", kind = "raw-dylib")]
unsafe extern "system" {
    fn CreateDXGIFactory1(riid: *const Guid, factory: *mut *mut c_void) -> Hresult;
}

/// Writes formatted text straight to the console handle.
struct Stdout(Handle);

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut written = 0;
        // SAFETY: valid buffer and length; the handle may be null, in which case WriteFile fails.
        unsafe { WriteFile(self.0, s.as_ptr(), s.len() as u32, &mut written, null_mut()) };
        Ok(())
    }
}

/// Calls COM method `index` of `object`'s vtable.
///
/// SAFETY: `object` must be a live COM object whose method at `index` has type `F`.
unsafe fn method<F: Copy>(object: *mut c_void, index: usize) -> F {
    unsafe {
        let vtable = *(object as *const *const usize);
        core::mem::transmute_copy(&*vtable.add(index))
    }
}

/// Prints the first adapter's name and vendor through DXGI.
fn print_adapter(out: &mut Stdout) {
    let mut factory = null_mut();
    // SAFETY: valid IID and out-pointer.
    let hr = unsafe { CreateDXGIFactory1(&IID_IDXGI_FACTORY1, &mut factory) };
    if hr < 0 || factory.is_null() {
        let _ = writeln!(out, "dxgi_factory={:#010x}", hr as u32);
        return;
    }
    // IDXGIFactory1::EnumAdapters1 is vtable slot 12, IDXGIAdapter1::GetDesc1 slot 10.
    type EnumAdapters1 = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult;
    type GetDesc1 = unsafe extern "system" fn(*mut c_void, *mut AdapterDesc1) -> Hresult;
    let mut adapter = null_mut();
    // SAFETY: factory is a live IDXGIFactory1.
    let hr = unsafe { method::<EnumAdapters1>(factory, 12)(factory, 0, &mut adapter) };
    if hr < 0 || adapter.is_null() {
        let _ = writeln!(out, "dxgi_adapter={:#010x}", hr as u32);
        return;
    }
    let mut desc = MaybeUninit::<AdapterDesc1>::zeroed();
    // SAFETY: adapter is a live IDXGIAdapter1; desc is writable.
    let hr = unsafe { method::<GetDesc1>(adapter, 10)(adapter, desc.as_mut_ptr()) };
    if hr < 0 {
        let _ = writeln!(out, "dxgi_desc={:#010x}", hr as u32);
        return;
    }
    // SAFETY: GetDesc1 succeeded, so desc is initialised.
    let desc = unsafe { desc.assume_init() };
    let _ = out.write_str("adapter=");
    for c in char::decode_utf16(desc.description.iter().copied().take_while(|&c| c != 0)) {
        let _ = out.write_char(c.unwrap_or('?'));
    }
    let _ = writeln!(out, "\nvendor={:#06x}", desc.vendor_id);
}

#[unsafe(no_mangle)]
pub extern "system" fn mainCRTStartup() -> ! {
    // SAFETY: plain Win32 call.
    let mut out = Stdout(unsafe { GetStdHandle(STD_OUTPUT_HANDLE) });
    let mut device = null_mut();
    let mut level = 0u32;
    // SAFETY: all pointers are valid or null where the API allows it.
    let hr = unsafe {
        D3D11CreateDevice(
            null_mut(),
            D3D_DRIVER_TYPE_HARDWARE,
            null_mut(),
            0,
            core::ptr::null(),
            0,
            D3D11_SDK_VERSION,
            &mut device,
            &mut level,
            null_mut(),
        )
    };
    let _ = writeln!(out, "d3d11_create={:#010x}", hr as u32);
    if hr >= 0 {
        let _ = writeln!(out, "feature_level={level:#06x}");
    }
    print_adapter(&mut out);
    // SAFETY: plain Win32 call.
    unsafe { ExitProcess(if hr >= 0 && !device.is_null() { 0 } else { 1 }) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // SAFETY: plain Win32 call.
    unsafe { ExitProcess(3) }
}

// With no C runtime, the linker needs `memset` and `memcpy` from somewhere.
// Volatile accesses keep LLVM from turning these loops back into calls to themselves.

/// # Safety
/// `dest` must be valid for `n` byte writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let d = dest as *mut u8;
    for i in 0..n {
        unsafe { core::ptr::write_volatile(d.add(i), c as u8) };
    }
    dest
}

/// # Safety
/// `src` must be valid for `n` byte reads and `dest` for `n` byte writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (dest as *mut u8, src as *const u8);
    for i in 0..n {
        unsafe { core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i))) };
    }
    dest
}
