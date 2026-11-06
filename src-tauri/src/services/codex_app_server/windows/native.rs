//! Native Windows discovery and termination never start a shell or a new app.
use super::{
    descendants, desktop_server, finish_descendants, normalize, selected_home, unix_micros, Process,
};
use crate::services::codex_app_server::Instance;
use std::{ffi::c_void, mem, path::Path, ptr};
use windows_sys::{
    Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessBasicInformation, ProcessCommandLineInformation,
        ProcessWow64Information,
    },
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, LocalFree, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
            FILETIME, HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING,
        },
        Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
        System::{
            Diagnostics::{
                Debug::ReadProcessMemory,
                ToolHelp::{
                    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                    TH32CS_SNAPPROCESS,
                },
            },
            Threading::{
                GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess,
                OpenProcessToken, QueryFullProcessImageNameW, TerminateProcess,
                PROCESS_BASIC_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
                PROCESS_VM_READ,
            },
        },
        UI::Shell::CommandLineToArgvW,
    },
};

struct Owned(HANDLE);
impl Owned {
    fn new(handle: HANDLE) -> Result<Self, ()> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn open(pid: u32, access: u32) -> Result<Owned, ()> {
    Owned::new(unsafe { OpenProcess(access, 0, pid) })
}

fn token_user(process: HANDLE) -> Result<Vec<usize>, ()> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(());
    }
    let token = Owned::new(token)?;
    let mut size = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size);
    }
    if size < mem::size_of::<TOKEN_USER>() as u32 || size > 64 * 1024 {
        return Err(());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(());
    }
    Ok(buffer)
}

fn same_user(process: HANDLE) -> Result<bool, ()> {
    let caller = token_user(unsafe { GetCurrentProcess() })?;
    let target = token_user(process)?;
    // GetTokenInformation supplies TOKEN_USER followed by its SID in each buffer.
    let caller = unsafe { &*caller.as_ptr().cast::<TOKEN_USER>() };
    let target = unsafe { &*target.as_ptr().cast::<TOKEN_USER>() };
    Ok(unsafe { EqualSid(caller.User.Sid, target.User.Sid) } != 0)
}

fn started(process: HANDLE) -> Result<u64, ()> {
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process, &mut code) } == 0 || code != 259 {
        return Err(());
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(());
    }
    unix_micros((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        .ok_or(())
}

fn image(process: HANDLE) -> Result<String, ()> {
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) } == 0 {
        return Err(());
    }
    String::from_utf16(buffer.get(..size as usize).ok_or(())?).map_err(|_| ())
}

fn query<T: Default>(process: HANDLE, class: i32) -> Result<T, ()> {
    let mut value = T::default();
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            class,
            (&mut value as *mut T).cast(),
            mem::size_of::<T>() as u32,
            ptr::null_mut(),
        )
    };
    if status < 0 {
        Err(())
    } else {
        Ok(value)
    }
}

fn command_line(process: HANDLE) -> Result<Vec<String>, ()> {
    let mut size = 0;
    unsafe {
        NtQueryInformationProcess(
            process,
            ProcessCommandLineInformation,
            ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size < mem::size_of::<UNICODE_STRING>() as u32 || size > 256 * 1024 {
        return Err(());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(mem::size_of::<usize>())];
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            ProcessCommandLineInformation,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    };
    if status < 0 {
        return Err(());
    }
    let value = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
    let begin = buffer.as_ptr() as usize;
    let text = value.Buffer as usize;
    let end = text.checked_add(value.Length as usize).ok_or(())?;
    if value.Length % 2 != 0
        || text % mem::align_of::<u16>() != 0
        || text < begin
        || end > begin + buffer.len() * mem::size_of::<usize>()
    {
        return Err(());
    }
    let mut command =
        unsafe { std::slice::from_raw_parts(value.Buffer, usize::from(value.Length) / 2) }.to_vec();
    command.push(0);
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
    if argv.is_null() {
        return Err(());
    }
    let result = (|| {
        if !(1..=4096).contains(&count) {
            return Err(());
        }
        let mut args = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let value = unsafe { *argv.add(index) };
            let mut len = 0;
            while len < 32768 && unsafe { *value.add(len) } != 0 {
                len += 1;
            }
            if len == 32768 {
                return Err(());
            }
            args.push(
                String::from_utf16(unsafe { std::slice::from_raw_parts(value, len) })
                    .map_err(|_| ())?,
            );
        }
        Ok(args)
    })();
    unsafe {
        LocalFree(argv.cast());
    }
    result
}

