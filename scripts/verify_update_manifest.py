"""Verify a signed update manifest against the public key embedded in the app."""

from __future__ import annotations

import argparse
import base64
import json
import re
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

ROOT = Path(__file__).resolve().parents[1]
RUST_PATHS_RS = ROOT / "rust" / "crates" / "kakao-app" / "src" / "config" / "paths.rs"


def _read_embedded_pubkey() -> str:
    text = RUST_PATHS_RS.read_text(encoding="utf-8")
    match = re.search(r'pub const UPDATE_PUBLIC_KEY_B64:\s*&str\s*=\s*"([^"]+)"', text)
    if match is None:
        raise ValueError(f"UPDATE_PUBLIC_KEY_B64 not found in {RUST_PATHS_RS}")
    return match.group(1)


UPDATE_PUBLIC_KEY_B64 = _read_embedded_pubkey()


def _canonical_payload(payload: dict[str, object]) -> bytes:
    return json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Verify signed update.json")
    parser.add_argument("--manifest", required=True, type=Path)
    args = parser.parse_args(argv)
    document = json.loads(args.manifest.read_text(encoding="utf-8"))
    payload = dict(document["payload"])
    signature = base64.b64decode(str(document["signature"]), validate=True)
    public = base64.b64decode(UPDATE_PUBLIC_KEY_B64, validate=True)
    Ed25519PublicKey.from_public_bytes(public).verify(signature, _canonical_payload(payload))
    required = {"version", "tag", "artifact_url", "sha256", "size", "expires_at"}
    if set(payload) != required or payload["tag"] != f"v{payload['version']}":
        raise ValueError("Manifest payload fields are invalid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
