#!/usr/bin/env python3
"""Unit tests for the read-only GitHub release-setup audit."""

from __future__ import annotations

import runpy
import tempfile
import unittest
from unittest import mock
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
AUDIT = runpy.run_path(str(SCRIPTS / "check-github-release-setup.py"))
AuditError = AUDIT["AuditError"]

REPOSITORY = "loupistai/loupist"
UPDATER_KEY, WINDOWS_CERT = (
    "LOUPIST_UPDATER_PUBLIC_KEY",
    "LOUPIST_WINDOWS_CERTIFICATE",
)
ENVIRONMENTS = ("release", "release-qualification", "stable-release")
CHECKS = ("Source and product contracts",)
ACTIONS_APP = 15368
ACTOR = {"actor_id": 1, "actor_type": "User", "bypass_mode": "always"}
V_TAGS = {"ref_name": {"include": ["refs/tags/v*"]}}
CUSTOM = {"protected_branches": False, "custom_branch_policies": True}
ABSENT_FROM_PULL_REQUESTS = (
    "Native macOS interaction",
    "Package smoke (aarch64-apple-darwin)",
    "Rust contracts (macOS)",
    "Native Windows interaction",
    "Package smoke (x86_64-pc-windows-msvc)",
)


def rulesets():
    status = {
        "do_not_enforce_on_create": False,
        "strict_required_status_checks_policy": False,
        "required_status_checks": [
            {"context": name, "integration_id": ACTIONS_APP} for name in CHECKS
        ],
    }
    main_rules = [
        {"type": "pull_request"},
        {"type": "required_status_checks", "parameters": status},
        {"type": "non_fast_forward"},
    ]
    return [
        {
            "target": "branch",
            "enforcement": "active",
            "bypass_actors": [],
            "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"]}},
            "rules": main_rules,
        },
        {
            "target": "tag",
            "enforcement": "active",
            "bypass_actors": [dict(ACTOR)],
            "conditions": V_TAGS,
            "rules": [{"type": "creation"}],
        },
        {
            "target": "tag",
            "enforcement": "active",
            "bypass_actors": [],
            "conditions": V_TAGS,
            "rules": [{"type": "update"}, {"type": "deletion"}],
        },
    ]


def environments():
    reviewer = {
        "type": "required_reviewers",
        "prevent_self_review": True,
        "reviewers": [{"type": "User", "reviewer": {"id": 1, "login": "reviewer"}}],
    }
    return {
        "environments": [
            {
                "name": "release",
                "protection_rules": [{"id": 1}],
                "deployment_branch_policy": dict(CUSTOM),
            },
            {
                "name": "release-qualification",
                "protection_rules": [{"id": 2}],
                "deployment_branch_policy": dict(CUSTOM),
            },
            {
                "name": "stable-release",
                "can_admins_bypass": False,
                "protection_rules": [reviewer],
                "deployment_branch_policy": dict(CUSTOM),
            },
        ]
    }


def policy(name, pattern="v*", **extra):
    return {
        "total_count": 1,
        "branch_policies": [{"id": 1, "node_id": f"policy-{name}", "name": pattern}],
        **extra,
    }


def configured():
    return {
        "repository": REPOSITORY,
        "repository_secrets": set(),
        "environment_secrets": {
            "release": {UPDATER_KEY, WINDOWS_CERT},
            "release-qualification": set(),
            "stable-release": set(),
        },
        "deployment_branch_policies": {name: policy(name) for name in ENVIRONMENTS},
        "required_secrets": {UPDATER_KEY, WINDOWS_CERT},
        "repository_data": {
            "full_name": REPOSITORY,
            "visibility": "public",
            "private": False,
            "default_branch": "main",
        },
        "rulesets": rulesets(),
        "main_legacy_protection": None,
        "immutable_releases": {"enabled": True, "enforced_by_owner": False},
        "environments": environments(),
    }


