from __future__ import annotations

import csv
import ctypes
import base64
import hashlib
import io
import json
import os
import shlex
import subprocess
import sys
import threading
import time
import webbrowser
from ctypes import wintypes
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional, Set, Tuple, cast
from urllib.parse import urlsplit
from urllib.request import Request, urlopen
from uuid import uuid4

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from .config import UPDATE_PUBLIC_KEY_B64, VERSION, get_runtime_paths

try:
    import psutil as _psutil
except Exception:
    _psutil = None
PSUTIL_AVAILABLE = _psutil is not None
psutil: Any = _psutil

try:
    import winreg as _winreg
except Exception:
    _winreg = None
WINREG_AVAILABLE = _winreg is not None
winreg: Any = _winreg


class ProcessInspector:
    _warning_lock = threading.Lock()
    _last_warning = ""

    @staticmethod
    def _set_warning(message: str) -> None:
        with ProcessInspector._warning_lock:
            ProcessInspector._last_warning = message or ""

    @staticmethod
    def consume_last_warning() -> str:
        with ProcessInspector._warning_lock:
            message = ProcessInspector._last_warning
            ProcessInspector._last_warning = ""
            return message

    @staticmethod
    def _normalize_image_name(image_name: str) -> str:
        name = (image_name or "").strip().lower()
        if not name:
            return ""
        return name if name.endswith(".exe") else f"{name}.exe"

    @staticmethod
    def get_process_ids(image_name: str = "kakaotalk.exe") -> Set[int]:
        normalized = ProcessInspector._normalize_image_name(image_name)
        if not normalized:
            ProcessInspector._set_warning("")
            return set()

        pids: Set[int] = set()
        warning_messages: list[str] = []
        psutil_mod: Any = psutil
        if psutil_mod is not None:
            try:
                proc_iter = psutil_mod.process_iter(["pid", "name"])
            except Exception as exc:
                proc_iter = None
                warning_messages.append(f"psutil init failed ({exc.__class__.__name__})")
            if proc_iter is not None:
                try:
                    for proc in proc_iter:
                        try:
                            proc_info = proc.info or {}
                            proc_name = (proc_info.get("name") or "").strip().lower()
                            if proc_name == normalized:
                                pids.add(int(proc_info["pid"]))
                        except Exception:
                            continue
                    ProcessInspector._set_warning("")
                    return pids
                except Exception as exc:
                    # Fall through to tasklist fallback on psutil loop failure.
                    warning_messages.append(f"psutil loop failed ({exc.__class__.__name__})")
            if warning_messages:
                warning_messages.append("using tasklist fallback")

        try:
            result = subprocess.run(
                ["tasklist", "/FI", f"IMAGENAME eq {normalized}", "/FO", "CSV", "/NH"],
                capture_output=True,
                text=True,
                creationflags=0x08000000,
                timeout=3,
            )
            if result.returncode != 0:
                warning_messages.append(f"tasklist returncode={result.returncode}")
            parser = csv.reader(io.StringIO(result.stdout))
            for row in parser:
                if len(row) < 2:
                    continue
                image = row[0].strip().lower()
                if image != normalized:
                    continue
                try:
                    pids.add(int(row[1]))
                except Exception:
                    continue
        except Exception as exc:
            warning_messages.append(f"tasklist failed ({exc.__class__.__name__})")

        ProcessInspector._set_warning("; ".join(warning_messages) if warning_messages else "")
        return pids

    @staticmethod
    def probe_tasklist() -> Tuple[bool, str]:
        try:
            result = subprocess.run(
                ["tasklist", "/FO", "CSV", "/NH"],
                capture_output=True,
                text=True,
                creationflags=0x08000000,
                timeout=3,
            )
            if result.returncode != 0:
                return False, f"tasklist returncode={result.returncode}"
            return True, "tasklist 실행 가능"
        except Exception as exc:
            return False, f"{exc.__class__.__name__}: {exc}"


