use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    agent_harness::AgentRuntimeProfile,
    app::{AppState, CommandError},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatSurface {
    Bot,
    Session,
    Scheduled,
    Delegated { handoff_id: String },
}

impl ChatSurface {
    pub(crate) fn resolve(
        state: &AppState,
        loupist_id: &str,
        thread_id: &str,
        interactive: bool,
    ) -> Result<Self, CommandError> {
        if interactive {
            return Ok(if thread_id == super::home::home_thread_id(loupist_id) {
                Self::Bot
            } else {
                Self::Session
            });
        }
        Ok(
            match super::handoff::open_for_chat(state, loupist_id, thread_id)? {
                Some(handoff_id) => Self::Delegated { handoff_id },
                None => Self::Scheduled,
            },
        )
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Bot => "bot",
            Self::Session => "session",
            Self::Scheduled => "scheduled",
            Self::Delegated { .. } => "delegated",
        }
    }

    pub(crate) fn interactive(&self) -> bool {
        matches!(self, Self::Bot | Self::Session)
    }

    pub(crate) fn is_bot(&self) -> bool {
        matches!(self, Self::Bot)
    }

    pub(crate) fn admits_tool(&self, name: &str) -> bool {
        match self {
            Self::Bot => name != "schedule_handoff",
            Self::Session => {
                name != "research_open"
                    && (!name.starts_with("schedule_")
                        || matches!(name, "schedule_list" | "schedule_read" | "schedule_handoff"))
            }
            Self::Scheduled | Self::Delegated { .. } => {
                crate::briefings::policy::allowed_name(name)
            }
        }
    }

    pub(crate) fn restrict(&self, runtime: AgentRuntimeProfile) -> AgentRuntimeProfile {
        let mut runtime = runtime.with_schedule_management(self.interactive());
        runtime
            .core_tool_names
            .retain(|name| self.admits_tool(name));
        for component in &mut runtime.components {
            component.tool_names.retain(|name| self.admits_tool(name));
        }
        runtime
    }

    pub(crate) fn envelope_block(&self) -> Value {
        json!({ "surface": self.name(), "hint": self.hint() })
    }

    fn hint(&self) -> &'static str {
        match self {
            Self::Bot => "This is the Bot, the Loupist's one continuing conversation with the user. Answer directly and briefly from Memory and a few reads. When the user asks for depth or the request needs multi-step work, call research_open to hand it to a research session and tell the user the session title; otherwise offer it.",
            Self::Session => "This is a research session: work in depth and file what you establish. A scheduling request goes to the Bot through schedule_handoff.",
            Self::Scheduled => "This is an unattended scheduled run: only pre-approved tools are available, nobody is present to answer a question, and the result is delivered to the Bot.",
            Self::Delegated { .. } => "This is a research session the Bot handed off: nobody is present to answer a question, so work it through with the tools allowed and finish with a complete answer whose first paragraph can stand alone in the Bot.",
        }
    }

    pub(crate) fn after_turn(&self, state: &AppState, loupist_id: &str) {
        if matches!(self, Self::Session) {
            return;
        }
        let _ = super::home::deliver_finished_runs(state, loupist_id);
        if !self.interactive() {
            // Nobody streamed this turn, so its reply reaches the renderer only here.
            crate::briefings::desktop::changed(loupist_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> AgentRuntimeProfile {
        AgentRuntimeProfile::new("chat", true, true, &[]).unwrap()
    }

    fn tools(runtime: &AgentRuntimeProfile) -> Vec<String> {
        let mut names = runtime.core_tool_names.clone();
        for component in &runtime.components {
            names.extend(component.tool_names.iter().cloned());
        }
        names
    }

    #[test]
    fn only_the_bot_lists_research_open_and_schedule_management() {
        let bot = tools(&ChatSurface::Bot.restrict(runtime()));
        assert!(bot.iter().any(|name| name == "research_open"));
        assert!(bot.iter().any(|name| name == "schedule_create"));
        assert!(!bot.iter().any(|name| name == "schedule_handoff"));

        let session = tools(&ChatSurface::Session.restrict(runtime()));
        assert!(!session.iter().any(|name| name == "research_open"));
        assert!(!session.iter().any(|name| name == "schedule_create"));
        assert!(session.iter().any(|name| name == "schedule_handoff"));

        for unattended in [
            ChatSurface::Scheduled,
            ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            },
        ] {
            let names = tools(&unattended.restrict(runtime()));
            assert!(!names.iter().any(|name| name == "research_open"));
            assert!(!names.iter().any(|name| name.starts_with("schedule_")));
            assert!(names.iter().any(|name| name == "lens_read"));
        }
    }

    #[test]
    fn the_rendered_envelope_carries_chat_surface_with_memory_off_and_on() {
        for (surface, use_memory) in [
            (ChatSurface::Bot, true),
            (ChatSurface::Session, true),
            (ChatSurface::Scheduled, true),
            (ChatSurface::Bot, false),
            (
                ChatSurface::Delegated {
                    handoff_id: "handoff-a".into(),
                },
                false,
            ),
        ] {
            let base = crate::agent_harness::turn_envelope_block(
                chrono::Utc::now(),
                use_memory,
                &[],
                None,
                None,
                None,
                None,
                &[],
            )
            .unwrap();
            let rendered = crate::agent_harness::render_turn_context(
                &crate::agent_harness::attach_chat_surface(&base, &surface),
            );
            let value: Value = serde_json::from_str(&rendered).unwrap();
            assert_eq!(value["chat_surface"], surface.envelope_block());
            assert_eq!(value["memory_protocol"].is_object(), use_memory);
        }
    }

    #[test]
    fn every_surface_has_a_fixed_envelope_block() {
        for surface in [
            ChatSurface::Bot,
            ChatSurface::Session,
            ChatSurface::Scheduled,
            ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            },
        ] {
            let block = surface.envelope_block();
            assert_eq!(block["surface"], surface.name());
            assert!(block["hint"].as_str().is_some_and(|hint| !hint.is_empty()));
        }
    }
}
