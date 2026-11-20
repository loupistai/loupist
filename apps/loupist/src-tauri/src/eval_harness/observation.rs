use super::*;

#[derive(Default)]
pub(super) struct TurnObservation {
    pub pi: trace::PiTrace,
    pub admission: Option<Value>,
    pub terminal: Option<Value>,
}

impl TurnObservation {
    pub fn observe(&mut self, event: &Value) {
        match event["type"].as_str() {
            Some("eval_admission") => {
                let mut event = event.clone();
                event.as_object_mut().unwrap().remove("type");
                self.admission = Some(event);
            }
            Some("eval_terminal") => self.terminal = Some(event["terminal"].clone()),
            _ => self.pi.observe(event),
        }
    }

    pub fn write_projection(&self, record: &mut Value) {
        if let Some(admission) = &self.admission {
            let mut admission = admission.clone();
            for key in ["resolved_appearance", "display_currency"] {
                record[key] = admission
                    .as_object_mut()
                    .unwrap()
                    .remove(key)
                    .unwrap_or_default();
            }
            record["admission"] = admission;
        }
        if let Some(terminal) = &self.terminal {
            record["terminal_event"] = terminal.clone();
        }
    }
}

pub(super) fn label_attempts(record: &mut Value) {
    record["interface_label_calls"] = json!(record["tool_calls"].as_array().map(|calls| {
        calls
            .iter()
            .filter(|call| call["name"] == "interface_labels")
            .count()
    }));
}
