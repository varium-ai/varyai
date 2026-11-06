//! Inspect only the desktop application's own stdio server in the selected home.
use super::Instance;
use std::{
    ffi::CStr,
    mem,
    path::{Path, PathBuf},
};

fn info(pid: i32) -> Option<libc::proc_bsdinfo> {
    let mut value = mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = mem::size_of::<libc::proc_bsdinfo>() as i32;
    // The kernel initializes the complete structure before it is read.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            value.as_mut_ptr().cast(),
            size,
        )
    };
    (read == size).then(|| unsafe { value.assume_init() })
}

fn executable(pid: i32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let read = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if read <= 0 {
        return None;
    }
    let value = CStr::from_bytes_until_nul(&buffer).ok()?.to_str().ok()?;
    Some(PathBuf::from(value))
}

fn arguments(pid: i32) -> Option<(Vec<String>, Vec<String>)> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut size = buffer.len();
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return None;
    }
    buffer.truncate(size);
    decode_arguments(&buffer)
}

fn decode_arguments(buffer: &[u8]) -> Option<(Vec<String>, Vec<String>)> {
    let argc = i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?);
    if !(1..=4096).contains(&argc) {
        return None;
    }
    let rest = buffer.get(4..)?;
    let mut offset = rest.iter().position(|b| *b == 0)? + 1;
    while rest.get(offset) == Some(&0) {
        offset += 1;
    }
    let mut fields = rest.get(offset..)?.split(|b| *b == 0);
    let args = fields
        .by_ref()
        .take(argc as usize)
        .map(|b| String::from_utf8(b.to_vec()).ok())
        .collect::<Option<Vec<_>>>()?;
    if args.len() != argc as usize {
        return None;
    }
    // Environment is inspected in memory and never included in logs or errors.
    let env = fields
        .filter(|b| !b.is_empty())
        .filter_map(|b| String::from_utf8(b.to_vec()).ok())
        .collect();
    Some((args, env))
}

fn desktop_server(exe: &Path, parent: &Path, args: &[String]) -> bool {
    let Some(bundle) = exe.ancestors().find(|p| {
        matches!(
            p.file_name().and_then(|v| v.to_str()),
            Some("ChatGPT.app" | "Codex.app")
        )
    }) else {
        return false;
    };
    let parent_name = parent.file_name().and_then(|v| v.to_str());
    parent.parent() == Some(bundle.join("Contents/MacOS").as_path())
        && matches!(parent_name, Some("ChatGPT" | "Codex"))
        && exe.file_name().is_some_and(|name| name == "codex")
        && args.iter().any(|arg| arg == "app-server")
        && !args.iter().any(|arg| {
            arg == "--listen"
                || arg.starts_with("--listen=")
                || arg.starts_with("--managed-daemon")
                || arg == "daemon"
        })
}

fn instance(pid: i32, home: &Path) -> Option<Instance> {
    let info = info(pid)?;
    if info.pbi_uid != unsafe { libc::geteuid() } {
        return None;
    }
    let exe = executable(pid)?;
    // Filter before reading arguments from other processes.
    if exe.file_name()?.to_str()? != "codex" {
        return None;
    }
    let parent = executable(info.pbi_ppid as i32)?;
    let (args, env) = arguments(pid)?;
    if !desktop_server(&exe, &parent, &args) {
        return None;
    }
    let get = |key: &str| env.iter().find_map(|v| v.strip_prefix(key));
    let actual = get("CODEX_HOME=")
        .map(PathBuf::from)
        .or_else(|| get("HOME=").map(|value| Path::new(value).join(".codex")))?;
    if fs_canonical(&actual)? != fs_canonical(home)? {
        return None;
    }
    Some(Instance {
        pid,
        started_us: info
            .pbi_start_tvsec
            .checked_mul(1_000_000)?
            .checked_add(info.pbi_start_tvusec)?,
    })
}

fn fs_canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

pub(super) fn instances(home: &Path) -> Result<Vec<Instance>, ()> {
    if cfg!(test) || std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
        return Ok(Vec::new());
    }
    let size = unsafe { libc::proc_listpids(1, 0, std::ptr::null_mut(), 0) };
    if size <= 0 {
        return Err(());
    }
    let mut pids = vec![0i32; size as usize / mem::size_of::<i32>() + 1024];
    let bytes = unsafe {
        libc::proc_listpids(
            1,
            0,
            pids.as_mut_ptr().cast(),
            (pids.len() * mem::size_of::<i32>()) as i32,
        )
    };
    if bytes <= 0 {
        return Err(());
    }
    pids.truncate(bytes as usize / mem::size_of::<i32>());
    Ok(pids
        .into_iter()
        .filter(|pid| *pid > 0)
        .filter_map(|pid| instance(pid, home))
        .collect())
}

pub(super) fn terminate(expected: &Instance, home: &Path) -> Result<(), ()> {
    if cfg!(test) || std::env::var_os("CC_SWITCH_TEST_HOME").is_some() {
        return Err(());
    }
    // A replacement from Magpie or the app itself must never receive this signal.
    if instance(expected.pid, home).as_ref() != Some(expected) {
        return Ok(());
    }
    let result = unsafe { libc::kill(expected.pid, libc::SIGTERM) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_process_access_is_disabled_in_tests() {
        let home = Path::new("/synthetic/codex-home");
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

    #[test]
    fn native_discovery_excludes_daemons_other_apps_and_explicit_listeners() {
        let exe = Path::new("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex");
        let parent = Path::new("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
        let args = vec![exe.to_string_lossy().into_owned(), "app-server".into()];
        assert!(desktop_server(exe, parent, &args));
        assert!(!desktop_server(
            exe,
            Path::new("/Applications/Other.app/Contents/MacOS/Other"),
            &args
        ));
        for extra in [
            "--listen",
            "--listen=stdio://",
            "--managed-daemon",
            "daemon",
        ] {
            let mut args = args.clone();
            args.push(extra.into());
            assert!(!desktop_server(exe, parent, &args));
        }
    }

    #[test]
    fn native_argument_decoder_handles_padding_and_truncation() {
        let mut bytes = 2i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(
            b"/synthetic/codex\0\0\0codex\0app-server\0CODEX_HOME=/synthetic/home\0\0",
        );
        let (args, env) = decode_arguments(&bytes).expect("synthetic process");
        assert_eq!(args, ["codex", "app-server"]);
        assert_eq!(env, ["CODEX_HOME=/synthetic/home"]);
        assert!(decode_arguments(&[1, 2]).is_none());
    }
}
