#!/usr/bin/env python3
"""Verify a release APK's integrity, signature, ABI, and native page alignment."""
import argparse
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import zipfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--abi', required=True, choices=['arm64-v8a', 'armeabi-v7a', 'x86_64'])
parser.add_argument('apk', type=Path)
args = parser.parse_args()
sdk = Path(os.environ.get('ANDROID_SDK_ROOT') or os.environ['ANDROID_HOME'])
versions = [p for p in (sdk / 'build-tools').iterdir() if (p / 'apksigner').is_file()]
build_tools = max(versions, key=lambda p: tuple(int(x) for x in p.name.split('.') if x.isdigit()))
subprocess.run([str(build_tools / 'apksigner'), 'verify', '--verbose', str(args.apk)], check=True)
page_kb = '4' if args.abi == 'armeabi-v7a' else '16'
subprocess.run([str(build_tools / 'zipalign'), '-c', '-P', page_kb, '4', str(args.apk)], check=True)
with zipfile.ZipFile(args.apk) as archive:
    assert archive.testzip() is None, 'APK contains corrupt ZIP entries'
    names = archive.namelist()
    assert len(names) == len(set(names)), 'APK contains duplicate ZIP entries'
    libs = [name for name in names if name.startswith('lib/') and name.endswith('.so')]
    assert libs and {name.split('/')[1] for name in libs} == {args.abi}, 'APK ABI mismatch'
    for name in libs:
        data = archive.read(name)
        assert data[:4] == b'\x7fELF' and data[5] == 1, f'Invalid ELF: {name}'
        machine = struct.unpack_from('<H', data, 18)[0]
        assert machine == {'arm64-v8a': 183, 'armeabi-v7a': 40, 'x86_64': 62}[args.abi], name
        if data[4] == 2:
            offset = struct.unpack_from('<Q', data, 32)[0]
            entry_size, count = struct.unpack_from('<HH', data, 54)
            for index in range(count):
                entry = offset + index * entry_size
                if struct.unpack_from('<I', data, entry)[0] == 1:
                    alignment = struct.unpack_from('<Q', data, entry + 48)[0]
                    assert alignment >= 16384, f'ELF lacks 16 KiB alignment: {name}'
with args.apk.open("rb") as apk_file:
    digest = hashlib.file_digest(apk_file, "sha256").hexdigest()
print(f"{args.apk.name}: {args.apk.stat().st_size} bytes; SHA256 {digest}")