def main_rule(s):
    return s["rulesets"][0]


def status(s):
    rules = main_rule(s)["rules"]
    return next(r for r in rules if r["type"] == "required_status_checks")["parameters"]


def without(s, rule_type):
    rules = main_rule(s)["rules"]
    main_rule(s)["rules"] = [rule for rule in rules if rule["type"] != rule_type]


def env(s, index):
    return s["environments"]["environments"][index]


def windows_ruleset(context):
    checks = {"required_status_checks": [{"context": context}]}
    rules = [{"type": "required_status_checks", "parameters": checks}]
    return {**rulesets()[0], "rules": rules}


REVIEW = "configure a required reviewer and prevent self-review"
ADMIN = "set can_admins_bypass to false to disallow administrator bypass"
ISOLATION = "workflow-referenced secret names must exist only in release: "

# (label, mutation, error subject, exact detail or None)
WEAKENED = [
    ("main disabled", lambda s: main_rule(s).update(enforcement="disabled"), "main protection", None),
    ("main bypass omitted", lambda s: main_rule(s).pop("bypass_actors"), "main protection", None),
    ("main bypass actor", lambda s: main_rule(s).update(bypass_actors=[ACTOR]), "main protection", None),
    ("no status rule", lambda s: without(s, "required_status_checks"), "main protection", None),
    ("enforced after create only", lambda s: status(s).update(do_not_enforce_on_create=True), "main protection", None),
    ("wrong status source", lambda s: status(s)["required_status_checks"][0].update(integration_id=1), "main protection", None),
    ("missing context", lambda s: status(s)["required_status_checks"].pop(), "main protection", None),
    ("force push allowed", lambda s: without(s, "non_fast_forward"), "main protection", None),
    ("no tag immutability", lambda s: s["rulesets"].pop(2), "release tags", None),
    ("immutability bypass omitted", lambda s: s["rulesets"][2].pop("bypass_actors"), "release tags", None),
    ("immutability exempt actor", lambda s: s["rulesets"][2].update(bypass_actors=[{**ACTOR, "bypass_mode": "exempt"}]), "release tags", None),
    ("immutability malformed bypass", lambda s: s["rulesets"][2].update(bypass_actors={}), "release tags", None),
    ("creator bypass omitted", lambda s: s["rulesets"][1].pop("bypass_actors"), "release tags", None),
    ("creator bypass empty", lambda s: s["rulesets"][1].update(bypass_actors=[]), "release tags", None),
    ("creator wrong mode", lambda s: s["rulesets"][1].update(bypass_actors=[{**ACTOR, "bypass_mode": "pull_request"}]), "release tags", None),
    ("creator malformed actor", lambda s: s["rulesets"][1].update(bypass_actors=[{**ACTOR, "actor_id": True}]), "release tags", None),
    ("creator with immutability", lambda s: s["rulesets"][1].update(rules=[{"type": t} for t in ("creation", "update", "deletion")]), "release tags", None),
    ("second creation rule", lambda s: s["rulesets"].append({**rulesets()[1], "bypass_actors": []}), "release tags", None),
    ("immutable releases off", lambda s: s.update(immutable_releases={"enabled": False, "enforced_by_owner": False}), "immutable releases", None),
    ("immutable releases unknown", lambda s: s.update(immutable_releases=None), "immutable releases", None),
    *[
        (f"{name} branch policy", lambda s, i=i: env(s, i).update(deployment_branch_policy={"protected_branches": True, "custom_branch_policies": False}), f"environment {name}", None)
        for i, name in enumerate(ENVIRONMENTS)
    ],
    ("tag policy missing", lambda s: s["deployment_branch_policies"].pop("release"), "environment release", None),
    ("tag policy listing incomplete", lambda s: s["deployment_branch_policies"]["release"].pop("total_count"), "environment release", None),
    ("tag policy without identity", lambda s: s["deployment_branch_policies"].update(release={"total_count": 1, "branch_policies": [{"name": "v*"}]}), "environment release", None),
    ("tag policy wrong pattern", lambda s: s["deployment_branch_policies"].update(release=policy("release", "release/*")), "environment release", None),
    ("stable without reviewer", lambda s: env(s, 2).update(protection_rules=[{"type": "wait_timer", "wait_timer": 5}]), "environment stable-release", REVIEW),
    ("stable self review", lambda s: env(s, 2)["protection_rules"][0].update(prevent_self_review=False), "environment stable-release", REVIEW),
    ("stable admin bypass", lambda s: env(s, 2).update(can_admins_bypass=True), "environment stable-release", ADMIN),
    ("stable admin bypass unknown", lambda s: env(s, 2).pop("can_admins_bypass"), "environment stable-release", ADMIN),
    ("stable admin bypass ambiguous", lambda s: env(s, 2).update(can_admins_bypass="false"), "environment stable-release", ADMIN),
    ("repository secret", lambda s: s.update(repository_secrets={UPDATER_KEY}), "repository secret isolation", ISOLATION + UPDATER_KEY),
    ("qualification secret", lambda s: s["environment_secrets"].update({"release-qualification": {WINDOWS_CERT}}), "environment release-qualification secret isolation", ISOLATION + WINDOWS_CERT),
    ("stable secret", lambda s: s["environment_secrets"].update({"stable-release": {UPDATER_KEY}}), "environment stable-release secret isolation", ISOLATION + UPDATER_KEY),
    *[
        (f"{source} {context}", lambda s, m=mutation: m(s), "checks absent from pull requests", None)
        for context in ABSENT_FROM_PULL_REQUESTS
        for source, mutation in (
            ("ruleset", lambda s, c=context: s["rulesets"].append(windows_ruleset(c))),
            ("legacy checks", lambda s, c=context: s.update(main_legacy_protection={"required_status_checks": {"checks": [{"context": c}]}})),
            ("legacy contexts", lambda s, c=context: s.update(main_legacy_protection={"required_status_checks": {"contexts": [c]}})),
        )
    ],
]  # fmt: skip


