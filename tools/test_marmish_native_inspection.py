"""Offline dependency/ABI refusal vectors; these do not qualify real target binaries."""
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/v02-inspect-native.sh'


class NativeInspection(unittest.TestCase):
    def run_case(self, platform='Linux', library='libc.so.6', abi='2.39', minimum='15.0'):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = {
                'uname': f"case \"$1\" in -m) echo x86_64 ;; *) echo '{platform}' ;; esac",
                'readelf': f'''case "$1" in
-d) printf '%s\\n' ' (NEEDED) Shared library: [{library}]' ;;
-VW) printf '%s\\n' 'Name: GLIBC_{abi}' ;;
esac''',
                'otool': f'''case "$1" in
-l) printf '%s\\n' 'cmd LC_BUILD_VERSION' 'minos {minimum}' ;;
-L) printf '%s\\n' 'fake:' ' /usr/lib/libSystem.B.dylib (compatibility version 1.0.0)' ;;
esac''',
            }
            for name, body in tools.items():
                path = root / name
                path.write_text('#!/bin/sh\n' + body + '\n')
                path.chmod(0o755)
            return subprocess.run(['/bin/sh', str(SCRIPT), str(root)],
                env={'PATH': str(root) + ':/usr/bin:/bin'},
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5).returncode

    def test_qualified_linux_and_macos_vectors(self):
        self.assertEqual(self.run_case(), 0)
        self.assertEqual(self.run_case(platform='Darwin'), 0)

    def test_refuses_unqualified_linux_dependency_or_abi(self):
        self.assertNotEqual(self.run_case(library='libssl.so.3'), 0)
        self.assertNotEqual(self.run_case(abi='2.40'), 0)
        self.assertNotEqual(self.run_case(abi='2.40.1'), 0)
        self.assertNotEqual(self.run_case(abi='PRIVATE'), 0)

    def test_refuses_macos_minimum_mismatch(self):
        self.assertNotEqual(self.run_case(platform='Darwin', minimum='11.0'), 0)


if __name__ == '__main__':
    unittest.main()
