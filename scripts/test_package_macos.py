"""Fail-closed release checks; no Apple or GitHub credentials required."""

import argparse
import contextlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import package_macos


class ReleaseSafetyTests(unittest.TestCase):
    def test_rejected_notarization_preserves_submission_and_diagnostics(self):
        result = subprocess.CompletedProcess([], 1, '{"id":"rejected-id","status":"Invalid"}', '')
        with tempfile.TemporaryDirectory() as directory:
            logs = Path(directory)
            with patch.object(package_macos.subprocess, 'run', return_value=result), \
                    patch.object(package_macos, 'run', return_value='{"issues":["bad signature"]}'):
                with self.assertRaisesRegex(RuntimeError, 'not accepted: Invalid'):
                    package_macos.notarize(Path('release.dmg'), [], logs)
            self.assertEqual(json.loads((logs / 'release.dmg.notary.json').read_text())['id'], 'rejected-id')
            self.assertTrue((logs / 'release.dmg.notary-log.json').exists())

    def test_timeout_does_not_count_as_success_or_lose_submission_id(self):
        result = subprocess.CompletedProcess([], 69, '{"id":"pending-id","status":"In Progress"}', '')
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(package_macos.subprocess, 'run', return_value=result):
                with self.assertRaisesRegex(RuntimeError, 'not accepted: In Progress'):
                    package_macos.notarize(Path('release.dmg'), [], Path(directory))
            self.assertIn('pending-id', (Path(directory) / 'release.dmg.notary.json').read_text())

    def test_existing_artifact_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'release.dmg'
            output.write_bytes(b'original release')
            with patch.dict(os.environ, {'MACOS_SIGNING_IDENTITY': 'identity', 'APPLE_TEAM_ID': 'team'}):
                with self.assertRaisesRegex(RuntimeError, 'Refusing to overwrite'):
                    package_macos.package(argparse.Namespace(output=output))
            self.assertEqual(output.read_bytes(), b'original release')

    def test_missing_credentials_does_not_produce_an_unsigned_artifact(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'MACOS_SIGNING_IDENTITY'):
                package_macos.package(argparse.Namespace())

    def test_existing_zip_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release.dmg"
            output.with_suffix(".zip").write_bytes(b"existing")
            with patch.dict(os.environ, {"MACOS_SIGNING_IDENTITY": "identity", "APPLE_TEAM_ID": "team"}):
                with self.assertRaisesRegex(RuntimeError, "Refusing to overwrite"):
                    package_macos.package(argparse.Namespace(output=output))
            self.assertFalse(output.exists())

    def test_partial_ci_credentials_fail_before_packaging(self):
        with patch.dict(os.environ, {"MACOS_SIGNING_IDENTITY": "identity", "APPLE_TEAM_ID": "team",
                                   "MACOS_CERTIFICATE_P12_BASE64": "YQ=="}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "MACOS_CERTIFICATE_PASSWORD"):
                package_macos.check_credentials()

    def test_rejected_app_never_creates_distributable_archives(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release.dmg"
            entitlements = Path(directory) / "entitlements.plist"
            entitlements.write_bytes(package_macos.plistlib.dumps({}))
            args = argparse.Namespace(output=output, app=Path(directory) / "input.app",
                                      architecture="arm64", entitlements=entitlements)
            details = "TeamIdentifier=team\nAuthority=Developer ID Application: test\nTimestamp=now\nflags=0x10000(runtime)"
            with patch.dict(os.environ, {"MACOS_SIGNING_IDENTITY": "identity", "APPLE_TEAM_ID": "team",
                                       "APPLE_NOTARY_PROFILE": "test"}, clear=True), \
                    patch.object(package_macos, "audit_bundle", return_value=([], {"CFBundleExecutable": "app"})), \
                    patch.object(package_macos, "credentials", return_value=contextlib.nullcontext(([], []))), \
                    patch.object(package_macos, "run", return_value=details) as run, \
                    patch.object(package_macos, "notarize", side_effect=RuntimeError("not accepted")):
                with self.assertRaisesRegex(RuntimeError, "not accepted"):
                    package_macos.package(args)
                self.assertFalse(any(call.args[:2] == ("hdiutil", "create") for call in run.call_args_list))
            self.assertFalse(output.exists())
            self.assertFalse(output.with_suffix(".zip").exists())


if __name__ == '__main__':
    unittest.main()
