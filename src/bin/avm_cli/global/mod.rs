use clap::Args;

use any_version_manager::platform::{cpu, os};

use crate::avm_cli::general_tool::{ToolName, ToolSet};

#[derive(Debug, Clone, Args)]
pub struct ToolGuideArgs {
    #[arg(value_enum, help = "Tool name. Omit to list all supported tools.")]
    pub tool: Option<ToolName>,
}

pub fn run_tool_guide(args: ToolGuideArgs, tools: &ToolSet) {
    match args.tool {
        Some(tool) => print_tool_detail(tool, tools),
        None => print_tool_list(tools),
    }
}

fn print_tool_list(tools: &ToolSet) {
    println!("Supported tools:\n");
    for (name, info) in tools.all_infos() {
        println!("- {}: {}", name, info.about);
    }

    println!(
        "\nUse `avm tool <tool>` to see install examples and available platform/flavor values."
    );
    println!("Example: `avm install node --lts`");
    println!("Example: `avm install liberica --platform x64-linux --flavor jdk`");
}

fn print_tool_detail(tool: ToolName, tools: &ToolSet) {
    let info = tools.tool_info(tool);
    let name = tool.command_name();
    println!("Tool: {}", name);
    println!("Description: {}", info.about);
    println!();
    println!("Install examples:");
    println!("- avm install {}", name);
    if info.all_platforms.is_some() {
        println!("- avm install {} --platform <platform>", name);
    }

    if info.all_flavors.is_some() {
        println!("- avm install {} --flavor <flavor>", name);
    }

    if let Some(default_platform) = &info.default_platform {
        println!("Default platform: {}", default_platform);
    }
    if let Some(default_flavor) = &info.default_flavor {
        println!("Default flavor: {}", default_flavor);
    }

    if let Some(platforms) = &info.all_platforms {
        println!();
        println!("Available platforms:");
        for platform in platforms {
            println!("- {}: {}", platform, describe_platform(platform));
        }
    }

    if let Some(flavors) = &info.all_flavors {
        println!();
        println!("Available flavors:");
        for flavor in flavors {
            let detail = tools.describe_flavor(tool, flavor);
            println!("- {}: {}", flavor, detail);
        }
    }
}

fn describe_platform(platform: &str) -> String {
    let Some((cpu, os)) = platform.split_once('-') else {
        return "Target platform identifier used by the upstream package distribution.".to_string();
    };

    format!("{} CPU on {}.", describe_cpu(cpu), describe_os(os))
}

fn describe_cpu(cpu: &str) -> &'static str {
    match cpu {
        cpu::X86 => "32-bit x86",
        cpu::X64 => "64-bit x86_64",
        cpu::ARM32 => "32-bit ARM",
        cpu::ARM64 => "64-bit ARM",
        cpu::ARMV6L => "ARMv6",
        cpu::ARMV7L => "ARMv7",
        cpu::PPC32 => "32-bit PowerPC",
        cpu::PPC64 => "64-bit PowerPC",
        cpu::PPC64LE => "64-bit PowerPC (little-endian)",
        cpu::S390X => "IBM Z (s390x)",
        cpu::RISCV32 => "32-bit RISC-V",
        cpu::RISCV64 => "64-bit RISC-V",
        cpu::MIPS32 => "32-bit MIPS",
        cpu::MIPS32LE => "32-bit MIPS (little-endian)",
        cpu::MIPS64 => "64-bit MIPS",
        cpu::MIPS64LE => "64-bit MIPS (little-endian)",
        cpu::LOONG64 => "LoongArch64",
        cpu::SPARC32 => "SPARC32",
        cpu::SPARC64 => "SPARC64",
        _ => "Target architecture",
    }
}

fn describe_os(os: &str) -> &'static str {
    match os {
        os::LINUX => "Linux",
        os::LINUX_MUSL => "Linux (musl libc)",
        os::WIN | os::WIN_GNU => "Windows",
        os::MAC => "macOS",
        os::FREEBSD => "FreeBSD",
        os::OPENBSD => "OpenBSD",
        os::NETBSD => "NetBSD",
        os::DRAGONFLYBSD => "DragonFly BSD",
        os::ILLUMOS => "Illumos",
        os::SOLARIS => "Solaris",
        os::AIX => "AIX",
        os::PLAN9 => "Plan 9",
        _ => "Target operating system",
    }
}
