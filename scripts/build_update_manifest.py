"""Build the signed update.json asset uploaded with every GitHub release."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from datetime import datetime, timedelta, timezone
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def canonical_payload(payload: dict[str, object]) -> bytes:
    return json.dumps(
        payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Build a signed GitHub Release update manifest"
    )
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--artifact-url", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--expires-in-days", type=int, default=365)
    args = parser.parse_args(argv)
    if not args.artifact.is_file() or args.artifact.suffix.lower() != ".exe":
        raise ValueError("--artifact must be an existing .exe")
    if not args.artifact_url.startswith("https://"):
        raise ValueError("--artifact-url must use HTTPS")
    if args.tag != f"v{args.version.removeprefix('v')}":
        raise ValueError("--tag must be v followed by --version")
    if args.expires_in_days <= 0:
        raise ValueError("--expires-in-days must be positive")
    private_b64 = os.environ.get("KAKAO_UPDATE_PRIVATE_KEY_B64", "").strip()
    if not private_b64:
        raise ValueError("KAKAO_UPDATE_PRIVATE_KEY_B64 is not set")
    private_key = Ed25519PrivateKey.from_private_bytes(
        base64.b64decode(private_b64, validate=True)
    )
    digest = hashlib.sha256(args.artifact.read_bytes()).hexdigest()
    payload: dict[str, object] = {
        "version": args.version.removeprefix("v"),
        "tag": args.tag,
        "artifact_url": args.artifact_url,
        "sha256": digest,
        "size": args.artifact.stat().st_size,
        "expires_at": (
            datetime.now(timezone.utc) + timedelta(days=args.expires_in_days)
        )
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
    }
    document = {
        "payload": payload,
        "signature": base64.b64encode(
            private_key.sign(canonical_payload(payload))
        ).decode("ascii"),
    }
    args.output.write_text(
        json.dumps(document, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
