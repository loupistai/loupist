//! Availability of optional user extension surfaces in this build.

#[cfg(not(test))]
const USER_SKILLS_IN_RELEASE: bool = false;
#[cfg(not(test))]
const CUSTOM_CONNECTIONS_IN_RELEASE: bool = false;

pub(crate) fn user_skills() -> bool {
    #[cfg(test)]
    {
        TEST_FLAGS.with(|flags| flags.get().0)
    }
    #[cfg(not(test))]
    {
        #[cfg(debug_assertions)]
        {
            static DEV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *DEV.get_or_init(|| std::env::var("LOUPIST_DEV_USER_SKILLS").as_deref() == Ok("1")) {
                return true;
            }
        }
        USER_SKILLS_IN_RELEASE
    }
}

pub(crate) fn custom_connections() -> bool {
    #[cfg(test)]
    {
        TEST_FLAGS.with(|flags| flags.get().1)
    }
    #[cfg(not(test))]
    {
        #[cfg(debug_assertions)]
        {
            static DEV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *DEV.get_or_init(|| {
                std::env::var("LOUPIST_DEV_CUSTOM_CONNECTIONS").as_deref() == Ok("1")
            }) {
                return true;
            }
        }
        CUSTOM_CONNECTIONS_IN_RELEASE
    }
}

pub(crate) fn require_user_skills() -> Result<(), crate::app::CommandError> {
    require(
        user_skills(),
        crate::app::CommandErrorCode::SkillUnavailable,
    )
}

pub(crate) fn require_custom_connections() -> Result<(), crate::app::CommandError> {
    require(
        custom_connections(),
        crate::app::CommandErrorCode::ConnectionUnavailable,
    )
}

fn require(
    available: bool,
    code: crate::app::CommandErrorCode,
) -> Result<(), crate::app::CommandError> {
    if available {
        Ok(())
    } else {
        Err(crate::app::CommandError::new(
            code,
            "This feature is not available in this version.",
        ))
    }
}

#[cfg(test)]
thread_local! {
    // Existing tests exercise the complete implementation in isolation from
    // the process environment. OFF tests override it on their test thread.
    static TEST_FLAGS: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((true, true)) };
}

#[cfg(test)]
pub(crate) fn with_test_flags<T>(
    user_skills: bool,
    custom_connections: bool,
    test: impl FnOnce() -> T,
) -> T {
    struct Restore((bool, bool));
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_FLAGS.with(|flags| flags.set(self.0));
        }
    }
    let _restore =
        Restore(TEST_FLAGS.with(|flags| flags.replace((user_skills, custom_connections))));
    test()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_harness::{
        self,
        native_policy::{
            classify, decide, Exposure, NativeDecision, NativeRefusal, NativeRequest, NativeRoots,
            NativeTool, PathClass,
        },
    };

    #[test]
    fn dormant_skills_are_not_discovered_listed_loaded_or_admitted_as_skill_files() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let workspace = base.join("workspace");
        let global = base.join("global");
        let project_skills = workspace.join(".loupist/skills");
        let bundled = base.join("agent/skills");
        let personal = global.join("skills/fixture");
        std::fs::create_dir_all(&personal).unwrap();
        std::fs::create_dir_all(&project_skills).unwrap();
        std::fs::create_dir_all(&bundled).unwrap();
        let contents = "---\nname: fixture\ndescription: A local method\n---\nInstructions.\n";
        let skill = personal.join("SKILL.md");
        std::fs::write(&skill, contents).unwrap();
        std::fs::write(workspace.join("AGENTS.md"), "Workspace instructions.").unwrap();

        with_test_flags(false, true, || {
            let snapshot =
                agent_harness::workspace::workspace_snapshot(&workspace, &global).unwrap();
            assert_eq!(snapshot.instructions.len(), 1);
            assert!(snapshot.skills.is_empty());
            assert!(
                agent_harness::workspace::discover_skills(&global, Some(&workspace))
                    .unwrap()
                    .is_empty()
            );
            assert!(agent_harness::skill_catalog(&[])
                .iter()
                .any(|s| s.id == "valuation"));
            assert!(!agent_harness::skill_catalog(&[])
                .iter()
                .any(|s| s.id == "skill-creator"));
            assert!(agent_harness::run_skill_ids(&["skill-creator".into()]).is_err());
            assert!(agent_harness::bundled_skill_content("skill-creator").is_none());

            let roots = NativeRoots::new(
                &workspace,
                &global,
                &global,
                &workspace.join(".loupist"),
                &root.path().join("scratch"),
                &root.path().join("attachments"),
                None,
            )
            .with_agent_skills(&bundled);
            assert_eq!(classify(&skill, &roots).class, PathClass::AppPrivate);
            assert_eq!(
                classify(&project_skills, &roots).class,
                PathClass::AppPrivate
            );
            assert_eq!(
                decide(
                    &NativeRequest {
                        tool: NativeTool::Read,
                        target: Some(bundled.join("skill-creator/SKILL.md")),
                    },
                    &roots,
                    Exposure::Clean,
                ),
                NativeDecision::Refuse(NativeRefusal::AppPrivate)
            );
            assert_eq!(
                classify(&bundled.join("valuation/SKILL.md"), &roots).class,
                PathClass::BundledSkill
            );
        });
        assert_eq!(std::fs::read_to_string(skill).unwrap(), contents);
        assert_eq!(
            agent_harness::workspace::discover_skills(&global, Some(&workspace))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn dormant_custom_connections_are_not_advertised_even_with_stale_ids() {
        let custom = format!("custom-{}", "1".repeat(32));
        with_test_flags(true, false, || {
            let profile = agent_harness::AgentRuntimeProfile::new(
                "chat",
                true,
                true,
                &[custom.clone(), "world-bank.indicators".into()],
            )
            .unwrap();
            assert_eq!(profile.capability_ids, ["world-bank.indicators"]);
            assert!(!profile
                .components
                .iter()
                .any(|component| component.id == custom));
            assert!(!profile
                .unavailable_connections
                .iter()
                .any(|connection| connection.id == custom));
            assert!(!crate::broker::BrokerError::UntrustedTextPaused
                .to_string()
                .contains("custom connection"));
        });
    }
}