class StartupManager:
    KEY = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
    NAME = "KakaoTalkAdBlockerLayout"
    PACKAGED_EXE_NAME = "KakaoTalkLayoutAdBlocker_v11.exe"
    STARTUP_ARGS = ("--startup-launch", "--minimized")

    @staticmethod
    def build_command() -> str:
        if getattr(sys, "frozen", False):
            return f'"{sys.executable}" --startup-launch --minimized'
        script = Path(sys.argv[0]).resolve()
        return f'"{sys.executable}" "{script}" --startup-launch --minimized'

    @staticmethod
    def get_registered_command() -> Optional[str]:
        winreg_mod: Any = winreg
        if winreg_mod is None:
            return None
        try:
            key = winreg_mod.OpenKey(
                winreg_mod.HKEY_CURRENT_USER,
                StartupManager.KEY,
                0,
                winreg_mod.KEY_READ,
            )
            try:
                value, _reg_type = winreg_mod.QueryValueEx(key, StartupManager.NAME)
                return str(value or "")
            finally:
                winreg_mod.CloseKey(key)
        except Exception:
            return None

    @staticmethod
    def _split_command_windows(command: str) -> list[str]:
        if os.name != "nt" or not command:
            return []
        try:
            shell32 = ctypes.WinDLL("shell32", use_last_error=True)
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            argc = ctypes.c_int(0)
            shell32.CommandLineToArgvW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(ctypes.c_int)]
            shell32.CommandLineToArgvW.restype = ctypes.POINTER(wintypes.LPWSTR)
            kernel32.LocalFree.argtypes = [ctypes.c_void_p]
            kernel32.LocalFree.restype = wintypes.HANDLE
            argv = shell32.CommandLineToArgvW(command, ctypes.byref(argc))
            if not argv:
                return []
            try:
                return [argv[i] for i in range(max(argc.value, 0))]
            finally:
                kernel32.LocalFree(ctypes.cast(argv, ctypes.c_void_p))
        except Exception:
            return []

    @staticmethod
    def _split_command(command: str) -> list[str]:
        if not command:
            return []
        windows_tokens = StartupManager._split_command_windows(command)
        if windows_tokens:
            return windows_tokens
        try:
            tokens = shlex.split(command, posix=False)
        except Exception:
            return []
        return [token[1:-1] if len(token) >= 2 and token[0] == token[-1] == '"' else token for token in tokens]

    @staticmethod
    def _expand_environment_strings(value: str) -> str:
        if not value:
            return value
        if os.name == "nt":
            try:
                kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
                kernel32.ExpandEnvironmentStringsW.argtypes = [wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD]
                kernel32.ExpandEnvironmentStringsW.restype = wintypes.DWORD
                required = int(kernel32.ExpandEnvironmentStringsW(value, None, 0))
                if required > 0:
                    buf = ctypes.create_unicode_buffer(required)
                    written = int(kernel32.ExpandEnvironmentStringsW(value, buf, required))
                    if written > 0:
                        return buf.value
            except Exception:
                pass
        return os.path.expandvars(value)

    @staticmethod
    def _command_target_paths(command: str) -> list[str]:
        tokens = StartupManager._split_command(command)
        if not tokens:
            return []
        targets = [StartupManager._expand_environment_strings(tokens[0])]
        for token in tokens[1:]:
            normalized = token.lower()
            if normalized.startswith("-") or normalized.startswith("/"):
                break
            targets.append(StartupManager._expand_environment_strings(token))
            if len(targets) >= 2:
                break
        return targets

    @staticmethod
    def _is_source_mode_compatible_packaged_command(command: str) -> bool:
        if getattr(sys, "frozen", False):
            return False
        tokens = StartupManager._split_command(command)
        if not tokens:
            return False
        target = Path(StartupManager._expand_environment_strings(tokens[0]))
        if target.name.lower() != StartupManager.PACKAGED_EXE_NAME.lower():
            return False
        if not target.exists():
            return False
        return tuple(tokens[1:]) == StartupManager.STARTUP_ARGS

    @staticmethod
    def _is_managed_registration_command(command: str, expected: str) -> bool:
        tokens = StartupManager._split_command(command)
        expected_tokens = StartupManager._split_command(expected)
        if not tokens:
            return False
        first_target = Path(StartupManager._expand_environment_strings(tokens[0]))
        if first_target.name.lower() == StartupManager.PACKAGED_EXE_NAME.lower():
            return True
        if expected_tokens:
            expected_first = Path(StartupManager._expand_environment_strings(expected_tokens[0]))
            if first_target.name.lower() == expected_first.name.lower():
                return True
        if len(tokens) >= 2 and expected_tokens and len(expected_tokens) >= 2:
            second_target = Path(StartupManager._expand_environment_strings(tokens[1]))
            expected_second = Path(StartupManager._expand_environment_strings(expected_tokens[1]))
            if second_target.name.lower() == expected_second.name.lower():
                return True
        return False

    @staticmethod
    def registration_health() -> tuple[str, str]:
        command = StartupManager.get_registered_command()
        if not command:
            return "not_registered", "Run 등록 안 됨"
        expected = StartupManager.build_command()
        if command == expected:
            return "healthy", "Run 등록 명령 정상"
        if StartupManager._is_source_mode_compatible_packaged_command(command):
            return "healthy", "Run 등록 명령 정상(packaged EXE)"
        if not StartupManager._is_managed_registration_command(command, expected):
            return "custom_command", "custom command left unchanged"
        targets = StartupManager._command_target_paths(command)
        missing_targets = [target for target in targets if target and not Path(target).exists()]
        if missing_targets:
            missing_text = ", ".join(missing_targets)
            return "missing_target", f"Run 등록 대상 누락: {missing_text}"
        return "stale_command", "Run 등록 명령 불일치"

    @staticmethod
    def probe_registration_command() -> Tuple[bool, str]:
        status, detail = StartupManager.registration_health()
        return status in {"healthy", "not_registered", "custom_command"}, detail

    @staticmethod
    def sync_registration_command() -> bool:
        winreg_mod: Any = winreg
        if winreg_mod is None:
            return False
        expected = StartupManager.build_command()
        current = StartupManager.get_registered_command()
        if current == expected or (
            current is not None and StartupManager._is_source_mode_compatible_packaged_command(current)
        ):
            return True
        if current is not None and not StartupManager._is_managed_registration_command(current, expected):
            return True
        try:
            key = winreg_mod.OpenKey(
                winreg_mod.HKEY_CURRENT_USER,
                StartupManager.KEY,
                0,
                winreg_mod.KEY_SET_VALUE,
            )
            try:
                winreg_mod.SetValueEx(key, StartupManager.NAME, 0, winreg_mod.REG_SZ, expected)
            finally:
                winreg_mod.CloseKey(key)
            return True
        except Exception:
            return False

    @staticmethod
    def wait_for_shell_ready(timeout_seconds: float = 15.0, poll_interval_seconds: float = 0.5) -> bool:
        if os.name != "nt":
            return False
        try:
            user32 = ctypes.WinDLL("user32", use_last_error=True)
            find_window = user32.FindWindowW
            find_window.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
            find_window.restype = wintypes.HWND
        except Exception:
            return False

        deadline = time.monotonic() + max(timeout_seconds, 0.0)
        stable_hits = 0
        required_hits = 2
        while time.monotonic() < deadline:
            explorer_ready = bool(ProcessInspector.get_process_ids("explorer.exe"))
            tray_ready = bool(find_window("Shell_TrayWnd", None))
            if explorer_ready and tray_ready:
                stable_hits += 1
                if stable_hits >= required_hits:
                    return True
            else:
                stable_hits = 0
            time.sleep(max(poll_interval_seconds, 0.05))
        return bool(ProcessInspector.get_process_ids("explorer.exe")) and bool(find_window("Shell_TrayWnd", None))

    @staticmethod
    def is_enabled() -> bool:
        winreg_mod: Any = winreg
        if winreg_mod is None:
            return False
        try:
            key = winreg_mod.OpenKey(
                winreg_mod.HKEY_CURRENT_USER,
                StartupManager.KEY,
                0,
                winreg_mod.KEY_READ,
            )
            try:
                winreg_mod.QueryValueEx(key, StartupManager.NAME)
                return True
            finally:
                winreg_mod.CloseKey(key)
        except Exception:
            return False

    @staticmethod
    def set_enabled(enable: bool) -> bool:
        winreg_mod: Any = winreg
        if winreg_mod is None:
            return False
        try:
            key = winreg_mod.OpenKey(
                winreg_mod.HKEY_CURRENT_USER,
                StartupManager.KEY,
                0,
                winreg_mod.KEY_SET_VALUE,
            )
            try:
                if enable:
                    cmd = StartupManager.build_command()
                    winreg_mod.SetValueEx(key, StartupManager.NAME, 0, winreg_mod.REG_SZ, cmd)
                else:
                    try:
                        winreg_mod.DeleteValue(key, StartupManager.NAME)
                    except FileNotFoundError:
                        pass
            finally:
                winreg_mod.CloseKey(key)
            return True
        except Exception:
            return False

    @staticmethod
    def probe_access() -> Tuple[bool, str]:
        winreg_mod: Any = winreg
        if winreg_mod is None:
            return False, "winreg unavailable"
        try:
            key = winreg_mod.OpenKey(
                winreg_mod.HKEY_CURRENT_USER,
                StartupManager.KEY,
                0,
                winreg_mod.KEY_READ,
            )
            try:
                pass
            finally:
                winreg_mod.CloseKey(key)
        except Exception as exc:
            return False, f"read failed ({exc.__class__.__name__}: {exc})"
        try:
            key = winreg_mod.OpenKey(
                winreg_mod.HKEY_CURRENT_USER,
                StartupManager.KEY,
                0,
                winreg_mod.KEY_SET_VALUE,
            )
            try:
                return True, "Run 레지스트리 읽기/쓰기 가능"
            finally:
                winreg_mod.CloseKey(key)
        except Exception as exc:
            return False, f"write failed ({exc.__class__.__name__}: {exc})"


