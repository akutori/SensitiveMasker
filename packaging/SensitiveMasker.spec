# -*- mode: python ; coding: utf-8 -*-
# Build from the project root:
#   uv run pyinstaller packaging/SensitiveMasker.spec --distpath dist --workpath build --noconfirm
from PyInstaller.utils.hooks import collect_submodules

a = Analysis(
    ['run_gui.py'],
    pathex=['src'],
    binaries=[],
    datas=[
        ('../assets/icon.ico', 'assets'),
    ],
    hiddenimports=collect_submodules('masking_core') + collect_submodules('gui') + ['tkinter'],
    hookspath=[],
    runtime_hooks=[],
    excludes=['cli'],
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
    name='SensitiveMasker',
    console=False,
    icon='../assets/icon.ico',
)
