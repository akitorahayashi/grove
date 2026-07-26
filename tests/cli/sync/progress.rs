use std::fs::File;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::harness::{TestContext, path_with_wrapper};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sync_progress_completes_when_stderr_is_a_terminal() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    let path = path_with_wrapper(
        &ctx,
        "sync-progress",
        "if [ \"$1\" = rev-parse ] && [ \"${2:-}\" = --is-inside-work-tree ]; then sleep 0.3; fi",
    );
    let mut master = -1;
    let mut slave = -1;
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "failed to open pseudo-terminal");
    let mut master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let rendered = std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("failed to read pseudo-terminal: {error}"),
            }
        }
        output
    });

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("gv"))
        .current_dir(ctx.workspace())
        .env("XDG_CACHE_HOME", ctx.cache_home())
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .stdout(Stdio::null())
        .stderr(Stdio::from(slave))
        .spawn()
        .expect("failed to run gv");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("failed to poll gv") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("failed to kill deadlocked gv");
            child.wait().expect("failed to reap deadlocked gv");
            let rendered = rendered.join().expect("pseudo-terminal reader panicked");
            panic!(
                "gv did not finish while rendering progress to a terminal:\n{}",
                String::from_utf8_lossy(&rendered)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let _rendered = rendered.join().expect("pseudo-terminal reader panicked");
    assert!(status.success(), "gv exited with {status}");
}