class ShellService:
    @staticmethod
    def open_folder(path: str) -> bool:
        try:
            os.makedirs(path, exist_ok=True)
            if os.name == "nt":
                os.startfile(path)  # type: ignore[attr-defined]
            else:
                webbrowser.open(f"file://{path}")
            return True
        except Exception:
            return False

    @staticmethod
    def open_url(url: str) -> bool:
        try:
            if os.name == "nt":
                subprocess.Popen(
                    ["rundll32", "url.dll,FileProtocolHandler", url],
                    creationflags=0x08000000,
                )
            else:
                webbrowser.open(url)
            return True
        except Exception:
            return False


class ReleaseService:
    RELEASES_URL = "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases"

    @staticmethod
    def open_releases_page() -> bool:
        return ShellService.open_url(ReleaseService.RELEASES_URL)


class UpdateError(RuntimeError):
    """A release update could not be verified, downloaded, or installed."""


class NoUpdateAvailable(UpdateError):
    """The signed latest-release manifest is not newer than this application."""


@dataclass(frozen=True)
class UpdateManifest:
    version: str
    tag: str
    artifact_url: str
    sha256: str
    size: int
    expires_at: datetime


@dataclass(frozen=True)
class StagedUpdate:
    manifest: UpdateManifest
    path: Path


