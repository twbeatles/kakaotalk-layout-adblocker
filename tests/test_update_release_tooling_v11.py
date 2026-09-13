from __future__ import annotations

import base64
import json

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from scripts import build_update_manifest, verify_update_manifest


def test_build_and_verify_update_manifest(monkeypatch, tmp_path):
    private_key = Ed25519PrivateKey.generate()
    private_b64 = base64.b64encode(private_key.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())).decode()
    public_b64 = base64.b64encode(private_key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)).decode()
    artifact = tmp_path / "KakaoTalkLayoutAdBlocker_v11.exe"
    output = tmp_path / "update.json"
    artifact.write_bytes(b"release payload")
    monkeypatch.setenv("KAKAO_UPDATE_PRIVATE_KEY_B64", private_b64)
    monkeypatch.setattr(verify_update_manifest, "UPDATE_PUBLIC_KEY_B64", public_b64)

    assert build_update_manifest.main([
        "--version", "11.0.1", "--tag", "v11.0.1", "--artifact", str(artifact),
        "--artifact-url", "https://github.com/twbeatles/kakaotalk-layout-adblocker/releases/download/v11.0.1/KakaoTalkLayoutAdBlocker_v11.exe",
        "--output", str(output),
    ]) == 0
    assert verify_update_manifest.main(["--manifest", str(output)]) == 0
    assert json.loads(output.read_text(encoding="utf-8"))["payload"]["tag"] == "v11.0.1"
