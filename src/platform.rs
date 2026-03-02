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
    pub const ARMV7L: &str = "armv7l";
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
    #[cfg(all(target_os = "windows", target_env = "gnu"))]
    return Some(os::WIN_GNU);

    #[cfg(all(target_os = "windows", not(target_env = "gnu")))]
    return Some(os::WIN);

    #[cfg(all(target_os = "linux", target_env = "musl"))]
    return Some(os::LINUX_MUSL);

    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    return Some(os::LINUX);

    #[cfg(target_os = "macos")]
    return Some(os::MAC);

    #[cfg(target_os = "solaris")]
    return Some(os::SOLARIS);

    #[cfg(target_os = "aix")]
    return Some(os::AIX);

    #[cfg(target_os = "freebsd")]
    return Some(os::FREEBSD);

    #[cfg(target_os = "netbsd")]
    return Some(os::NETBSD);

    #[cfg(target_os = "openbsd")]
    return Some(os::OPENBSD);

    #[cfg(target_os = "dragonfly")]
    return Some(os::DRAGONFLYBSD);

    #[cfg(target_os = "illumos")]
    return Some(os::ILLUMOS);

    None
}

#[allow(unreachable_code)]
pub fn current_cpu() -> Option<&'static str> {
    #[cfg(target_arch = "x86")]
    return Some(cpu::X86);

    #[cfg(target_arch = "x86_64")]
    return Some(cpu::X64);

    #[cfg(all(target_arch = "arm", target_feature = "v7"))]
    return Some(cpu::ARMV7L);

    #[cfg(all(target_arch = "arm", target_feature = "v6"))]
    return Some(cpu::ARMV6L);

    #[cfg(all(
        target_arch = "arm",
        not(any(target_feature = "v6", target_feature = "v7"))
    ))]
    return Some(cpu::ARM32);

    #[cfg(target_arch = "aarch64")]
    return Some(cpu::ARM64);

    #[cfg(target_arch = "loongarch64")]
    return Some(cpu::LOONG64);

    #[cfg(target_arch = "riscv32")]
    return Some(cpu::RISCV32);

    #[cfg(target_arch = "riscv64")]
    return Some(cpu::RISCV64);

    #[cfg(target_arch = "powerpc")]
    return Some(cpu::PPC32);

    #[cfg(target_arch = "powerpc64")]
    return Some(cpu::PPC64);

    #[cfg(target_arch = "mips")]
    return Some(cpu::MIPS32);

    #[cfg(target_arch = "mips64")]
    return Some(cpu::MIPS64);

    #[cfg(target_arch = "s390x")]
    return Some(cpu::S390X);

    #[cfg(target_arch = "sparc")]
    return Some(cpu::SPARC32);

    #[cfg(target_arch = "sparc64")]
    return Some(cpu::SPARC64);

    None
}
