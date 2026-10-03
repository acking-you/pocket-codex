//! Host platform detection (TRD §4.3.2, T13).

use super::super::error::AcpError;

/// The host platform as the catalog and npm see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    /// `darwin-aarch64` | `darwin-x86_64` | `linux-aarch64` | `linux-x86_64` |
    /// `windows-aarch64` | `windows-x86_64`
    pub key: &'static str,
    /// `darwin` | `linux` | `win32`
    pub npm_os: &'static str,
    /// `arm64` | `x64`
    pub npm_cpu: &'static str,
    /// `Some("glibc")` on Linux.
    pub libc: Option<&'static str>,
    /// AVX2 available (always true off x86_64).
    pub avx2: bool,
}

impl Platform {
    /// Archive target keys to try, in order (`-baseline` first on x86_64
    /// without AVX2).
    pub fn archive_keys(&self) -> Vec<String> {
        if self.key.ends_with("x86_64") && !self.avx2 {
            vec![format!("{}-baseline", self.key), self.key.to_string()]
        } else {
            vec![self.key.to_string()]
        }
    }
}

/// Detect the running platform.
pub fn detect() -> Result<Platform, AcpError> {
    let musl = cfg!(target_os = "linux")
        && ["/lib/ld-musl-x86_64.so.1", "/lib/ld-musl-aarch64.so.1"]
            .iter()
            .any(|p| std::path::Path::new(p).exists());
    detect_from(std::env::consts::OS, std::env::consts::ARCH, musl, avx2())
}

#[cfg(target_arch = "x86_64")]
fn avx2() -> bool {
    std::is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn avx2() -> bool {
    true
}

/// Pure function behind [`detect`], for tests on any runner.
pub fn detect_from(
    os: &str,
    arch: &str,
    musl_present: bool,
    avx2: bool,
) -> Result<Platform, AcpError> {
    let (npm_os, os_key) = match os {
        "macos" => ("darwin", "darwin"),
        "linux" => ("linux", "linux"),
        "windows" => ("win32", "windows"),
        other => return Err(AcpError::UnsupportedPlatform(format!("{other} is not supported"))),
    };
    let (npm_cpu, arch_key) = match arch {
        "aarch64" => ("arm64", "aarch64"),
        "x86_64" => ("x64", "x86_64"),
        other => return Err(AcpError::UnsupportedPlatform(format!("{other} is not supported"))),
    };
    if os == "linux" && musl_present {
        return Err(AcpError::UnsupportedPlatform("musl Linux is not supported yet".into()));
    }
    let key = match (os_key, arch_key) {
        ("darwin", "aarch64") => "darwin-aarch64",
        ("darwin", _) => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", _) => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        _ => "windows-x86_64",
    };
    Ok(Platform {
        key,
        npm_os,
        npm_cpu,
        libc: (os == "linux").then_some("glibc"),
        avx2: arch != "x86_64" || avx2,
    })
}
