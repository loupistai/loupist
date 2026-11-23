use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{agent_harness::AgentRuntimeProfile, app::AppState};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatSurface {
    Bot,
    Session,
    Scheduled,
    PriceSetup,
    Delegated { handoff_id: String },
}

impl ChatSurface {
    pub(crate) fn resolve(
        loupist_id: &str,
        thread_id: &str,
        interactive: bool,
        handoff_id: Option<&str>,
    ) -> Self {
        if interactive {
            if thread_id == super::home::home_thread_id(loupist_id) {
                Self::Bot
            } else {
                Self::Session
            }
        } else if let Some(handoff_id) = handoff_id {
            Self::Delegated {
                handoff_id: handoff_id.to_owned(),
            }
        } else {
            Self::Scheduled
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Bot => "bot",
            Self::Session => "session",
            Self::Scheduled => "scheduled",
            Self::PriceSetup => "price_setup",
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
            // The Bot keeps its conversation in bot_memory; a working note is a session's research record.
            Self::Bot => !matches!(name, "schedule_handoff" | "note_save" | "schedule_quiet"),
            Self::Session => {
                !matches!(name, "research_open" | "bot_memory")
                    && (!name.starts_with("schedule_")
                        || matches!(name, "schedule_list" | "schedule_read" | "schedule_handoff"))
            }
            Self::Scheduled => crate::briefings::policy::allowed_name(name),
            Self::PriceSetup => {
                matches!(
                    name,
                    "asset_lookup"
                        | "asset_page_read"
                        | "price_read_save"
                        | "dataset_describe"
                        | "dataset_query"
                        | "capability_load"
                ) || crate::broker::ToolMethod::from_tool_name(name).is_some_and(|method| {
                    method
                        .producer()
                        .is_some_and(|producer| producer.retention.keeps_dataset())
                        && crate::briefings::policy::allowed_method(method)
                })
            }
            Self::Delegated { .. } => {
                name != "schedule_quiet" && crate::briefings::policy::allowed_name(name)
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
        // The extension refuses a tool pack with no tools, and capability_load with
        // nothing to load; either ends Pi before the turn starts.
        runtime
            .components
            .retain(|component| component.kind == "mcp" || !component.tool_names.is_empty());
        if runtime.components.is_empty() {
            runtime
                .core_tool_names
                .retain(|name| name != "capability_load");
        }
        runtime
    }

    pub(crate) fn envelope_block(&self) -> Value {
        let mut hint = match self {
            Self::Bot => "This is Bot, the Loupist's one continuing conversation with the user. A quick ask, why an asset moved included, is answered from a read or two and this envelope: read no Skill the user did not choose, file no Lens page and save no Source. A quick ask does save one thing: with saving on, when it reads the price of a followed asset whose row in What the user follows carries price_read, it saves a price read for that asset with price_read_save before answering, because the app shows that asset's price only by repeating a saved read. A file or pasted original material the user hands over, or a request to keep something, is saved and filed as in a session. research_open hands depth to a session that reports back here. With saving on, bot_memory keeps what this conversation should carry forward.",
            Self::Session => "This is a research session: work in depth and file what you establish. A scheduling request goes to Bot through schedule_handoff.",
            Self::Scheduled => "This is an unattended scheduled run: only pre-approved tools are available, nobody is present to answer a question, and the result is delivered to Bot.",
            Self::PriceSetup | Self::Delegated { .. } => return json!({ "surface": self.name() }),
        }.to_owned();
        if matches!(self, Self::Scheduled) {
            hint.push_str(" No user is writing in this run, so the task stands in for the user: the answer's language is the one the task names, or the one the task is written in when it names none. interface.language is the language of the app's screens, which the answer follows only when the task has no wording of its own, as in a briefing. The app's product names are in interface.names.");
        }
        json!({ "surface": self.name(), "hint": hint })
    }

    pub(crate) fn typed_handoff_hint() -> &'static str {
        "This is a research session Bot handed off. The message is what the user wrote to Bot, unchanged; Bot has replied to it there and passed the research here, and handoff.bot_note is Bot's own note on that research. This run cannot ask a question, so work it through with the tools allowed and finish with a complete answer. The user can open the session while it runs, and a further user message is their guidance for this run. Bot previews the answer's first prose paragraph. interface.language is the language of the app's screens, and the app's product names are in interface.names."
    }

    pub(crate) fn link_handoff_hint() -> &'static str {
        "This is a research session opened from a World link in Bot. The message is the host's fixed caption, not words the user wrote, and handoff.map_link is the link it names. This run cannot ask a question, so work it through with the tools allowed and finish with a complete answer. The user can open the session while it runs, and a further user message is their guidance for this run. Bot previews the answer's first prose paragraph. interface.language is the language of the app's screens, which the answer follows here because the user wrote nothing in this run. The app's product names are in interface.names."
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
        assert!(!bot
            .iter()
            .any(|name| name == "schedule_handoff" || name == "schedule_quiet"));
        assert!(bot.iter().any(|name| name == "loupist_setup"));

        let session = tools(&ChatSurface::Session.restrict(runtime()));
        assert!(!session.iter().any(|name| name == "research_open"));
        assert!(!session.iter().any(|name| name == "schedule_create"));
        assert!(session.iter().any(|name| name == "schedule_handoff"));
        assert!(session.iter().any(|name| name == "loupist_setup"));

        for unattended in [
            ChatSurface::Scheduled,
            ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            },
        ] {
            let names = tools(&unattended.restrict(runtime()));
            assert!(!names.iter().any(|name| name == "research_open"));
            assert!(!names.iter().any(|name| name == "loupist_setup"));
            assert!(!names
                .iter()
                .any(|name| name.starts_with("schedule_") && name != "schedule_quiet"));
            assert_eq!(
                names.iter().any(|name| name == "schedule_quiet"),
                matches!(unattended, ChatSurface::Scheduled)
            );
            assert!(names.iter().any(|name| name == "lens_read"));
        }
    }

    #[test]
    fn every_surface_keeps_a_runtime_profile_the_extension_accepts() {
        // The extension exits Pi on a tool pack with no tools, or on capability_load
        // without components (agent/loupist-extension.mjs, registerLoupistExtension).
        let ids: Vec<String> = [
            "community.web-research",
            "defillama.tvl",
            "openbb.platform",
            "openfigi.lookup",
            "world-bank.indicators",
        ]
        .map(String::from)
        .to_vec();
        for surface in [
            ChatSurface::Bot,
            ChatSurface::Session,
            ChatSurface::Scheduled,
            ChatSurface::PriceSetup,
            ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            },
        ] {
            let runtime =
                surface.restrict(AgentRuntimeProfile::new("chat", true, true, &ids).unwrap());
            assert!(
                runtime
                    .components
                    .iter()
                    .all(|component| component.kind == "mcp" || !component.tool_names.is_empty()),
                "{surface:?}"
            );
            assert_eq!(
                !runtime.components.is_empty(),
                runtime
                    .core_tool_names
                    .iter()
                    .any(|name| name == "capability_load"),
                "{surface:?}"
            );
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
                crate::local_time::now(),
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
        assert_eq!(ChatSurface::Bot.envelope_block()["hint"], "This is Bot, the Loupist's one continuing conversation with the user. A quick ask, why an asset moved included, is answered from a read or two and this envelope: read no Skill the user did not choose, file no Lens page and save no Source. A quick ask does save one thing: with saving on, when it reads the price of a followed asset whose row in What the user follows carries price_read, it saves a price read for that asset with price_read_save before answering, because the app shows that asset's price only by repeating a saved read. A file or pasted original material the user hands over, or a request to keep something, is saved and filed as in a session. research_open hands depth to a session that reports back here. With saving on, bot_memory keeps what this conversation should carry forward.");
        for surface in [
            ChatSurface::Bot,
            ChatSurface::Session,
            ChatSurface::Scheduled,
            ChatSurface::PriceSetup,
            ChatSurface::Delegated {
                handoff_id: "handoff-a".into(),
            },
        ] {
            let block = surface.envelope_block();
            assert_eq!(block["surface"], surface.name());
            if matches!(
                surface,
                ChatSurface::PriceSetup | ChatSurface::Delegated { .. }
            ) {
                assert!(block.get("hint").is_none());
                continue;
            }
            let hint = block["hint"].as_str().unwrap();
            assert!(hint.contains("Bot"));
            assert!(!hint.contains("Home"));
            // Scheduled runs use this language rule. Delegated hints are attached with the note.
            assert_eq!(hint.contains("interface.language"), !surface.interactive());
            assert_eq!(
                hint.contains("or the one the task is written in when it names none"),
                !surface.interactive()
            );
        }
    }
}
