from __future__ import annotations

import os
import sys
from dataclasses import dataclass
from pathlib import Path

VERSION = "11.1.3"
APP_NAME = "KakaoTalk Layout AdBlocker"
APPDATA_DIRNAME = "KakaoTalkAdBlockerLayout"
# The public half of the Ed25519 release-signing key.  Only the matching
# private key is stored in GitHub Actions as KAKAO_UPDATE_PRIVATE_KEY_B64.
UPDATE_PUBLIC_KEY_B64 = "Cix9d2r5UZxpDL4Bp9CWNrjMDRTQHF5Y1snTMYnMQ2U="


@dataclass(frozen=True)
class RuntimePaths:
    appdata_dir: str
    settings_file: str
    rules_file: str
    log_file: str


def resource_base_dir() -> str:
    if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
        return str(getattr(sys, "_MEIPASS"))
    return str(Path(__file__).resolve().parents[2])


def _default_appdata_dir() -> str:
    appdata = os.environ.get("APPDATA")
    if not appdata:
        appdata = str(Path.home() / "AppData" / "Roaming")
    return str(Path(appdata) / APPDATA_DIRNAME)


def _build_runtime_paths(appdata_dir: str) -> RuntimePaths:
    return RuntimePaths(
        appdata_dir=appdata_dir,
        settings_file=os.path.join(appdata_dir, "layout_settings_v11.json"),
        rules_file=os.path.join(appdata_dir, "layout_rules_v11.json"),
        log_file=os.path.join(appdata_dir, "layout_adblock.log"),
    )