fn remote(process: HANDLE, address: usize, length: usize) -> Result<Vec<u8>, ()> {
    if address == 0 {
        return Err(());
    }
    let mut bytes = vec![0u8; length];
    let mut read = 0;
    if unsafe {
        ReadProcessMemory(
            process,
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            length,
            &mut read,
        )
    } == 0
        || read != length
    {
        return Err(());
    }
    Ok(bytes)
}

fn pointer(process: HANDLE, address: usize, width: usize) -> Result<usize, ()> {
    let bytes = remote(process, address, width)?;
    match width {
        4 => Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| ())?) as usize),
        8 => usize::try_from(u64::from_le_bytes(bytes.try_into().map_err(|_| ())?)).map_err(|_| ()),
        _ => Err(()),
    }
}

fn environment(process: HANDLE, basic: &PROCESS_BASIC_INFORMATION) -> Result<Vec<String>, ()> {
    let wow: usize = query(process, ProcessWow64Information)?;
    let (peb, parameters_offset, environment_offset, width) =
        if wow != 0 || mem::size_of::<usize>() == 4 {
            (
                if wow != 0 {
                    wow
                } else {
                    basic.PebBaseAddress as usize
                },
                0x10,
                0x48,
                4,
            )
        } else {
            (basic.PebBaseAddress as usize, 0x20, 0x80, 8)
        };
    // The PEB and RTL_USER_PROCESS_PARAMETERS offsets differ for native and WOW64 pointers.
    let parameters = pointer(
        process,
        peb.checked_add(parameters_offset).ok_or(())?,
        width,
    )?;
    let address = pointer(
        process,
        parameters.checked_add(environment_offset).ok_or(())?,
        width,
    )?;
    let mut words = Vec::new();
    let mut offset = 0;
    while offset < 1024 * 1024 {
        let current = address.checked_add(offset).ok_or(())?;
        let length = 4096 - current % 4096;
        let bytes = remote(process, current, length)?;
        for pair in bytes.chunks_exact(2) {
            let word = u16::from_le_bytes([pair[0], pair[1]]);
            if word == 0 && words.last() == Some(&0) {
                return words
                    .split(|word| *word == 0)
                    .filter(|value| !value.is_empty())
                    .map(|value| String::from_utf16(value).map_err(|_| ()))
                    .collect();
            }
            words.push(word);
        }
        offset += length;
    }
    Err(())
}

struct Entry {
    pid: u32,
    parent: u32,
    name: String,
}

fn entries() -> Result<Vec<Entry>, ()> {
    let snapshot = Owned::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })?;
    let mut entry = PROCESSENTRY32W {
        dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut result = Vec::new();
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while next != 0 {
        let len = entry
            .szExeFile
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(entry.szExeFile.len());
        result.push(Entry {
            pid: entry.th32ProcessID,
            parent: entry.th32ParentProcessID,
            name: String::from_utf16_lossy(&entry.szExeFile[..len]),
        });
        next = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(());
    }
    Ok(result)
}

