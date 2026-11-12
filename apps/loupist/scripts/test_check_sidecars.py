"""Exercise the packaged provider smoke's private credential handshake offline."""

import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "check_sidecars", Path(__file__).with_name("check-sidecars.py")
)
SIDECARS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SIDECARS)


class ProviderCredentialSmokeTest(unittest.TestCase):
    def test_key_is_only_an_answer_to_the_expected_input(self):
        key = "private-smoke-fixture"
        prompt = {
            "type": "extension_ui_request",
            "method": "input",
            "title": "loupist-provider/1:api_key",
            "id": "key-prompt",
        }
        for field, value in [
            ("title", "loupist-provider/1:manual_code"),
            ("title", "API key"),
            ("type", "response"),
            ("method", "confirm"),
            ("id", ""),
            ("id", "bad\n"),
            ("id", "x" * 129),
            ("id", None),
        ]:
            with self.subTest(field=field, value=value):
                self.assertIsNone(SIDECARS.provider_smoke_key_answer({**prompt, field: value}, key))
        self.assertEqual(
            SIDECARS.provider_smoke_key_answer(prompt, key),
            {"type": "extension_ui_response", "id": "key-prompt", "value": key},
        )

    def test_environment_never_carries_the_key_and_rejects_an_offered_key(self):
        key = "private-smoke-fixture"
        arguments = (Path("runtime"), Path("private"), Path("result.json"), key)
        with patch.object(SIDECARS, "pi_environment", return_value={"PATH": "/fixture"}):
            environment = SIDECARS.provider_smoke_environment(*arguments)
            self.assertNotIn(key, environment.values())
            self.assertNotIn("LOUPIST_PROVIDER_API_KEY", environment)
        for environment in [
            {"LOUPIST_PROVIDER_API_KEY": key},
            {"OTHER_VARIABLE": key},
            {"LOUPIST_PROVIDER_API_KEY": "even-an-unrelated-value"},
        ]:
            with self.subTest(environment=list(environment)):
                with patch.object(SIDECARS, "pi_environment", return_value=environment):
                    with self.assertRaisesRegex(RuntimeError, "must not enter the environment") as raised:
                        SIDECARS.provider_smoke_environment(*arguments)
                    self.assertNotIn(key, str(raised.exception))


if __name__ == "__main__":
    unittest.main()
