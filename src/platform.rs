use smol_str::SmolStr;

pub mod os {
    pub const WIN: &str = "win";
    pub const WIN_GNU: &str = "win_gnu";
    pub const LINUX: &str = "linux";
    pub const LINUX_MUSL: &str = "linux_musl";
    pub const MAC: &str = "mac";
    pub const SOLARIS: &str = "solaris";
    pub const AIX: &str = "aix";
    pub const FREEBSD: &str = "freebsd";
    pub const NETBSD: &str = "netbsd";
    pub const OPENBSD: &str = "openbsd";
    pub const DRAGONFLYBSD: &str = "dragonflybsd";
    pub const ILLUMOS: &str = "illumos";
    pub const PLAN9: &str = "plan9";
}

pub mod cpu {
    pub const X86: &str = "x86";
    pub const X64: &str = "x64";
    pub const ARM32: &str = "arm32";
    pub const ARM64: &str = "arm64";
    pub const ARMV6L: &str = "armv6l";
    pub const LOONG64: &str = "loong64";
    pub const RISCV32: &str = "riscv32";
    pub const RISCV64: &str = "riscv64";
    pub const PPC32: &str = "ppc32";
    pub const PPC64: &str = "ppc64";
    pub const PPC64LE: &str = "ppc64le";
    pub const SPARC32: &str = "sparc32";
    pub const SPARC64: &str = "sparc64";
    pub const MIPS32: &str = "mips32";
    pub const MIPS64: &str = "mips64";
    pub const MIPS32LE: &str = "mips32le";
    pub const MIPS64LE: &str = "mips64le";
    pub const S390X: &str = "s390x";
}

pub fn create_platform_string(cpu: &str, os: &str) -> SmolStr {
    format!("{}-{}", cpu, os).into()
}

#[allow(unreachable_code)]
pub fn current_os() -> Option<&'static str> {
    #[cfg(target_os = "windows")]
    return Some(os::WIN);

    #[cfg(target_os = "linux")]
    return Some(os::LINUX);

    #[cfg(target_os = "macos")]
    return Some(os::MAC);

    None
}

#[allow(unreachable_code)]
pub fn current_cpu() -> Option<&'static str> {
    #[cfg(target_arch = "x86")]
    return Some(cpu::X86);

    #[cfg(target_arch = "x86_64")]
    return Some(cpu::X64);

    #[cfg(target_arch = "arm")]
    return Some(cpu::ARM32);

    #[cfg(target_arch = "aarch64")]
    return Some(cpu::ARM64);

    #[cfg(target_arch = "riscv32")]
    return Some(cpu::RISCV32);

    #[cfg(target_arch = "riscv64")]
    return Some(cpu::RISCV64);

    // #[cfg(target_arch = "ppc32")]
    // return Some(cpu::PPC32);

    // #[cfg(target_arch = "ppc64")]
    // return Some(cpu::PPC64);

    #[cfg(target_arch = "sparc")]
    return Some(cpu::SPARC32);

    #[cfg(target_arch = "sparc64")]
    return Some(cpu::SPARC64);

    None
}
