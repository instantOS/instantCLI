//! A conservative format check before changing permissions or launching a file.
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

pub(super) fn warning(path: &Path) -> Result<Option<String>> {
    let mut header = Vec::new();
    File::open(path)
        .with_context(|| format!("Cannot inspect {}", path.display()))?
        .take(256)
        .read_to_end(&mut header)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    Ok(check_header(&header))
}

fn check_header(header: &[u8]) -> Option<String> {
    if header.starts_with(b"#!") {
        return check_script(header);
    }
    if header.starts_with(b"\x7fELF") {
        return check_elf(header);
    }
    Some("The file has no recognized executable header or script shebang (#!).".into())
}

fn check_script(header: &[u8]) -> Option<String> {
    let line = header
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    if line.len() == 256 || line.ends_with(b"\r") {
        return Some("The script's shebang is too long or uses Windows line endings.".into());
    }
    let Ok(line) = std::str::from_utf8(&line[2..]) else {
        return Some("The script's interpreter line is not valid text.".into());
    };
    let mut words = line.split_ascii_whitespace();
    let Some(interpreter) = words.next() else {
        return Some("The script's shebang does not specify an interpreter.".into());
    };
    if !Path::new(interpreter).is_absolute() || which::which(interpreter).is_err() {
        return Some(format!(
            "The script's interpreter is unavailable: {interpreter}"
        ));
    }
    if Path::new(interpreter)
        .file_name()
        .is_some_and(|name| name == "env")
    {
        let arguments: Vec<_> = words.collect();
        let command = match arguments.as_slice() {
            [command] if !command.starts_with('-') => *command,
            ["-S", command, ..] if !command.starts_with('-') => *command,
            _ => return Some("The script's env shebang cannot be verified.".into()),
        };
        if which::which(command).is_err() {
            return Some(format!(
                "The script's interpreter is not on PATH: {command}"
            ));
        }
    }
    None
}

fn check_elf(header: &[u8]) -> Option<String> {
    let class = if cfg!(target_pointer_width = "64") {
        2
    } else {
        1
    };
    let endian = if cfg!(target_endian = "little") { 1 } else { 2 };
    let (size, entry_end, phnum) = if class == 2 {
        (64, 32, 56)
    } else {
        (52, 28, 44)
    };
    if header.len() < size || header[6] != 1 {
        return Some("The ELF executable header is incomplete or invalid.".into());
    }
    if header[4] != class || header[5] != endian {
        return Some("The binary's architecture differs from this system's native format.".into());
    }
    let machine = u16::from_ne_bytes([header[18], header[19]]);
    let native_machine = match std::env::consts::ARCH {
        "x86" => Some(3),
        "x86_64" => Some(62),
        "arm" => Some(40),
        "aarch64" => Some(183),
        "riscv32" | "riscv64" => Some(243),
        "loongarch64" => Some(258),
        "powerpc" => Some(20),
        "powerpc64" => Some(21),
        "s390x" => Some(22),
        _ => None,
    };
    if native_machine != Some(machine) {
        return Some("The binary does not target this system's native CPU architecture.".into());
    }
    let kind = u16::from_ne_bytes([header[16], header[17]]);
    if !matches!(kind, 2 | 3)
        || header[24..entry_end].iter().all(|byte| *byte == 0)
        || header[phnum..phnum + 2] == [0, 0]
    {
        return Some(
            "The ELF file has no runnable entry point; it may be a library or object file.".into(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_files_and_misleading_extensions_need_confirmation() -> Result<()> {
        for header in [b"".as_slice(), b"plain text", b"\x89PNG\r\n", b"\x7fELF"] {
            assert!(check_header(header).is_some());
        }
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("fake.AppImage");
        std::fs::write(&path, "not a program")?;
        assert!(warning(&path)?.is_some());
        Ok(())
    }

    #[test]
    fn scripts_require_a_usable_shebang() {
        assert!(check_header(b"#!/bin/sh\necho hello\n").is_none());
        for header in [
            b"#!\n".as_slice(),
            b"#!/nonexistent/ins-test-interpreter\n",
            b"#!/bin/sh\r\necho hello\r\n",
            b"#!/usr/bin/env ins-nonexistent-test-interpreter\n",
            b"#!/usr/bin/env sh -e\n",
        ] {
            assert!(check_header(header).is_some(), "{header:?}");
        }
        assert!(check_header(b"#!/usr/bin/env sh\necho hello\n").is_none());
        assert!(check_header(b"#!/usr/bin/env -S sh -e\necho hello\n").is_none());
    }

    #[test]
    fn native_elf_is_recognized_but_libraries_and_foreign_binaries_are_flagged() -> Result<()> {
        let mut header = Vec::new();
        File::open(std::env::current_exe()?)?
            .take(256)
            .read_to_end(&mut header)?;
        assert!(check_header(&header).is_none());
        let mut library = header.clone();
        library[24..32].fill(0);
        assert!(check_header(&library).is_some());
        header[18..20].copy_from_slice(&0xffff_u16.to_ne_bytes());
        assert!(check_header(&header).is_some());
        Ok(())
    }
}