def classic_protection(protection):
    """Replace the main ruleset with classic branch protection."""
    return lambda s: s.update(
        rulesets=s["rulesets"][1:], main_legacy_protection=protection
    )


def errors(findings):
    return {item.subject: item.detail for item in findings if item.level == "error"}


class GitHubReleaseSetupTest(unittest.TestCase):
    def audit(self, mutate=None):
        state = configured()
        if mutate:
            mutate(state)
        return AUDIT["audit_release_setup"](**state)

    def test_configured_repository_passes_every_required_check(self) -> None:
        findings = self.audit()
        self.assertEqual([item.level for item in findings], ["pass"] * 12)
        details = {item.subject: item.detail for item in findings}
        for name in ENVIRONMENTS:
            self.assertIn("GITHUB_REF_TYPE=tag", details[f"environment {name}"])
        # Owner enforcement and classic branch protection are accepted forms.
        owner = {"enabled": False, "enforced_by_owner": True}
        self.assertFalse(
            errors(self.audit(lambda s: s.update(immutable_releases=owner)))
        )

        def strict(s):
            status(s).update(strict_required_status_checks_policy=True)

        self.assertFalse(errors(self.audit(strict)))
        classic = {
            "required_pull_request_reviews": {"required_approving_review_count": 1},
            "required_status_checks": {
                "strict": False,
                "checks": [{"context": c, "app_id": ACTIONS_APP} for c in CHECKS],
            },
        }
        self.assertFalse(errors(self.audit(classic_protection(classic))))

    def test_every_weakened_control_is_an_error(self) -> None:
        for label, mutate, subject, detail in WEAKENED:
            with self.subTest(label):
                found = errors(self.audit(mutate))
                self.assertIn(subject, found)
                if detail:
                    self.assertEqual(found[subject], detail)
        no_pr = classic_protection({"required_status_checks": {"strict": True}})
        self.assertIn("main protection", errors(self.audit(no_pr)))

    def test_missing_setup_lists_every_missing_item(self) -> None:
        def empty(s):
            s.update(rulesets=[], environments={"environments": []})
            s["environment_secrets"]["release"] = set()

        found = errors(self.audit(empty))
        for subject in ("main protection", "release tags", "environment release"):
            self.assertIn(subject, found)
        self.assertEqual(
            found["release environment secrets"],
            f"missing workflow-referenced secret names: {UPDATER_KEY}, {WINDOWS_CERT}",
        )

    def test_secret_names_come_from_the_workflow_without_values(self) -> None:
        release = SCRIPTS.parents[2] / ".github/workflows/release.yml"
        names = AUDIT["required_release_secret_names"](release)
        self.assertFalse(any("WINDOWS" in name for name in names))
        with tempfile.TemporaryDirectory() as directory:
            workflow = Path(directory) / "release.yml"
            workflow.write_text(
                "env:\n  A: ${{ secrets.LOUPIST_ONE }}\n  B: ${{secrets.LOUPIST_TWO}}\n"
            )
            self.assertEqual(
                AUDIT["required_release_secret_names"](workflow),
                {"LOUPIST_ONE", "LOUPIST_TWO"},
            )

    def test_ambiguous_api_listings_fail_closed(self) -> None:
        with self.assertRaises(AuditError):
            AUDIT["repository_secret_names"](
                {"total_count": 101, "secrets": [{"name": "LOUPIST_ONE"}]}
            )
        for summaries in ([{"id": 1}, {"id": 1}], [{"id": True}]):
            with self.assertRaises(AuditError):
                AUDIT["load_ruleset_details"]("read-only-gh", REPOSITORY, summaries)

    def test_remote_audit_reads_only_name_and_policy_endpoints(self) -> None:
        base = f"/repos/{REPOSITORY}"
        listed = [{"id": index + 1, **rule} for index, rule in enumerate(rulesets())]
        secrets = {"release": {"LOUPIST_ONE"}}
        endpoints = {
            base: configured()["repository_data"],
            f"{base}/rulesets?per_page=100&includes_parents=false": [
                {"id": rule["id"]} for rule in listed
            ],
            **{f"{base}/rulesets/{rule['id']}": rule for rule in listed},
            f"{base}/branches/main/protection": None,
            f"{base}/immutable-releases": {"enabled": True, "enforced_by_owner": False},
            f"{base}/environments?per_page=100": environments(),
            f"{base}/actions/secrets?per_page=100": {"total_count": 0, "secrets": []},
        }
        for name in ENVIRONMENTS:
            names = sorted(secrets.get(name, set()))
            endpoints[f"{base}/environments/{name}/secrets?per_page=100"] = {
                "total_count": len(names),
                "secrets": [{"name": item} for item in names],
            }
            endpoints[
                f"{base}/environments/{name}/deployment-branch-policies?per_page=100"
            ] = policy(name)
        requests = []

        def fake_gh_api(gh, endpoint, *, allow_not_found=False):
            self.assertEqual(gh, "read-only-gh")
            requests.append(endpoint)
            return endpoints[endpoint]

        remote_audit = AUDIT["remote_audit"]
        with tempfile.TemporaryDirectory() as directory:
            workflow = Path(directory) / "release.yml"
            workflow.write_text("env:\n  TOKEN: ${{ secrets.LOUPIST_ONE }}\n")
            with mock.patch.dict(remote_audit.__globals__, {"gh_api": fake_gh_api}):
                findings = remote_audit(REPOSITORY, workflow, "read-only-gh")
        self.assertFalse(errors(findings))
        self.assertEqual(set(requests), set(endpoints))


if __name__ == "__main__":
    unittest.main()
