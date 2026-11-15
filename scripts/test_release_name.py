"""Release-name regressions, executed only by the Windows GitHub job."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from release_name import make_release, release_metadata


class ReleaseNameTests(unittest.TestCase):
    def test_matches_excellent_bible_format(self):
        release = make_release("0.3.0", "2026-11-14", "123")
        self.assertEqual(release["release_tag"], "v2026-11-14.123-v0.3.0")
        self.assertEqual(release["release_base"], "ExEditor-v2026-11-14.123-v0.3.0")

    def test_rejects_invalid_dates_paths_and_numbers(self):
        for version, day, number in [
            ("../other", "2026-11-14", "1"),
            ("0.3.0", "2026-02-31", "1"),
            ("0.3.0", "2026-1-4", "1"),
            ("0.3.0", "2026-11-14", "0"),
            ("0.3.0", "2026-11-14", "-1"),
            ("0.3.0", "2026-11-14", "1/other"),
        ]:
            with self.subTest(version=version, day=day, number=number):
                with self.assertRaises(ValueError):
                    make_release(version, day, number)

    def test_uses_cargo_version_and_shared_job_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text('[package]\nversion = "0.3.0"\n', encoding="utf-8")
            env = {
                "EXEDITOR_RELEASE_DATE": "2026-11-14",
                "EXEDITOR_RELEASE_NUMBER": "123",
                "EXEDITOR_RELEASE_TAG": "v2026-11-14.123-v0.3.0",
                "EXEDITOR_RELEASE_BASE": "ExEditor-v2026-11-14.123-v0.3.0",
            }
            with patch.dict(os.environ, env, clear=True):
                self.assertEqual(release_metadata(root)["version"], "0.3.0")
            env["EXEDITOR_RELEASE_TAG"] = "v2026-11-14.124-v0.3.0"
            with patch.dict(os.environ, env, clear=True):
                with self.assertRaises(ValueError):
                    release_metadata(root)


if __name__ == "__main__":
    unittest.main()
