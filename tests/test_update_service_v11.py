from __future__ import annotations

import base64
import hashlib
import json
from datetime import datetime, timedelta, timezone

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from kakao_adblocker.config.paths import RuntimePaths, VERSION
from kakao_adblocker.services import NoUpdateAvailable, StagedUpdate, UpdateError, UpdateManifest, UpdateService


def _bump_patch(version: str) -> str:
    parts = [int(part) for part in version.split(".")]
    parts[-1] += 1
    return ".".join(str(part) for part in parts)


def _document(version: str | None = None) -> tuple[str, bytes]:
    if version is None:
        version = _bump_patch(VERSION)
    key = Ed25519PrivateKey.generate()
    public = base64.b64encode(key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)).decode()
    tag = f"v{version}"
    payload = {
        "version": version,
        "tag": tag,
        "artifact_url": f"{UpdateService.RELEASE_DOWNLOAD_PREFIX}{tag}/KakaoTalkLayoutAdBlocker_v11.exe",
        "sha256": "a" * 64,
        "size": 123,
        "expires_at": (datetime.now(timezone.utc) + timedelta(days=1)).isoformat(),
    }
    signature = base64.b64encode(key.sign(UpdateService._canonical_payload(payload))).decode()
    return public, json.dumps({"payload": payload, "signature": signature}).encode()


class _Response:
    def __init__(self, payload: bytes):
        self.payload, self.offset = payload, 0

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self, size: int) -> bytes:
        value = self.payload[self.offset : self.offset + size]
        self.offset += len(value)
        return value

    def geturl(self) -> str:
        return "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/latest/download/update.json"


def test_newer_manifest_fixture_stays_ahead_of_package_version():
    newer = _bump_patch(VERSION)
    assert UpdateService._is_newer(newer, VERSION)
    assert not UpdateService._is_newer(VERSION, VERSION)
    assert not UpdateService._is_newer(VERSION, newer)


def test_check_for_update_accepts_signed_newer_manifest(monkeypatch):
    newer = _bump_patch(VERSION)
    public, document = _document(newer)
    monkeypatch.setattr("kakao_adblocker.services.UPDATE_PUBLIC_KEY_B64", public)
    monkeypatch.setattr("kakao_adblocker.services.urlopen", lambda *_args, **_kwargs: _Response(document))
    manifest = UpdateService.check_for_update()
    assert manifest.version == newer
    assert manifest.tag == f"v{newer}"


def test_check_for_update_rejects_tampered_manifest(monkeypatch):
    newer = _bump_patch(VERSION)
    public, document = _document(newer)
    tampered = _bump_patch(newer)
    monkeypatch.setattr("kakao_adblocker.services.UPDATE_PUBLIC_KEY_B64", public)
    monkeypatch.setattr(
        "kakao_adblocker.services.urlopen",
        lambda *_args, **_kwargs: _Response(document.replace(newer.encode(), tampered.encode())),
    )
    with pytest.raises(UpdateError, match="서명"):
        UpdateService.check_for_update()


def test_check_for_update_reports_current_version(monkeypatch):
    public, document = _document(VERSION)
    monkeypatch.setattr("kakao_adblocker.services.UPDATE_PUBLIC_KEY_B64", public)
    monkeypatch.setattr("kakao_adblocker.services.urlopen", lambda *_args, **_kwargs: _Response(document))
    with pytest.raises(NoUpdateAvailable):
        UpdateService.check_for_update()


def test_download_update_verifies_bytes_and_uses_unique_staging_path(monkeypatch, tmp_path):
    payload = b"verified update"
    manifest = UpdateManifest("11.0.1", "v11.0.1", f"{UpdateService.RELEASE_DOWNLOAD_PREFIX}v11.0.1/KakaoTalkLayoutAdBlocker_v11.exe", hashlib.sha256(payload).hexdigest(), len(payload), datetime.now(timezone.utc) + timedelta(days=1))
    paths = RuntimePaths(str(tmp_path), str(tmp_path / "s.json"), str(tmp_path / "r.json"), str(tmp_path / "l.log"))
    monkeypatch.setattr("kakao_adblocker.services.get_runtime_paths", lambda create=False: paths)
    monkeypatch.setattr("kakao_adblocker.services.urlopen", lambda *_args, **_kwargs: _Response(payload))
    monkeypatch.setattr("kakao_adblocker.services.sys.frozen", True, raising=False)
    staged = UpdateService.download_update(manifest)
    assert staged.path.read_bytes() == payload
    assert staged.path.parent.name == "updates"


def test_launch_installer_embeds_reverification_and_result_path(monkeypatch, tmp_path):
    staged_file = tmp_path / "stage.exe"
    staged_file.write_bytes(b"x")
    manifest = UpdateManifest("11.0.1", "v11.0.1", "https://example.invalid/file.exe", "a" * 64, 1, datetime.now(timezone.utc) + timedelta(days=1))
    paths = RuntimePaths(str(tmp_path), str(tmp_path / "s.json"), str(tmp_path / "r.json"), str(tmp_path / "l.log"))
    captured: dict[str, list[str]] = {}
    monkeypatch.setattr("kakao_adblocker.services.get_runtime_paths", lambda create=False: paths)
    monkeypatch.setattr("kakao_adblocker.services.sys.frozen", True, raising=False)
    monkeypatch.setattr("kakao_adblocker.services.sys.executable", str(staged_file))
    monkeypatch.setattr("kakao_adblocker.services.subprocess.Popen", lambda args, **_kwargs: captured.setdefault("args", args))
    UpdateService.launch_installer(StagedUpdate(manifest, staged_file))
    script = base64.b64decode(captured["args"][-1]).decode("utf-16le")
    assert "Get-FileHash -Algorithm SHA256" in script
    assert "last-update-result.json" in script


def test_consume_install_result_removes_result_file(monkeypatch, tmp_path):
    paths = RuntimePaths(str(tmp_path), str(tmp_path / "s.json"), str(tmp_path / "r.json"), str(tmp_path / "l.log"))
    monkeypatch.setattr("kakao_adblocker.services.get_runtime_paths", lambda create=False: paths)
    path = UpdateService.result_path()
    path.write_text('{"status":"failed","error":"locked"}', encoding="utf-8")
    assert UpdateService.consume_install_result() == {"status": "failed", "error": "locked"}
    assert not path.exists()
