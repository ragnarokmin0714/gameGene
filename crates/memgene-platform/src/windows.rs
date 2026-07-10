//! Windows backend: the real target platform.
//!
//! Uses the Win32 APIs `OpenProcess` / `ReadProcessMemory` /
//! `WriteProcessMemory` / `VirtualQueryEx` and the ToolHelp snapshot APIs for
//! process and module enumeration.
//!
//! NOTE: this module only compiles on Windows and is not exercised by the
//! Linux CI here — verify it on the target machine.

use super::ProcessInfo;
use memgene_core::{MemError, MemoryRegion, MemorySource};
use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

/// Decode a UTF-16 fixed array (NUL-terminated) into a String.
fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                out.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    name: wide_to_string(&entry.szExeFile),
                });
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn attach(pid: u32) -> Result<Box<dyn MemorySource>, MemError> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
            false,
            pid,
        )
    }
    .map_err(|e| MemError::Read {
        addr: 0,
        reason: format!("OpenProcess failed: {e}"),
    })?;
    Ok(Box::new(WindowsProcess { pid, handle }))
}

struct WindowsProcess {
    pid: u32,
    handle: HANDLE,
}

// A process handle is just a kernel object reference; Win32 memory APIs are
// safe to call from any thread with it.
unsafe impl Send for WindowsProcess {}
unsafe impl Sync for WindowsProcess {}

impl Drop for WindowsProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl MemorySource for WindowsProcess {
    fn regions(&self) -> Vec<MemoryRegion> {
        let mut regions = Vec::new();
        let mut addr: usize = 0;
        loop {
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            let written = unsafe {
                VirtualQueryEx(
                    self.handle,
                    Some(addr as *const c_void),
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if written == 0 {
                break; // reached the end of the address space
            }
            let region_size = mbi.RegionSize;
            let protect = mbi.Protect;
            let committed = mbi.State == MEM_COMMIT;
            let guarded = (protect & PAGE_GUARD).0 != 0 || (protect & PAGE_NOACCESS).0 != 0;
            if committed && !guarded {
                regions.push(MemoryRegion {
                    base: mbi.BaseAddress as u64,
                    size: region_size as u64,
                    writable: is_writable(protect.0),
                });
            }
            // Advance past this region; guard against wraparound at the top.
            let next = (mbi.BaseAddress as usize).checked_add(region_size);
            match next {
                Some(n) if n > addr => addr = n,
                _ => break,
            }
        }
        regions
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<usize, MemError> {
        let mut read: usize = 0;
        let res = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                Some(&mut read),
            )
        };
        match res {
            Ok(()) => Ok(read),
            // A partial read still returns an error but sets `read`; treat any
            // bytes we got as a short read so scanning skips the bad tail.
            Err(_) if read > 0 => Ok(read),
            Err(e) => Err(MemError::Read {
                addr,
                reason: e.to_string(),
            }),
        }
    }

    fn write(&self, addr: u64, data: &[u8]) -> Result<(), MemError> {
        let mut written: usize = 0;
        unsafe {
            WriteProcessMemory(
                self.handle,
                addr as *const c_void,
                data.as_ptr() as *const c_void,
                data.len(),
                Some(&mut written),
            )
        }
        .map_err(|e| MemError::Write {
            addr,
            reason: e.to_string(),
        })?;
        if written != data.len() {
            return Err(MemError::Write {
                addr,
                reason: format!("only {written} of {} bytes written", data.len()),
            });
        }
        Ok(())
    }

    fn module_base(&self, name: &str) -> Option<u64> {
        unsafe {
            let snapshot =
                CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.pid).ok()?;
            let mut entry = MODULEENTRY32W {
                dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
                ..Default::default()
            };
            let mut found = None;
            if Module32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    if wide_to_string(&entry.szModule).eq_ignore_ascii_case(name) {
                        found = Some(entry.modBaseAddr as u64);
                        break;
                    }
                    if Module32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            found
        }
    }
}

/// Whether a page-protection constant permits writing.
fn is_writable(protect: u32) -> bool {
    let w =
        PAGE_READWRITE.0 | PAGE_WRITECOPY.0 | PAGE_EXECUTE_READWRITE.0 | PAGE_EXECUTE_WRITECOPY.0;
    protect & w != 0
}
