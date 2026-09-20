from pathlib import Path


def read_rust_module(path):
    """Read a whole Rust module: facade file plus its split-directory files.

    Long source files were split into single-responsibility submodules
    (facade `.rs` + `<name>/` directory). Grep-based invariants must see the
    entire module, not just the facade, so future splits keep passing.
    """
    base = Path(path)
    texts = [base.read_text(encoding="utf-8")]
    split_dir = base.with_suffix("")
    if split_dir.is_dir():
        for child in sorted(split_dir.glob("*.rs")):
            texts.append(child.read_text(encoding="utf-8"))
    return "\n".join(texts)


def test_windows_ci_runs_packaged_self_check_in_release_build():
    workflow = Path(".github/workflows/windows-ci.yml").read_text(encoding="utf-8")

    assert "cargo build --release -p kakao-app" in workflow
    assert "kakao-adblock-rs.exe" in workflow
    assert "KakaoTalkLayoutAdBlocker_v11.exe --self-check --json" in workflow


def test_startup_smoke_uses_bounded_wait_in_build_script():
    script = Path("scripts/build_release.ps1").read_text(encoding="utf-8")
    startup_section = script.split("function Invoke-StartupSmoke", 1)[1].split("Push-Location", 1)[0]

    assert "-Wait `" not in startup_section
    assert ".WaitForExit(60000)" in startup_section
    assert "packaged startup smoke timed out" in startup_section


def test_release_workflow_reads_rust_version_and_toolchain():
    workflow = Path(".github/workflows/release.yml").read_text(encoding="utf-8")

    assert "rust/crates/kakao-app/src/config/paths.rs" in workflow
    assert "kakao_adblocker/config/paths.py" not in workflow
    assert "dtolnay/rust-toolchain@stable" in workflow
    assert "PYTHONPATH: legacy/python-v11" in workflow


def test_native_exe_is_windows_gui_and_embeds_app_icon():
    main = Path("rust/crates/kakao-app/src/main.rs").read_text(encoding="utf-8")
    build = Path("rust/crates/kakao-app/build.rs").read_text(encoding="utf-8")
    tray = read_rust_module("rust/crates/kakao-win32/src/tray.rs")

    assert 'windows_subsystem = "windows"' in main
    assert "HWND_MESSAGE" in tray
    assert Path("packaging/app_icon.ico").is_file()
    assert "packaging" in build and "app_icon.ico" in build
    assert "set_icon" in build
    assert "app_icon_resource" in tray
    assert "LoadIconW(None, IDI_APPLICATION)" not in tray.split("fn load_app_icon", 1)[0]


def test_rust_holds_single_instance_mutex_and_resizes_with_nomove():
    app_lib = Path("rust/crates/kakao-app/src/lib.rs").read_text(encoding="utf-8")
    engine = read_rust_module("rust/crates/kakao-app/src/engine.rs")
    mutex = Path("rust/crates/kakao-win32/src/single_instance.rs").read_text(encoding="utf-8")

    assert "let _instance_guard" in app_lib
    assert "InstanceMutex::acquire()" in app_lib
    assert "CloseHandle(self.handle)" in mutex
    apply = engine.split("for pos in &evaluation.actions.set_pos", 1)[1].split("restore_all", 1)[0]
    assert "SWP_NOMOVE" in apply
    assert "is_view_resize" in apply
    assert "if !is_view_resize" in apply
