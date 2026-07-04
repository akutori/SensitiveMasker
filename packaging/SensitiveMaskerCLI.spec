# -*- mode: python ; coding: utf-8 -*-
# Build from the project root:
#   uv run pyinstaller packaging/SensitiveMaskerCLI.spec --distpath dist --workpath build --noconfirm
from PyInstaller.utils.hooks import collect_submodules

a = Analysis(
    ['run_cli.py'],
    pathex=['src'],
    binaries=[],
    datas=[],
    hiddenimports=collect_submodules('masking_core') + collect_submodules('cli'),
    hookspath=[],
    runtime_hooks=[],
    excludes=['gui', 'tkinter'],
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name='SensitiveMaskerCLI',
    console=True,
    icon='../assets/icon.ico',
)
