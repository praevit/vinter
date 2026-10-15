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
//! swapchain=0x00000000
//! present=0x00000000
//! ```
//!
//! The swap chain and present lines come from a small window it opens for an
//! instant: they show that the backend can draw to the screen, not only
//! create a device. Exit code 0 if a device was created, 1 otherwise.
//! `no_std` so it builds on macOS/Linux with only `rustup target add
//! x86_64-pc-windows-msvc`. Rebuild and update the embedded copy with
//! `tools/d3dprobe/build.sh`.

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

/// IID_IDXGIFactory {7b7166ec-21c7-44ae-b21a-c9ae321ae369}
const IID_IDXGI_FACTORY: Guid = Guid {
    data1: 0x7b7166ec,
    data2: 0x21c7,
    data3: 0x44ae,
    data4: [0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69],
};

#[repr(C)]
struct SwapChainDesc {
    width: u32,
    height: u32,
    refresh_numerator: u32,
    refresh_denominator: u32,
    format: u32,
    scanline_ordering: u32,
    scaling: u32,
    sample_count: u32,
    sample_quality: u32,
    buffer_usage: u32,
    buffer_count: u32,
    output_window: Handle,
    windowed: i32,
    swap_effect: u32,
    flags: u32,
}

#[repr(C)]
struct WndClassW {
    style: u32,
    wnd_proc: *const c_void,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
}

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
const DXGI_FORMAT_R8G8B8A8_UNORM: u32 = 28;
const DXGI_USAGE_RENDER_TARGET_OUTPUT: u32 = 0x20;
const WS_POPUP_VISIBLE: u32 = 0x8000_0000 | 0x1000_0000;
/// "d3dprobe" as UTF-16, NUL-terminated.
const CLASS_NAME: [u16; 9] = [100, 51, 100, 112, 114, 111, 98, 101, 0];

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetStdHandle(which: u32) -> Handle;
    fn WriteFile(file: Handle, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
    fn ExitProcess(code: u32) -> !;
    fn GetModuleHandleW(name: *const u16) -> Handle;
}

#[link(name = "user32", kind = "raw-dylib")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    fn DefWindowProcW(window: Handle, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn DestroyWindow(window: Handle) -> i32;
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

/// Opens a small window, makes a swap chain for it and presents one frame.
fn print_present(out: &mut Stdout, device: *mut c_void) {
    let mut factory = null_mut();
    // SAFETY: valid IID and out-pointer.
    let hr = unsafe { CreateDXGIFactory1(&IID_IDXGI_FACTORY, &mut factory) };
    if hr < 0 || factory.is_null() {
        return;
    }
    // SAFETY: plain Win32 calls with valid, NUL-terminated strings.
    let window = unsafe {
        let instance = GetModuleHandleW(core::ptr::null());
        let class = WndClassW {
            style: 0,
            wnd_proc: DefWindowProcW as *const c_void,
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: null_mut(),
            cursor: null_mut(),
            background: null_mut(),
            menu_name: core::ptr::null(),
            class_name: CLASS_NAME.as_ptr(),
        };
        RegisterClassW(&class);
        CreateWindowExW(
            0,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.as_ptr(),
            WS_POPUP_VISIBLE,
            0,
            0,
            64,
            64,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        )
    };
    if window.is_null() {
        let _ = writeln!(out, "window=failed");
        return;
    }
    let desc = SwapChainDesc {
        width: 64,
        height: 64,
        refresh_numerator: 0,
        refresh_denominator: 1,
        format: DXGI_FORMAT_R8G8B8A8_UNORM,
        scanline_ordering: 0,
        scaling: 0,
        sample_count: 1,
        sample_quality: 0,
        buffer_usage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        buffer_count: 1,
        output_window: window,
        windowed: 1,
        swap_effect: 0,
        flags: 0,
    };
    // IDXGIFactory::CreateSwapChain is vtable slot 10, IDXGISwapChain::Present slot 8.
    type CreateSwapChain =
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const SwapChainDesc, *mut *mut c_void) -> Hresult;
    type Present = unsafe extern "system" fn(*mut c_void, u32, u32) -> Hresult;
    let mut swapchain = null_mut();
    // SAFETY: factory is a live IDXGIFactory, device a live ID3D11Device.
    let hr = unsafe { method::<CreateSwapChain>(factory, 10)(factory, device, &desc, &mut swapchain) };
    let _ = writeln!(out, "swapchain={:#010x}", hr as u32);
    if hr >= 0 && !swapchain.is_null() {
        // SAFETY: swapchain is a live IDXGISwapChain.
        let hr = unsafe { method::<Present>(swapchain, 8)(swapchain, 0, 0) };
        let _ = writeln!(out, "present={:#010x}", hr as u32);
    }
    // SAFETY: window was created above.
    unsafe { DestroyWindow(window) };
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
    if hr >= 0 && !device.is_null() {
        print_present(&mut out, device);
    }
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
