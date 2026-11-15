"""Release-name regressions, executed only by the Windows GitHub job."""
import os
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from release_name import make_release, next_release_number, release_metadata, successful_workflow_runs


class ReleaseNameTests(unittest.TestCase):
    def test_matches_excellent_bible_format(self):
        release = make_release("0.3.0", "2026-11-14", "123")
        self.assertEqual(release["release_tag"], "v26-11-14.123-v0.3.0")
        self.assertEqual(release["release_base"], "ExEditor-v26-11-14.123-v0.3.0")

    def test_lifetime_counter_ignores_dates_failed_runs_and_duplicate_records(self):
        history = [{"number": 1, "workflow_run": 100}, {"number": 2, "workflow_run": 101}]
        self.assertEqual(next_release_number(history, [100, 101]), 3)
        self.assertEqual(next_release_number(history, [100, 101, 102, 102]), 4)
        # A failed attempt never enters the completed-success set or changes the ledger.
        self.assertEqual(next_release_number(history, [100, 101]), 3)
        for day in ["2026-11-14", "2026-11-15", "2027-01-01"]:
            self.assertIn(".3-v0.3.0", make_release("0.3.0", day, "3")["release_tag"])

    def test_metadata_only_workflows_do_not_count_as_releases(self):
        responses = [
            {"workflow_runs": [{"id": 100}, {"id": 101}, {"id": 102}]},
            {"jobs": [{"steps": [{"name": "Inspect installer version resources", "conclusion": "success"}]}]},
            {"jobs": [{"steps": [{"name": "Upload installer", "conclusion": "success"}]}]},
        ]
        streams = [io.BytesIO(json.dumps(value).encode()) for value in responses]
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": "example/app", "GITHUB_TOKEN": "fixture"}, clear=True):
            with patch("urllib.request.urlopen", side_effect=streams):
                self.assertEqual(successful_workflow_runs({100}), [102])

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
                "EXEDITOR_RELEASE_TAG": "v26-11-14.123-v0.3.0",
                "EXEDITOR_RELEASE_BASE": "ExEditor-v26-11-14.123-v0.3.0",
            }
            with patch.dict(os.environ, env, clear=True):
                self.assertEqual(release_metadata(root)["version"], "0.3.0")
            env["EXEDITOR_RELEASE_TAG"] = "v26-11-14.124-v0.3.0"
            with patch.dict(os.environ, env, clear=True):
                with self.assertRaises(ValueError):
                    release_metadata(root)


if __name__ == "__main__":
    unittest.main()
