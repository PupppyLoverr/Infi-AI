//! tty/DRM backend. Its body (`udev_imp.rs`) is compiled once per renderer:
//! GLES for hardware GPUs, Pixman when the only GL is a software rasterizer.

use smithay::backend::{renderer::DebugFlags, session::libseat::LibSeatSession};

pub trait TtyBackend {
    fn session(&mut self) -> &mut LibSeatSession;
    fn debug_flags(&self) -> DebugFlags;
    fn set_debug_flags(&mut self, flags: DebugFlags);
}

pub fn run_udev() {
    if use_pixman() {
        tracing::info!("renderer: pixman (no hardware GL)");
        crate::udev_pixman::run_udev();
    } else {
        tracing::info!("renderer: gles");
        crate::udev_gles::run_udev();
    }
}

/// `COSMOS_RENDERER=pixman|gles` forces a renderer. Otherwise the GL probe
/// runs in a child process so a software GL never maps into the compositor.
fn use_pixman() -> bool {
    match std::env::var("COSMOS_RENDERER").as_deref() {
        Ok("pixman") => return true,
        Ok("gles") => return false,
        _ => {}
    }
    let status = std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).arg("--probe-gl").status());
    match status {
        Ok(status) => !status.success(),
        Err(err) => {
            tracing::warn!(?err, "GL probe failed to run; using gles");
            false
        }
    }
}

/// Exit status for `--probe-gl`: 0 when a render node has a hardware EGL device.
pub fn probe_gl() -> i32 {
    use smithay::backend::{
        allocator::gbm::GbmDevice,
        drm::DrmDeviceFd,
        egl::{EGLDevice, EGLDisplay},
    };
    let Ok(dir) = std::fs::read_dir("/dev/dri") else {
        return 1;
    };
    for entry in dir.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("renderD") {
            continue;
        }
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path())
        else {
            continue;
        };
        let fd = DrmDeviceFd::new(smithay::utils::DeviceFd::from(std::os::fd::OwnedFd::from(
            file,
        )));
        let Ok(gbm) = GbmDevice::new(fd) else {
            continue;
        };
        // SAFETY: `gbm` stays alive for as long as the display is used.
        let Ok(display) = (unsafe { EGLDisplay::new(gbm) }) else {
            continue;
        };
        if EGLDevice::device_for_display(&display).is_ok_and(|d| !d.is_software()) {
            return 0;
        }
    }
    1
}
