"""Exercise packaged helper bootstrap and discovery contracts offline."""

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
                self.assertIsNone(
                    SIDECARS.provider_smoke_key_answer({**prompt, field: value}, key)
                )
        self.assertEqual(
            SIDECARS.provider_smoke_key_answer(prompt, key),
            {"type": "extension_ui_response", "id": "key-prompt", "value": key},
        )

    def test_environment_never_carries_the_key_and_rejects_an_offered_key(self):
        key = "private-smoke-fixture"
        arguments = (Path("runtime"), Path("private"), Path("result.json"), key)
        with patch.object(
            SIDECARS, "pi_environment", return_value={"PATH": "/fixture"}
        ):
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
                    with self.assertRaisesRegex(
                        RuntimeError, "must not enter the environment"
                    ) as raised:
                        SIDECARS.provider_smoke_environment(*arguments)
                    self.assertNotIn(key, str(raised.exception))


class OpenBBInventorySmokeTest(unittest.TestCase):
    executable = Path("openbb/loupist-openbb")
    runtime = executable.parent
    manifest = {
        "allowed_categories": ["equity", "regulators"],
        "providers": [
            {
                "id": "sec",
                "keyless": True,
                "config_mapping": {"contact_email": "sec_contact_email"},
                "credential_mapping": {},
                "network_hosts": ["www.sec.gov"],
            },
            {
                "id": "tradier",
                "keyless": False,
                "config_mapping": {"account_type": "tradier_account_type"},
                "credential_mapping": {"api_key": "tradier_api_key"},
                "network_hosts": ["api.tradier.com"],
            },
            {
                "id": "fred",
                "keyless": False,
                "credential_mapping": {"api_key": "fred_api_key"},
                "network_hosts": ["api.stlouisfed.org"],
            },
            {
                "id": "nasdaq",
                "keyless": True,
                "credential_mapping": {"api_key": "nasdaq_api_key"},
                "network_hosts": ["www.nasdaq.com"],
            },
        ],
    }
    configured = {
        "filings": {"sec"},
        "quote": {"tradier"},
        "series": {"fred"},
        "search": {"nasdaq"},
    }
    unconfigured = {"search": {"nasdaq"}}

    def test_full_inventory_and_empty_setup_use_distinct_bootstraps(self):
        with (
            patch.object(
                SIDECARS, "validate_openbb_bundle", return_value=self.manifest
            ),
            patch.object(
                SIDECARS,
                "_openbb_discovery",
                side_effect=[self.configured, self.unconfigured],
            ) as discovery,
        ):
            SIDECARS.openbb_smoke(self.executable, self.runtime)
        full, empty = discovery.call_args_list
        _, _, credentials, settings = full.args
        self.assertEqual(
            credentials,
            dict.fromkeys(
                ["fred_api_key", "nasdaq_api_key", "tradier_api_key"],
                "loupist-package-smoke-fake-credential",
            ),
        )
        self.assertEqual(
            settings,
            {
                "allowed_categories": ["equity", "regulators"],
                "enabled_provider_ids": ["fred", "nasdaq", "sec", "tradier"],
                "allowed_network_hosts": [
                    "api.stlouisfed.org",
                    "api.tradier.com",
                    "www.nasdaq.com",
                    "www.sec.gov",
                ],
                "provider_config": {
                    "sec": {"contact_email": "package-smoke@example.invalid"},
                    "tradier": {"account_type": "sandbox"},
                },
            },
        )
        self.assertEqual(full.kwargs["expected_categories"], {"equity", "regulators"})
        self.assertEqual(empty.args[2], {})
        self.assertEqual(
            empty.args[3],
            {
                "allowed_categories": ["equity", "regulators"],
                "enabled_provider_ids": ["fred", "nasdaq"],
                "allowed_network_hosts": ["api.stlouisfed.org", "www.nasdaq.com"],
                "provider_config": {},
            },
        )

    def test_empty_setup_rejects_protected_providers_and_sec_only_tools(self):
        for unexpected, message in [
            ({"series": {"fred"}}, "keyless provider discovery differs"),
            ({"filings": set()}, "exposed SEC-only Tools without setup"),
        ]:
            with (
                self.subTest(unexpected=unexpected),
                patch.object(
                    SIDECARS, "validate_openbb_bundle", return_value=self.manifest
                ),
                patch.object(
                    SIDECARS,
                    "_openbb_discovery",
                    side_effect=[self.configured, self.unconfigured | unexpected],
                ),
            ):
                with self.assertRaisesRegex(RuntimeError, message):
                    SIDECARS.openbb_smoke(self.executable, self.runtime)


if __name__ == "__main__":
    unittest.main()
