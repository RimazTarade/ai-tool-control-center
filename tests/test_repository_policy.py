import tempfile
import unittest
from pathlib import Path

from scripts.repository_policy import scan


class RepositoryPolicyTest(unittest.TestCase):
    def test_reports_metadata_without_secret_value(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = "api_" + 'key="example-secret-value"'
            (root / "config.txt").write_text(fixture, encoding="utf-8")
            findings = scan(root)
            self.assertEqual([(Path("config.txt"), "secret assignment", 1)], findings)
            self.assertNotIn("example-secret-value", repr(findings))


if __name__ == "__main__":
    unittest.main()
