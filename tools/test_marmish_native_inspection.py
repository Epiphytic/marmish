"""Offline dependency/ABI refusal vectors; these do not qualify real target binaries."""
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/v02-inspect-native.sh'
PRODUCTS = ('marmish', 'marmishd', 'marmish-worker')
SYSTEM = '/usr/lib/libSystem.B.dylib'
IOKIT = '/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit'
OBSERVED = (SYSTEM, '/usr/lib/libiconv.2.dylib',
    '/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation',
    '/System/Library/Frameworks/Security.framework/Versions/A/Security',
    '/System/Library/Frameworks/SystemConfiguration.framework/Versions/A/SystemConfiguration', IOKIT)


class NativeInspection(unittest.TestCase):
    def run_case(self, platform='Linux', library='libc.so.6', abi='2.39', minimum='15.0',
                 imports=None, per_binary=None, full=False):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = {
                'uname': f"case \"$1\" in -m) echo x86_64 ;; *) echo '{platform}' ;; esac",
                'readelf': f'''case "$1" in
-d) printf '%s\\n' ' (NEEDED) Shared library: [{library}]' ;;
-VW) printf '%s\\n' 'Name: GLIBC_{abi}' ;;
esac''',
            }
            cases = []
            for binary in PRODUCTS:
                selected = (per_binary or {}).get(binary, {})
                selected_min = selected.get('minimum', minimum)
                lines = ['cmd LC_BUILD_VERSION']
                if selected_min is not None:
                    lines.append('minos ' + selected_min)
                deps = selected.get('imports', imports if imports is not None else [SYSTEM])
                listing = ['fake:'] + [' ' + dep + ' (compatibility version 1.0.0)' for dep in deps]
                quote = lambda values: ' '.join(shlex.quote(v) for v in values)
                cases.append(f'''*/{binary}) case "$1" in
-l) printf '%s\\n' {quote(lines)} ;;
-L) printf '%s\\n' {quote(listing)} ;;
esac ;;''')
            tools['otool'] = 'case "$2" in\n' + '\n'.join(cases) + '\nesac'
            for name, body in tools.items():
                path = root / name
                path.write_text('#!/bin/sh\n' + body + '\n')
                path.chmod(0o755)
            result = subprocess.run(['/bin/sh', str(SCRIPT), str(root)],
                env={'PATH': str(root) + ':/usr/bin:/bin'},
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
            return result if full else result.returncode

    def test_qualified_linux_and_macos_vectors(self):
        self.assertEqual(self.run_case(), 0)
        self.assertEqual(self.run_case(platform='Darwin'), 0)

    def test_actual_six_import_list_on_all_products(self):
        result = self.run_case(platform='Darwin', imports=OBSERVED, full=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for binary in PRODUCTS:
            self.assertIn((binary + ' macOS minimum 15.0').encode(), result.stdout)

    def test_macho_rejects_external_and_near_match_imports_with_attribution(self):
        for dependency in ('/opt/homebrew/lib/libssl.3.dylib',
                IOKIT.replace('/A/', '/B/'), IOKIT + '/x', '@rpath/IOKit',
                IOKIT.replace('IOKit.framework', 'Other.framework')):
            with self.subTest(dependency=dependency):
                result = self.run_case(platform='Darwin', imports=OBSERVED,
                    per_binary={'marmishd': {'imports': (*OBSERVED, dependency)}}, full=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(('marmishd unqualified Mach-O dependency: ' + dependency).encode(), result.stderr)
                self.assertNotIn(b'marmishd macOS minimum', result.stdout)

    def test_macho_diagnostic_sanitizes_and_bounds_untrusted_name(self):
        dependency = '::error::' + '\x1b' + 'x' * 300
        result = self.run_case(platform='Darwin',
            per_binary={'marmishd': {'imports': [SYSTEM, dependency]}}, full=True)
        self.assertNotEqual(result.returncode, 0)
        prefix = b'native inspection: marmishd unqualified Mach-O dependency: '
        self.assertTrue(result.stderr.startswith(prefix))
        display = result.stderr[len(prefix):].rstrip(b'\n')
        self.assertEqual(len(display), 240)
        self.assertNotIn(b':', display)
        self.assertNotIn(b'\x1b', display)

    def test_macho_missing_libsystem_and_minimum_have_fixed_attribution(self):
        for override, message in (({'imports': [IOKIT]}, 'missing Mach-O libSystem dependency'),
                ({'minimum': None}, 'unexpected Mach-O minimum'),
                ({'minimum': '11.0'}, 'unexpected Mach-O minimum')):
            with self.subTest(override=override):
                result = self.run_case(platform='Darwin', imports=OBSERVED,
                    per_binary={'marmishd': override}, full=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stderr, ('native inspection: marmishd ' + message + '\n').encode())
                self.assertNotIn(b'marmishd macOS minimum', result.stdout)

    def test_refuses_unqualified_linux_dependency_or_abi(self):
        self.assertNotEqual(self.run_case(library='libssl.so.3'), 0)
        self.assertNotEqual(self.run_case(abi='2.40'), 0)
        self.assertNotEqual(self.run_case(abi='2.40.1'), 0)
        self.assertNotEqual(self.run_case(abi='PRIVATE'), 0)

    def test_refuses_macos_minimum_mismatch(self):
        self.assertNotEqual(self.run_case(platform='Darwin', minimum='11.0'), 0)


if __name__ == '__main__':
    unittest.main()
