"""Inspect Windows wheels without loading the extension or installing NI software."""

import argparse
from email.parser import BytesParser
from pathlib import Path
import zipfile

import pefile
import tomli


def verify_wheel(path, expected_version, python_version):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        for name in names:
            native = Path(name).name.lower()
            if native.startswith(('nifpga', 'libnifpga')) and native.endswith(
                ('.dll', '.lib', '.a')
            ):
                raise ValueError(f'{path.name}: bundled NI library: {name}')

        modules = [name for name in names if name.endswith('.pyd')]
        if len(modules) != 1:
            raise ValueError(f'{path.name}: expected exactly one Python extension')

        metadata_files = [name for name in names if name.endswith('.dist-info/METADATA')]
        if len(metadata_files) != 1:
            raise ValueError(f'{path.name}: expected exactly one package metadata file')
        metadata = BytesParser().parsebytes(archive.read(metadata_files[0]))
        if metadata['Name'] != 'nifpga-fast-fifo-recv' or metadata['Version'] != expected_version:
            raise ValueError(f'{path.name}: package name or version does not match Cargo.toml')

        python_tag = 'cp' + python_version.replace('.', '')
        if not path.name.endswith(f'-{python_tag}-{python_tag}-win_amd64.whl'):
            raise ValueError(f'{path.name}: expected a {python_tag} Windows x64 wheel')

        with pefile.PE(data=archive.read(modules[0]), fast_load=True) as binary:
            if binary.FILE_HEADER.Machine != 0x8664:
                raise ValueError(f'{path.name}: extension is not Windows x64')
            binary.parse_data_directories(
                directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_IMPORT']]
            )
            ni_imports = {
                symbol.name
                for entry in getattr(binary, 'DIRECTORY_ENTRY_IMPORT', [])
                if entry.dll.lower() == b'nifpga.dll'
                for symbol in entry.imports
            }
            required = {b'NiFpgaDll_Open', b'NiFpgaDll_Close', b'NiFpgaDll_ReadFifoU64'}
            if not required.issubset(ni_imports):
                raise ValueError(f'{path.name}: required dynamic NiFpga.dll imports are missing')

    print(f'OK: {path.name}: NI runtime is external; required DLL imports verified')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--python-version', required=True)
    parser.add_argument('--manifest', type=Path, default=Path('Cargo.toml'))
    args = parser.parse_args()
    version = tomli.loads(args.manifest.read_text(encoding='utf-8'))['package']['version']
    wheels = sorted(args.directory.glob('*.whl'))
    if len(wheels) != 1:
        parser.error(f'expected exactly one wheel in {args.directory}, found {len(wheels)}')
    for wheel in wheels:
        verify_wheel(wheel, version, args.python_version)


if __name__ == '__main__':
    main()