class UpdateService:
    """Signed GitHub Releases updater for the frozen Windows executable."""

    MANIFEST_URL = "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/latest/download/update.json"
    MAX_MANIFEST_BYTES = 64 * 1024
    MAX_ARTIFACT_BYTES = 512 * 1024 * 1024
    USER_AGENT = "KakaoTalkLayoutAdBlocker-Updater"
    RELEASE_DOWNLOAD_PREFIX = "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/download/"
    RESULT_FILE_NAME = "last-update-result.json"

    @staticmethod
    def _version_tuple(value: str) -> tuple[int, ...]:
        parts = str(value or "").strip().split(".")
        if not parts or any(not part.isdigit() for part in parts):
            raise UpdateError(f"Invalid update version: {value}")
        return tuple(int(part) for part in parts)

    @classmethod
    def _is_newer(cls, candidate: str, current: str) -> bool:
        left, right = cls._version_tuple(candidate), cls._version_tuple(current)
        width = max(len(left), len(right))
        return left + (0,) * (width - len(left)) > right + (0,) * (width - len(right))

    @staticmethod
    def _canonical_payload(payload: dict[str, object]) -> bytes:
        return json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")

    @classmethod
    def check_for_update(cls) -> UpdateManifest:
        request = Request(cls.MANIFEST_URL, headers={"User-Agent": cls.USER_AGENT})
        try:
            with urlopen(request, timeout=10) as response:
                document = response.read(cls.MAX_MANIFEST_BYTES + 1)
                final_url = str(response.geturl())
        except Exception as exc:
            raise UpdateError(f"업데이트 정보 다운로드 실패: {exc}") from exc
        if not final_url.startswith("https://") or len(document) > cls.MAX_MANIFEST_BYTES:
            raise UpdateError("업데이트 매니페스트가 올바르지 않습니다.")
        try:
            parsed = json.loads(document.decode("utf-8"))
            payload = dict(parsed["payload"])
            signature = base64.b64decode(str(parsed["signature"]), validate=True)
            public_key = base64.b64decode(UPDATE_PUBLIC_KEY_B64, validate=True)
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, cls._canonical_payload(payload))
            version = str(payload["version"])
            tag = str(payload["tag"])
            artifact_url = str(payload["artifact_url"])
            sha256 = str(payload["sha256"]).lower()
            size = int(payload["size"])
            expires_at = datetime.fromisoformat(str(payload["expires_at"]).replace("Z", "+00:00"))
        except (KeyError, TypeError, ValueError, InvalidSignature) as exc:
            raise UpdateError("업데이트 서명 또는 형식 검증에 실패했습니다.") from exc
        if not tag.startswith("v") or tag[1:] != version:
            raise UpdateError("업데이트 태그 정보가 올바르지 않습니다.")
        expected_url = f"{cls.RELEASE_DOWNLOAD_PREFIX}{tag}/KakaoTalkLayoutAdBlocker_v11.exe"
        if artifact_url != expected_url or urlsplit(artifact_url).scheme != "https":
            raise UpdateError("업데이트 파일 위치가 올바르지 않습니다.")
        if len(sha256) != 64 or any(c not in "0123456789abcdef" for c in sha256):
            raise UpdateError("업데이트 파일 정보가 올바르지 않습니다.")
        if size <= 0 or size > cls.MAX_ARTIFACT_BYTES:
            raise UpdateError("업데이트 파일 크기가 허용 범위를 벗어났습니다.")
        if expires_at.tzinfo is None:
            raise UpdateError("업데이트 매니페스트 만료 시각이 올바르지 않습니다.")
        if expires_at <= datetime.now(timezone.utc):
            raise UpdateError("업데이트 매니페스트가 만료되었습니다.")
        if not cls._is_newer(version, VERSION):
            raise NoUpdateAvailable("현재 최신 버전을 사용 중입니다.")
        return UpdateManifest(version, tag, artifact_url, sha256, size, expires_at)

    @classmethod
    def download_update(cls, manifest: UpdateManifest) -> StagedUpdate:
        if not getattr(sys, "frozen", False):
            raise UpdateError("자동 업데이트는 배포된 EXE에서만 사용할 수 있습니다.")
        version, url = manifest.version, manifest.artifact_url
        expected_hash, expected_size = manifest.sha256, manifest.size
        temporary: Path | None = None
        try:
            staging = Path(get_runtime_paths(create=True).appdata_dir) / "updates"
            staging.mkdir(parents=True, exist_ok=True)
            session_id = uuid4().hex
            destination = staging / f"KakaoTalkLayoutAdBlocker_v11-{version}-{session_id}.exe"
            temporary = staging / f".{destination.name}.download"
            digest, total = hashlib.sha256(), 0
            request = Request(url, headers={"User-Agent": cls.USER_AGENT})
            with urlopen(request, timeout=30) as response, temporary.open("wb") as handle:
                if not str(response.geturl()).startswith("https://"):
                    raise UpdateError("업데이트 파일 리디렉션이 안전하지 않습니다.")
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > expected_size or total > cls.MAX_ARTIFACT_BYTES:
                        raise UpdateError("업데이트 파일 크기가 일치하지 않습니다.")
                    digest.update(chunk)
                    handle.write(chunk)
            if total != expected_size or digest.hexdigest() != expected_hash:
                raise UpdateError("업데이트 파일 무결성 검증에 실패했습니다.")
            temporary.replace(destination)
            return StagedUpdate(manifest, destination)
        except Exception as exc:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
            if isinstance(exc, UpdateError):
                raise
            raise UpdateError(f"업데이트 다운로드 실패: {exc}") from exc

    @classmethod
    def result_path(cls) -> Path:
        root = Path(get_runtime_paths(create=True).appdata_dir) / "updates"
        root.mkdir(parents=True, exist_ok=True)
        return root / cls.RESULT_FILE_NAME

    @classmethod
    def consume_install_result(cls) -> dict[str, object] | None:
        path: Path | None = None
        try:
            path = cls.result_path()
            result = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(result, dict) or result.get("status") not in {"applied", "failed", "rolled_back"}:
                return None
            return result
        except (OSError, ValueError, TypeError):
            return None
        finally:
            try:
                if path is not None:
                    path.unlink(missing_ok=True)
            except OSError:
                pass

    @staticmethod
    def _ps_quote(value: str) -> str:
        return value.replace("'", "''")

    @classmethod
    def launch_installer(cls, staged: StagedUpdate) -> None:
        target = Path(sys.executable).resolve()
        if not getattr(sys, "frozen", False) or not target.exists() or not staged.path.is_file():
            raise UpdateError("업데이트 설치 대상을 확인할 수 없습니다.")
        # The helper waits until this process exits, atomically swaps the verified
        # file, restores its backup on failure, then starts the updated EXE.
        script = (
            "$ErrorActionPreference='Stop'; "
            f"$p={os.getpid()}; $t='{cls._ps_quote(str(target))}'; $s='{cls._ps_quote(str(staged.path.resolve()))}'; "
            f"$h='{staged.manifest.sha256}'; $n={staged.manifest.size}; $r='{cls._ps_quote(str(cls.result_path()))}'; $b=$t+'.bak'; "
            "while(Get-Process -Id $p -ErrorAction SilentlyContinue){Start-Sleep -Milliseconds 200}; "
            "try { if((Get-Item -LiteralPath $s).Length -ne $n -or (Get-FileHash -Algorithm SHA256 -LiteralPath $s).Hash.ToLower() -ne $h){throw 'staged update verification failed'}; Remove-Item -LiteralPath $b -Force -ErrorAction SilentlyContinue; Move-Item -LiteralPath $t -Destination $b -Force; Move-Item -LiteralPath $s -Destination $t -Force; Start-Process -FilePath $t; $j=(@{status='applied';version='"
            f"{staged.manifest.version}"
            "'} | ConvertTo-Json -Compress); [IO.File]::WriteAllText($r,$j,[Text.Encoding]::UTF8) } "
            "catch { $status='failed'; if((Test-Path -LiteralPath $b) -and -not (Test-Path -LiteralPath $t)){ Move-Item -LiteralPath $b -Destination $t -Force; $status='rolled_back' }; $j=(@{status=$status;error=$_.Exception.Message} | ConvertTo-Json -Compress); [IO.File]::WriteAllText($r,$j,[Text.Encoding]::UTF8) }"
        )
        encoded = base64.b64encode(script.encode("utf-16le")).decode("ascii")
        try:
            subprocess.Popen(["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", encoded], creationflags=0x08000000)
        except Exception as exc:
            raise UpdateError(f"업데이트 설치 도우미 실행 실패: {exc}") from exc


__all__ = ["ProcessInspector", "StartupManager", "ShellService", "ReleaseService", "UpdateError", "NoUpdateAvailable", "UpdateManifest", "StagedUpdate", "UpdateService"]
