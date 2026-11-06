//! Native Windows lifecycle operations retain process handles across identity checks.
use super::super::FAILED;
use super::*;
use std::{
    mem, ptr,
    time::{Duration, Instant},
};
use windows_sys::{
    Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation},
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_NO_MORE_FILES, FILETIME, HANDLE, HWND,
            INVALID_HANDLE_VALUE, LPARAM, UNICODE_STRING,
        },
        Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
        Storage::Packaging::Appx::GetApplicationUserModelId,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            RemoteDesktop::ProcessIdToSessionId,
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, GetExitCodeProcess, GetProcessTimes,
                OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, TerminateProcess,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            },
        },
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
                SW_SHOWNORMAL, WM_CLOSE,
            },
        },
    },
};

pub(super) struct Native<'a> {
    pub app: &'a str,
}
struct Handle(HANDLE);
impl Handle {
    fn new(handle: HANDLE) -> Result<Self, String> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(FAILED.into())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn open(pid: u32, access: u32) -> Result<Handle, String> {
    Handle::new(unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })
}
fn utf16(value: &[u16]) -> Result<String, String> {
    String::from_utf16(
        &value[..value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len())],
    )
    .map_err(|_| FAILED.into())
}
fn user(process: HANDLE) -> Result<Vec<usize>, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(FAILED.into());
    }
    let token = Handle::new(token)?;
    let mut size = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size);
    }
    if size < mem::size_of::<TOKEN_USER>() as u32 || size > 65536 {
        return Err(FAILED.into());
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
        return Err(FAILED.into());
    }
    Ok(buffer)
}
fn owned(process: HANDLE, pid: u32) -> Result<bool, String> {
    let own = user(unsafe { GetCurrentProcess() })?;
    let other = user(process)?;
    let own = unsafe { &*own.as_ptr().cast::<TOKEN_USER>() };
    let other = unsafe { &*other.as_ptr().cast::<TOKEN_USER>() };
    let mut own_session = 0;
    let mut other_session = 0;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut own_session) } == 0
        || unsafe { ProcessIdToSessionId(pid, &mut other_session) } == 0
    {
        return Err(FAILED.into());
    }
    Ok(own_session == other_session && unsafe { EqualSid(own.User.Sid, other.User.Sid) } != 0)
}
fn identity(handle: HANDLE, pid: u32, parent: u32) -> Result<Process, String> {
    let mut code = 0;
    if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 || code != 259 {
        return Err(CHANGED.into());
    }
    if !owned(handle, pid)? {
        return Err(FAILED.into());
    }
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) } == 0 {
        return Err(FAILED.into());
    }
    let image = utf16(&buffer[..size as usize])?;
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(FAILED.into());
    }
    let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    let mut id = vec![0u16; 512];
    let mut size = id.len() as u32;
    let aumid = if unsafe { GetApplicationUserModelId(handle, &mut size, id.as_mut_ptr()) } == 0 {
        Some(utf16(&id)?)
    } else {
        None
    };
    Ok(Process {
        pid,
        parent,
        image,
        args: vec![],
        created,
        aumid,
    })
}
fn command_line(handle: HANDLE) -> Result<Vec<String>, String> {
    let mut size = 0;
    unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size < mem::size_of::<UNICODE_STRING>() as u32 || size > 256 * 1024 {
        return Err(FAILED.into());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(mem::size_of::<usize>())];
    if unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } < 0
    {
        return Err(FAILED.into());
    }
    let value = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
    let start = value.Buffer as usize;
    if value.Length % 2 != 0
        || value.Length > value.MaximumLength
        || size as usize > buffer.len() * mem::size_of::<usize>()
        || start % mem::align_of::<u16>() != 0
        || start < buffer.as_ptr() as usize
        || start
            .checked_add(value.Length as usize)
            .is_none_or(|end| end > buffer.as_ptr() as usize + size as usize)
    {
        return Err(FAILED.into());
    }
    let mut command =
        unsafe { std::slice::from_raw_parts(value.Buffer, usize::from(value.Length) / 2) }.to_vec();
    command.push(0);
    let mut count = 0;
    let argv =
        unsafe { windows_sys::Win32::UI::Shell::CommandLineToArgvW(command.as_ptr(), &mut count) };
    if argv.is_null() {
        return Err(FAILED.into());
    }
    let result = (|| {
        if !(1..=4096).contains(&count) {
            return Err(FAILED.into());
        }
        let mut args = Vec::new();
        for index in 0..count as usize {
            let value = unsafe { *argv.add(index) };
            let mut length = 0;
            while length < 32768 && unsafe { *value.add(length) } != 0 {
                length += 1;
            }
            if length == 32768 {
                return Err(FAILED.into());
            }
            args.push(utf16(unsafe { std::slice::from_raw_parts(value, length) })?);
        }
        Ok(args)
    })();
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(argv.cast());
    }
    result
}
fn snapshot() -> Result<Vec<(u32, u32, String)>, String> {
    let snapshot = Handle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })?;
    let mut entry = PROCESSENTRY32W {
        dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    let mut entries = Vec::new();
    while more != 0 {
        entries.push((
            entry.th32ProcessID,
            entry.th32ParentProcessID,
            utf16(&entry.szExeFile)?,
        ));
        more = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(FAILED.into());
    }
    Ok(entries)
}
fn same_identity(expected: &Process, actual: &Process) -> bool {
    expected.pid == actual.pid
        && expected.created == actual.created
        && normalize(&expected.image) == normalize(&actual.image)
        && expected.aumid == actual.aumid
}
fn checked(process: &Process, access: u32) -> Result<Handle, String> {
    let handle = open(process.pid, access)?;
    if !same_identity(process, &identity(handle.0, process.pid, process.parent)?) {
        return Err(CHANGED.into());
    }
    Ok(handle)
}
struct Windows<'a> {
    processes: &'a [Process],
    close: bool,
    shown: bool,
    failed: bool,
}
unsafe extern "system" fn window_callback(window: HWND, context: LPARAM) -> i32 {
    // EnumWindows calls synchronously; this stack context stays live throughout.
    let context = unsafe { &mut *(context as *mut Windows<'_>) };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
    }
    if let Some(process) = context.processes.iter().find(|process| process.pid == pid) {
        let Ok(_handle) = checked(process, 0) else {
            context.failed = true;
            return 0;
        };
        if unsafe { IsWindowVisible(window) } != 0 {
            context.shown = true;
        }
        if context.close && unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } == 0 {
            context.failed = true;
            return 0;
        }
    }
    1
}
fn windows(processes: &[Process], close: bool) -> Result<bool, String> {
    let mut context = Windows {
        processes,
        close,
        shown: false,
        failed: false,
    };
    if unsafe {
        EnumWindows(
            Some(window_callback),
            &mut context as *mut Windows<'_> as LPARAM,
        )
    } == 0
        || context.failed
    {
        return Err(RUNNING.into());
    }
    Ok(context.shown)
}
fn shell(file: &str, args: Option<&str>) -> Result<(), String> {
    if file.contains('\0') || args.is_some_and(|args| args.contains('\0')) {
        return Err(FAILED.into());
    }
    let file = file.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let args = args.map(|args| args.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            ptr::null(),
            file.as_ptr(),
            args.as_ref().map_or(ptr::null(), |args| args.as_ptr()),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err(FAILED.into())
    }
}
impl Runtime for Native<'_> {
    fn processes(&self) -> Result<Vec<Process>, String> {
        if cfg!(test) || std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
            return Err(FAILED.into());
        }
        let exe = match self.app {
            "workbuddy" => "WorkBuddy.exe",
            "codex" => "ChatGPT.exe",
            _ => "claude.exe",
        };
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let programs = std::env::var("ProgramFiles").unwrap_or_default();
        let mut processes = Vec::new();
        for (pid, parent, name) in snapshot()? {
            if !name.eq_ignore_ascii_case(exe) {
                continue;
            }
            let mut caller_session = 0;
            let mut target_session = 0;
            if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut caller_session) } == 0 {
                return Err(FAILED.into());
            }
            if unsafe { ProcessIdToSessionId(pid, &mut target_session) } == 0
                || target_session != caller_session
            {
                continue;
            }
            let handle = open(pid, 0)?;
            if !owned(handle.0, pid)? {
                continue;
            }
            let mut process = identity(handle.0, pid, parent)?;
            if !matches_image(self.app, &process.image, &local, &programs) {
                continue;
            }
            process.args = command_line(handle.0)?;
            if !runs_script(&process.args) {
                processes.push(process);
            }
        }
        processes.sort_by_key(|process| process.pid);
        Ok(processes)
    }
    fn close(&self, processes: &[Process]) -> Result<(), String> {
        let _handles = processes
            .iter()
            .map(|process| checked(process, 0))
            .collect::<Result<Vec<_>, _>>()?;
        windows(processes, true).map(|_| ())
    }
    fn wait_closed(&self, processes: &[Process]) -> Result<Closed, String> {
        let start = Instant::now();
        loop {
            let live = self.processes()?;
            if live.is_empty() {
                return Ok(Closed::Exited);
            }
            if live.iter().any(|process| !processes.contains(process)) {
                return Err(CHANGED.into());
            }
            if start.elapsed() >= Duration::from_secs(10)
                || (start.elapsed() >= Duration::from_secs(1) && !windows(&live, false)?)
            {
                return Ok(Closed::Remains);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn end_tree(&self, main: &Process) -> Result<(), String> {
        let root = checked(main, PROCESS_TERMINATE)?;
        let entries = snapshot()?;
        let mut tree = vec![(main.clone(), root)];
        loop {
            let mut added = false;
            for (pid, parent, _) in &entries {
                if tree.iter().any(|(process, _)| process.pid == *pid) {
                    continue;
                }
                let Some((ancestor, _)) = tree.iter().find(|(process, _)| process.pid == *parent)
                else {
                    continue;
                };
                let child = open(*pid, PROCESS_TERMINATE)?;
                let identity = identity(child.0, *pid, *parent)?;
                if identity.created < ancestor.created {
                    return Err(CHANGED.into());
                }
                tree.push((identity, child));
                added = true;
            }
            if !added {
                break;
            }
        }
        // Retained handles prevent PID reuse from redirecting termination. End
        // the verified root first so it cannot intentionally start more workers.
        complete_tree(&tree, |(process, handle)| {
            let mut code = 0;
            if unsafe { GetExitCodeProcess(handle.0, &mut code) } != 0 && code != 259 {
                return Ok(());
            }
            if !same_identity(process, &identity(handle.0, process.pid, process.parent)?) {
                return Err(CHANGED.into());
            }
            if unsafe { TerminateProcess(handle.0, 0) } == 0 {
                // Children may end themselves when the root exits. Continue
                // cleaning up every other retained child even on a real error.
                if unsafe { GetExitCodeProcess(handle.0, &mut code) } == 0 || code == 259 {
                    return Err(RUNNING.into());
                }
            }
            Ok(())
        })
    }
    fn wait_gone(&self) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if self.processes()?.is_empty() {
                return Ok(());
            }
            if start.elapsed() >= Duration::from_secs(10) {
                return Err(RUNNING.into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn launch(&self, main: &Process) -> Result<(), String> {
        if let Some(id) = launch_aumid(self.app, main.aumid.as_deref()) {
            let explorer = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .ok_or(FAILED)?
                .join("explorer.exe");
            if !explorer.is_file() {
                return Err(FAILED.into());
            }
            return shell(
                &explorer.to_string_lossy(),
                Some(&format!(r"shell:AppsFolder\{id}")),
            );
        }
        let mut image = std::path::PathBuf::from(&main.image);
        if let Some(parent) = image.parent().filter(|parent| {
            parent.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("app-")
            })
        }) {
            if let (Some(root), Some(name)) = (parent.parent(), image.file_name()) {
                let stub = root.join(name);
                if stub.is_file() {
                    image = stub;
                }
            }
        }
        if !image.is_file() || image.is_symlink() {
            return Err(FAILED.into());
        }
        shell(&image.to_string_lossy(), None)
    }
    fn wait_back(&self) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if !self.processes()?.is_empty() {
                return Ok(());
            }
            if start.elapsed() >= Duration::from_secs(20) {
                return Err(FAILED.into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
