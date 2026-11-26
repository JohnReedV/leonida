//! Public CLI startup checks using only original, generated PE images.
#![cfg(all(windows, target_arch = "x86_64"))]

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Run a child with file-backed output so a stalled child cannot fill a pipe.
fn run_title(root: &Path, title: &Path, arguments: &[&str]) -> Output {
    let config = root.join("leonida.toml");
    std::fs::write(&config, "").unwrap();
    let stdout = root.join("stdout.txt");
    let stderr = root.join("stderr.txt");
    let mut command = Command::new(env!("CARGO_BIN_EXE_leonida"));
    command.env_clear();
    for name in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .current_dir(root)
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("LEONIDA_CONFIG", config)
        .env("LEONIDA_CACHE_DIR", root.join("cache"))
        .env("LEONIDA_DATA_DIR", root.join("data"))
        .arg("run")
        .arg(title)
        .args(arguments)
        .stdout(Stdio::from(std::fs::File::create(&stdout).unwrap()))
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()));
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "startup child exceeded 30 seconds\nstdout: {}\nstderr: {}",
                std::fs::read_to_string(&stdout).unwrap(),
                std::fs::read_to_string(&stderr).unwrap(),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: std::fs::read(stdout).unwrap(),
        stderr: std::fs::read(stderr).unwrap(),
    }
}

#[derive(Clone, Copy)]
enum Image {
    Title,
    TitleCallingFirst,
    Dll,
    DllCallingFirst,
    ExitingDll,
}

/// A small PE writer: each image owns its strings, thunks, export and relocation.
/// Offsets passed here are file offsets; the single section maps 0x200 to RVA 0x1000.
struct Pe {
    bytes: Vec<u8>,
    next: usize,
}

impl Pe {
    fn new() -> Self {
        Self {
            bytes: vec![0; 0x1000],
            next: 0x280,
        }
    }

    fn reserve(&mut self, length: usize, alignment: usize) -> usize {
        let at = self.next.next_multiple_of(alignment);
        self.next = at + length;
        assert!(self.next <= self.bytes.len());
        at
    }

    fn text(&mut self, text: &str) -> usize {
        let at = self.reserve(text.len() + 1, 1);
        self.bytes[at..at + text.len()].copy_from_slice(text.as_bytes());
        at
    }

