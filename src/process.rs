//! The program a pane's shell runs, read from the kernel. Rust's standard library cannot list
//! processes, so macOS answers through the C library every binary already links, and Linux
//! through /proc.

pub struct Program {
    pub pid: u32,
    pub args: Vec<String>,
}

/// The child the shell with PID started first, which is the program in its pane.
pub fn first_child(pid: u32) -> Option<Program> {
    let child = sys::children(pid).into_iter().min()?;
    Some(Program {
        pid: child,
        args: sys::args(child)?,
    })
}

#[cfg(target_os = "macos")]
use macos as sys;
#[cfg(not(target_os = "macos"))]
use procfs as sys;

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_int, c_void};
    use std::ptr;

    const PROC_PPID_ONLY: u32 = 6;
    const CTL_KERN: c_int = 1;
    const KERN_PROCARGS2: c_int = 49;

    unsafe extern "C" {
        fn proc_listpids(kind: u32, typeinfo: u32, buffer: *mut c_void, buffersize: c_int)
        -> c_int;
        fn sysctl(
            name: *mut c_int,
            namelen: u32,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }

    pub fn children(pid: u32) -> Vec<u32> {
        let mut pids = vec![0i32; 64];
        loop {
            let size = (pids.len() * size_of::<i32>()) as c_int;
            // SAFETY: the buffer holds `size` bytes, and the call writes at most that many.
            let used =
                unsafe { proc_listpids(PROC_PPID_ONLY, pid, pids.as_mut_ptr().cast(), size) };
            if used < 0 {
                return Vec::new();
            }
            // A full buffer may have cut the list short, so it grows and asks again.
            if used < size {
                pids.truncate(used as usize / size_of::<i32>());
                break;
            }
            pids.resize(pids.len() * 2, 0);
        }
        pids.into_iter()
            .filter(|&p| p > 0)
            .map(|p| p as u32)
            .collect()
    }

    pub fn args(pid: u32) -> Option<Vec<String>> {
        let mut mib = [CTL_KERN, KERN_PROCARGS2, pid as c_int];
        let mut size = 0usize;
        // SAFETY: with no buffer, sysctl only writes the size it needs into `size`.
        let sized = unsafe {
            sysctl(
                mib.as_mut_ptr(),
                3,
                ptr::null_mut(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        if sized != 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        // SAFETY: the buffer holds `size` bytes, and sysctl writes at most that many and
        // stores how many it wrote back into `size`.
        let read = unsafe {
            sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        if read != 0 {
            return None;
        }
        buf.truncate(size);
        parse_procargs(&buf)
    }

    /// KERN_PROCARGS2 holds argc, the executable path, NUL padding, then the arguments.
    pub(super) fn parse_procargs(buf: &[u8]) -> Option<Vec<String>> {
        let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?) as usize;
        let rest = &buf[4..];
        let rest = &rest[rest.iter().position(|&b| b == 0)?..];
        let rest = &rest[rest.iter().position(|&b| b != 0)?..];
        let args: Vec<String> = rest
            .split(|&b| b == 0)
            .take(argc)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        (args.len() == argc).then_some(args)
    }
}

/// Linux. It uses only std::fs, so it also builds on macOS, where its parsers are tested.
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod procfs {
    use std::fs;

    pub fn children(pid: u32) -> Vec<u32> {
        // Kernels built with CONFIG_PROC_CHILDREN list each thread's children directly. A
        // thread lists only the children it forked itself, so every thread is read.
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            let lists: Vec<String> = tasks
                .flatten()
                .filter_map(|task| fs::read_to_string(task.path().join("children")).ok())
                .collect();
            if !lists.is_empty() {
                return lists
                    .iter()
                    .flat_map(|text| text.split_whitespace())
                    .filter_map(|p| p.parse().ok())
                    .collect();
            }
        }
        let Ok(dir) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|entry| {
                let child: u32 = entry.file_name().to_str()?.parse().ok()?;
                let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
                (parent(&stat)? == pid).then_some(child)
            })
            .collect()
    }

    /// The parent pid in /proc/PID/stat. It is the second field after the program name,
    /// which sits in parentheses and may itself hold spaces and parentheses.
    pub fn parent(stat: &str) -> Option<u32> {
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    pub fn args(pid: u32) -> Option<Vec<String>> {
        split_cmdline(&fs::read(format!("/proc/{pid}/cmdline")).ok()?)
    }

    /// /proc/PID/cmdline ends each argument with a NUL, and is empty for a zombie.
    pub fn split_cmdline(raw: &[u8]) -> Option<Vec<String>> {
        let raw = raw.strip_suffix(&[0])?;
        Some(
            raw.split(|&b| b == 0)
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::procfs;

    #[test]
    fn the_parent_follows_a_name_with_parentheses() {
        assert_eq!(procfs::parent("123 (a) b (c)) S 45 123 0"), Some(45));
    }

    #[test]
    fn cmdline_keeps_empty_arguments_and_skips_zombies() {
        assert_eq!(
            procfs::split_cmdline(b"sleep\0\x00300\0"),
            Some(vec!["sleep".into(), "".into(), "300".into()])
        );
        assert_eq!(procfs::split_cmdline(b""), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn procargs_skip_the_executable_path_and_padding() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend(b"/bin/sleep\0\0\0\0sleep\x00300\0HOME=/x\0");
        assert_eq!(
            super::macos::parse_procargs(&buf),
            Some(vec!["sleep".into(), "300".into()])
        );
    }

    #[test]
    fn a_spawned_child_is_found_with_its_arguments() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let expected = Some((child.id(), vec!["sleep".to_owned(), "5".to_owned()]));
        // Linux lets spawn return while exec still swaps the program, so for a moment the
        // child shows the test's own arguments or none.
        let mut found = None;
        for _ in 0..100 {
            found = super::first_child(std::process::id()).map(|p| (p.pid, p.args));
            if found == expected {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found, expected);
    }
}