fn identity(process: HANDLE, pid: u32, home: &Path) -> Result<Instance, ()> {
    if !same_user(process)? {
        return Err(());
    }
    let since = started(process)?;
    let basic: PROCESS_BASIC_INFORMATION = query(process, ProcessBasicInformation)?;
    let parent = open(
        u32::try_from(basic.InheritedFromUniqueProcessId).map_err(|_| ())?,
        PROCESS_QUERY_LIMITED_INFORMATION,
    )?;
    if !same_user(parent.0)? || started(parent.0)? > since {
        return Err(());
    }
    if !desktop_server(&image(process)?, &image(parent.0)?, &command_line(process)?) {
        return Err(());
    }
    let actual = selected_home(&environment(process, &basic)?).ok_or(())?;
    if !Path::new(&actual).is_absolute() {
        return Err(());
    }
    let actual = std::fs::canonicalize(actual).map_err(|_| ())?;
    let expected = std::fs::canonicalize(home).map_err(|_| ())?;
    if normalize(&actual.to_string_lossy()) != normalize(&expected.to_string_lossy()) {
        return Err(());
    }
    Ok(Instance {
        pid: i32::try_from(pid).map_err(|_| ())?,
        started_us: since,
    })
}

pub(crate) fn instances(home: &Path) -> Result<Vec<Instance>, ()> {
    if cfg!(test) || std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
        return Ok(Vec::new());
    }
    Ok(entries()?
        .into_iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case("codex.exe"))
        .filter_map(|entry| {
            let process = open(
                entry.pid,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            )
            .ok()?;
            identity(process.0, entry.pid, home).ok()
        })
        .collect())
}

fn children(root: &Instance) -> Result<Vec<Instance>, ()> {
    let processes = entries()?
        .into_iter()
        .filter_map(|entry| {
            let process = open(entry.pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
            if !same_user(process.0).ok()? {
                return None;
            }
            let basic: PROCESS_BASIC_INFORMATION =
                query(process.0, ProcessBasicInformation).ok()?;
            if basic.InheritedFromUniqueProcessId != entry.parent as usize {
                return None;
            }
            Some(Process {
                identity: Instance {
                    pid: i32::try_from(entry.pid).ok()?,
                    started_us: started(process.0).ok()?,
                },
                parent: i32::try_from(entry.parent).ok()?,
            })
        })
        .collect::<Vec<_>>();
    Ok(descendants(&processes, root))
}

fn terminate_identity(expected: &Instance) -> Result<(), ()> {
    let process = match open(
        expected.pid as u32,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
    ) {
        Ok(process) => process,
        Err(()) if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER => return Ok(()),
        Err(()) => return Err(()),
    };
    if started(process.0).ok() != Some(expected.started_us) {
        return Ok(());
    }
    if !same_user(process.0)? {
        return Err(());
    }
    if unsafe { TerminateProcess(process.0, 0) } == 0 {
        Err(())
    } else {
        Ok(())
    }
}

pub(crate) fn terminate(expected: &Instance, home: &Path) -> Result<(), ()> {
    if cfg!(test) || std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
        return Err(());
    }
    let process = match open(
        expected.pid as u32,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ | PROCESS_TERMINATE,
    ) {
        Ok(process) => process,
        Err(()) if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER => return Ok(()),
        Err(()) => return Err(()),
    };
    if started(process.0).ok() != Some(expected.started_us) {
        return Ok(());
    }
    if identity(process.0, expected.pid as u32, home).as_ref() != Ok(expected) {
        return Err(());
    }
    let descendants = children(expected)?;
    if unsafe { TerminateProcess(process.0, 0) } == 0 {
        return Err(());
    }
    // MCP/tool descendants inherit stdout. Even when a later listing or one
    // child fails, close the other known copies so the app can detect EOF.
    finish_descendants(descendants, children(expected), terminate_identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_process_access_is_disabled_in_tests() {
        let home = Path::new(r"C:\synthetic\codex-home");
        assert_eq!(instances(home), Ok(Vec::new()));
        assert!(terminate(
            &Instance {
                pid: i32::MAX,
                started_us: 0
            },
            home
        )
        .is_err());
    }
}