    fn u16(&mut self, at: usize, value: u16) {
        self.bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, at: usize, value: u32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, at: usize, value: u64) {
        self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn rva(at: usize) -> u32 {
        (at + 0xe00) as u32
    }

    fn directory(&mut self, index: usize, at: usize, length: usize) {
        self.u32(0x98 + 112 + index * 8, Self::rva(at));
        self.u32(0x98 + 116 + index * 8, length as u32);
    }

    /// All title imports use ordinal 1; only ExitProcess is imported by name.
    fn imports(&mut self, modules: &[(&str, Option<&str>)], delayed: bool) -> Vec<usize> {
        if modules.is_empty() {
            return Vec::new();
        }
        let stride = if delayed { 32 } else { 20 };
        let length = (modules.len() + 1) * stride;
        let table = self.reserve(length, 4);
        self.directory(if delayed { 13 } else { 1 }, table, length);
        let mut slots = Vec::new();
        for (index, &(module, symbol)) in modules.iter().enumerate() {
            let name = self.text(module);
            let lookup = self.reserve(16, 8);
            let slot = self.reserve(16, 8);
            let value = if let Some(symbol) = symbol {
                let hint = self.reserve(2, 2);
                let string = self.text(symbol);
                assert_eq!(string, hint + 2);
                u64::from(Self::rva(hint))
            } else {
                (1u64 << 63) | 1
            };
            self.u64(lookup, value);
            self.u64(slot, value);
            let descriptor = table + index * stride;
            if delayed {
                let handle = self.reserve(8, 8);
                self.u32(descriptor, 1); // fields contain RVAs, not absolute addresses
                self.u32(descriptor + 4, Self::rva(name));
                self.u32(descriptor + 8, Self::rva(handle));
                self.u32(descriptor + 12, Self::rva(slot));
                self.u32(descriptor + 16, Self::rva(lookup));
            } else {
                self.u32(descriptor, Self::rva(lookup));
                self.u32(descriptor + 12, Self::rva(name));
                self.u32(descriptor + 16, Self::rva(slot));
            }
            slots.push(slot);
        }
        slots
    }

    fn branch_through(&mut self, at: usize, slot: usize, call: bool) {
        self.bytes[at] = 0xff;
        self.bytes[at + 1] = if call { 0x15 } else { 0x25 };
        // RIP-relative call/jump through the import slot; both locations share one section.
        self.u32(at + 2, (slot as i32 - (at + 6) as i32) as u32);
    }
}

/// Original AMD64 code and metadata only; no compiler or game data is needed.
fn write_image(path: &Path, kind: Image, ordinary: &[&str], delayed: &[&str]) {
    let dll = matches!(
        kind,
        Image::Dll | Image::DllCallingFirst | Image::ExitingDll
    );
    let mut pe = Pe::new();
    let mut imports: Vec<_> = ordinary.iter().map(|name| (*name, None)).collect();
    if matches!(kind, Image::ExitingDll) {
        imports.push(("KERNEL32.dll", Some("ExitProcess")));
    }
    let slots = pe.imports(&imports, false);
    pe.imports(
        &delayed.iter().map(|name| (*name, None)).collect::<Vec<_>>(),
        true,
    );

    // The title normally returns 73. The chain control tail-calls ordinal 1 instead.
    pe.bytes[0x200..0x206].copy_from_slice(&[0xb8, 73, 0, 0, 0, 0xc3]);
    if matches!(kind, Image::TitleCallingFirst) {
        pe.branch_through(0x200, slots[0], false);
    }
    if dll {
        // DllMain sets our initialized byte and returns TRUE. The ordinal export reads it.
        pe.bytes[0x200..0x20d].copy_from_slice(&[
            0xc6, 0x05, 0x69, 0, 0, 0, 73, // mov byte [RVA 0x1070], 73
            0xb8, 1, 0, 0, 0, 0xc3,
        ]);
        pe.bytes[0x220..0x228].copy_from_slice(&[
            0x0f, 0xb6, 0x05, 0x49, 0, 0, 0, 0xc3, // movzx eax, byte [initialized]; ret
        ]);
        if matches!(kind, Image::DllCallingFirst) {
            // Verify this DLL initialized, then tail-call its dependency's ordinal export.
            pe.bytes[0x220..0x229].copy_from_slice(&[
                0x80, 0x3d, 0x49, 0, 0, 0, 73, // cmp byte [initialized], 73
                0x75, 6, // jne return_zero
            ]);
            pe.branch_through(0x229, slots[0], false);
            pe.bytes[0x22f..0x232].copy_from_slice(&[0x31, 0xc0, 0xc3]);
        }
        if matches!(kind, Image::ExitingDll) {
            // A process exit is observable even if the main title entry never runs.
            pe.bytes[0x200..0x209].copy_from_slice(&[
                0x48, 0x83, 0xec, 0x28, // aligned Win64 shadow space
                0xb9, 73, 0, 0, 0, // mov ecx, 73
            ]);
            pe.branch_through(0x209, *slots.last().unwrap(), true);
            pe.bytes[0x20f..0x211].copy_from_slice(&[0x0f, 0x0b]);
        }
        let table = pe.reserve(40, 4);
        let function = pe.reserve(4, 4);
        // Distinct names also make equal-behavior DLLs distinct content-addressed images.
        let name = pe.text(path.file_name().unwrap().to_str().unwrap());
        pe.u32(table + 12, Pe::rva(name));
        pe.u32(table + 16, 1);
        pe.u32(table + 20, 1);
        pe.u32(table + 28, Pe::rva(function));
        pe.u32(function, 0x1020);
        pe.directory(0, table, pe.next - table);
    }
    let base = if dll { 0x1_9000_0000 } else { 0x1_8000_0000 };
    let relocations = pe.reserve(12, 4);
    let pointer = pe.reserve(8, 8);
    pe.u32(relocations, 0x1000);
    pe.u32(relocations + 4, 12);
    pe.u16(relocations + 8, 0xa000 | (Pe::rva(pointer) & 0xfff) as u16);
    pe.u64(pointer, base);
    pe.directory(5, relocations, 12);

    let file_size = pe.next.next_multiple_of(0x200);
    let section_size = (file_size - 0x200) as u32;
    pe.bytes[..2].copy_from_slice(b"MZ");
    pe.u32(0x3c, 0x80);
    pe.bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    pe.u16(0x84, 0x8664);
    pe.u16(0x86, 1);
    pe.u16(0x94, 0xf0);
    pe.u16(0x96, if dll { 0x2022 } else { 0x0022 });
    let optional = 0x98;
    pe.u16(optional, 0x20b);
    pe.u32(optional + 4, section_size);
    pe.u32(optional + 16, 0x1000);
    pe.u32(optional + 20, 0x1000);
    pe.u64(optional + 24, base);
    pe.u32(optional + 32, 0x1000);
    pe.u32(optional + 36, 0x200);
    pe.u16(optional + 40, 6);
    pe.u16(optional + 48, 6);
    pe.u32(
        optional + 56,
        (0x1000 + section_size).next_multiple_of(0x1000),
    );
    pe.u32(optional + 60, 0x200);
    pe.u16(optional + 68, 3);
    pe.u16(optional + 70, 0x0140);
    pe.u64(optional + 72, 0x10_0000);
    pe.u64(optional + 80, 0x1000);
    pe.u64(optional + 88, 0x10_0000);
    pe.u64(optional + 96, 0x1000);
    pe.u32(optional + 108, 16);
    let section = 0x188;
    pe.bytes[section..section + 6].copy_from_slice(b".probe");
    pe.u32(section + 8, section_size);
    pe.u32(section + 12, 0x1000);
    pe.u32(section + 16, section_size);
    pe.u32(section + 20, 0x200);
    pe.u32(section + 36, 0xe000_0020);
    pe.bytes.truncate(file_size);
    std::fs::write(path, pe.bytes).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
    title: std::path::PathBuf,
}

impl Fixture {
    fn new(kind: Image, ordinary: &[&str], delayed: &[&str]) -> Self {
        let root = tempfile::tempdir().unwrap();
        leonida_core::ensure_outside_repo(root.path()).unwrap();
        let title = root.path().join("probe.exe");
        write_image(&title, kind, ordinary, delayed);
        Self { root, title }
    }

