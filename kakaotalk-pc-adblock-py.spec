# -*- mode: python ; coding: utf-8 -*-


import os
import sys
a = Analysis(
    ['kakaotalk_layout_adblock_v11.py'],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=[],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)

# Python 3.15 abi3t: cryptography _rust.pyd links python3t.dll
# (<prefix>\abi3t-compat), which PyInstaller does not auto-collect.
# Without it the frozen exe dies with "DLL load failed while importing _rust".
_py3t_dll = os.path.join(sys.base_prefix, 'abi3t-compat', 'python3t.dll')
_py3t_dest = 'python3t.dll'
if os.path.isfile(_py3t_dll) and not any(
        (b[0] or '').lower() == _py3t_dest for b in a.binaries):
    a.binaries.append((_py3t_dest, _py3t_dll, 'BINARY'))
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='kakaotalk-pc-adblock-py',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