    fn dll(&self, name: &str, kind: Image, ordinary: &[&str], delayed: &[&str]) {
        write_image(&self.root.path().join(name), kind, ordinary, delayed);
    }

    fn run(&self, arguments: &[&str]) -> Output {
        run_title(self.root.path(), &self.title, arguments)
    }
}

fn assert_refused(output: &Output, missing: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stderr.contains(missing), "{stdout}\n{stderr}");
    assert!(
        stderr.contains("missing from the game folder"),
        "{stdout}\n{stderr}"
    );
    assert!(!stdout.contains("bound          "), "{stdout}\n{stderr}");
    assert!(!stdout.contains("title exited with"), "{stdout}\n{stderr}");
}

fn assert_title_ran(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("title exited with 73"),
        "{stdout}\n{stderr}"
    );
}

#[test]
fn missing_transitive_static_dll_is_refused_before_initialization() {
    let fixture = Fixture::new(Image::Title, &["u6-a.dll"], &[]);
    fixture.dll("u6-a.dll", Image::ExitingDll, &["u6-b.dll"], &[]);
    let output = fixture.run(&["--execute"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(code) = output.status.code() {
        eprintln!("U6_STARTUP_CHILD_EXIT={code}");
    }
    assert_ne!(
        output.status.code(),
        Some(73),
        "missing u6-b.dll was allowed to reach A's DllMain (ExitProcess sentinel 73)\n{stdout}\n{stderr}",
    );
    assert_refused(&output, "u6-b.dll");
    eprintln!("U6_STATIC_PREFLIGHT_REFUSED_BEFORE_DLLMAIN");
}

#[test]
fn complete_static_chain_initializes_dlls_and_runs_title() {
    let fixture = Fixture::new(Image::TitleCallingFirst, &["u6-a.dll"], &[]);
    fixture.dll("u6-a.dll", Image::DllCallingFirst, &["u6-b.dll"], &[]);
    fixture.dll("u6-b.dll", Image::Dll, &[], &[]);
    assert_title_ran(&fixture.run(&["--execute"]));
    eprintln!("U6_COMPLETE_STATIC_CHAIN_EXECUTED");
}

#[test]
fn deeper_missing_static_dll_is_named_once_across_case_aliases() {
    let fixture = Fixture::new(Image::Title, &["U6-A.DLL", "u6-a.dll"], &[]);
    fixture.dll("u6-a.dll", Image::Dll, &["u6-c.dll", "U6-C.DLL"], &[]);
    fixture.dll(
        "u6-c.dll",
        Image::ExitingDll,
        &["u6-b.dll", "U6-B.DLL"],
        &[],
    );
    let output = fixture.run(&["--execute"]);
    assert_refused(&output, "u6-b.dll");
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    assert_eq!(stderr.matches("u6-b.dll").count(), 1, "{stderr}");
}

#[test]
fn static_cycles_and_shared_dependencies_finish_startup() {
    let fixture = Fixture::new(Image::Title, &["u6-a.dll", "u6-c.dll", "U6-A.DLL"], &[]);
    fixture.dll("u6-a.dll", Image::Dll, &["u6-b.dll"], &[]);
    fixture.dll("u6-b.dll", Image::Dll, &["U6-A.DLL"], &[]);
    fixture.dll("u6-c.dll", Image::Dll, &["u6-b.dll"], &[]);
    assert_title_ran(&fixture.run(&["--execute"]));
}

#[test]
fn missing_direct_delay_import_stays_optional() {
    let fixture = Fixture::new(Image::Title, &[], &["u6-b.dll"]);
    let output = fixture.run(&["--execute"]);
    assert_title_ran(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing      u6-b.dll!"), "{stdout}");
}

#[test]
fn present_delay_only_dll_can_have_an_absent_static_dependency() {
    let fixture = Fixture::new(Image::Title, &[], &["u6-a.dll"]);
    fixture.dll("u6-a.dll", Image::Dll, &["u6-b.dll"], &[]);
    let output = fixture.run(&["--execute"]);
    assert_title_ran(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("title dll      u6-a.dll"), "{stdout}");
    assert!(stdout.contains("bound          u6-a.dll"), "{stdout}");
    assert!(stdout.contains("missing      u6-b.dll!"), "{stdout}");
    eprintln!("U6_DELAY_ONLY_DEPENDENCY_REMAINED_OPTIONAL");
}

#[test]
fn a_delay_path_does_not_hide_a_required_static_path() {
    let fixture = Fixture::new(Image::Title, &["u6-a.dll"], &["u6-c.dll"]);
    fixture.dll("u6-a.dll", Image::Dll, &["u6-c.dll"], &[]);
    fixture.dll("u6-c.dll", Image::ExitingDll, &["u6-b.dll"], &[]);
    assert_refused(&fixture.run(&["--execute"]), "u6-b.dll");
}

#[test]
fn explicit_zero_policy_still_allows_missing_static_dependencies() {
    let fixture = Fixture::new(Image::Title, &["u6-a.dll"], &[]);
    fixture.dll("u6-a.dll", Image::ExitingDll, &["u6-b.dll"], &[]);
    let output = fixture.run(&["--execute", "--unimplemented", "zero"]);
    assert_eq!(
        output.status.code(),
        Some(73),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("U6_ZERO_POLICY_REACHED_DLLMAIN");
}

#[test]
fn inspection_reports_missing_static_dependencies_without_initializing_dlls() {
    let fixture = Fixture::new(Image::Title, &["u6-a.dll"], &[]);
    fixture.dll("u6-a.dll", Image::ExitingDll, &["u6-b.dll"], &[]);
    let output = fixture.run(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("u6-b.dll"), "{stdout}\n{stderr}");
    assert!(stdout.contains("missing dlls   "), "{stdout}\n{stderr}");
    assert!(stdout.contains("not executing"), "{stdout}\n{stderr}");
    assert!(!stdout.contains("title exited with"), "{stdout}\n{stderr}");
}

#[test]
fn host_and_hle_imports_do_not_require_title_files() {
    let fixture = Fixture::new(Image::Title, &["KERNEL32.dll", "xgameruntime.dll"], &[]);
    assert_title_ran(&fixture.run(&["--execute"]));
}
