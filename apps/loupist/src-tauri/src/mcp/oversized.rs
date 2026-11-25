use super::McpError;
use serde_json::{Map, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

// Enough for any envelope field and the host's 32-digit ID, even with each
// character written as a JSON Unicode escape.
const MAX_FIELD_BYTES: usize = 256;

/// Drain through the newline and read only the top-level response envelope.
/// JSON members can occur in any order, including the ID after the payload.
pub(super) async fn drain<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    prefix: Vec<u8>,
) -> Result<Option<String>, McpError> {
    let mut envelope = Envelope::default();
    envelope.scan(&prefix);
    drop(prefix);
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Err(McpError::Protocol);
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            envelope.scan(&available[..newline]);
            reader.consume(newline + 1);
            return Ok(envelope.response_id());
        }
        envelope.scan(available);
        let count = available.len();
        reader.consume(count);
    }
}

#[derive(Default)]
struct Envelope {
    fields: Map<String, Value>,
    member: Vec<u8>,
    colon: Option<usize>,
    depth: usize,
    quoted: bool,
    escaped: bool,
    complete: bool,
    invalid: bool,
}

impl Envelope {
    fn scan(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.invalid {
                return;
            }
            if self.quoted {
                if self.escaped {
                    self.escaped = false;
                } else if byte == b'\\' {
                    self.escaped = true;
                } else if byte == b'"' {
                    self.quoted = false;
                }
            } else {
                match byte {
                    b' ' | b'\t' | b'\r' => continue,
                    b'{' if self.depth == 0 && !self.complete => {
                        self.depth = 1;
                        continue;
                    }
                    _ if self.depth == 0 => {
                        self.invalid = true;
                        return;
                    }
                    b':' if self.depth == 1 && self.colon.is_none() => {
                        self.colon = Some(self.member.len());
                        continue;
                    }
                    b',' if self.depth == 1 => {
                        self.finish_member();
                        continue;
                    }
                    b'}' if self.depth == 1 => {
                        self.finish_member();
                        self.complete = true;
                        self.depth = 0;
                        continue;
                    }
                    b']' if self.depth == 1 => {
                        self.invalid = true;
                        return;
                    }
                    b'{' | b'[' => self.depth += 1,
                    b'}' | b']' => self.depth -= 1,
                    b'"' => self.quoted = true,
                    _ => {}
                }
            }
            if self.member.len() <= MAX_FIELD_BYTES {
                self.member.push(byte);
            }
        }
    }

    fn finish_member(&mut self) {
        if let Some(colon) = self.colon.take() {
            if let Ok(key) = serde_json::from_slice::<String>(&self.member[..colon]) {
                let value = match key.as_str() {
                    "id" | "jsonrpc" => serde_json::from_slice::<Value>(&self.member[colon..]).ok(),
                    "method" | "result" | "error" if self.member.len() > colon => Some(Value::Null),
                    _ => None,
                };
                if let Some(value) = value {
                    self.invalid |= self.fields.insert(key, value).is_some();
                }
            }
        }
        self.member.clear();
    }

    fn response_id(mut self) -> Option<String> {
        if self.invalid
            || !self.complete
            || self.fields.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || self.fields.contains_key("method")
            || self.fields.contains_key("result") == self.fields.contains_key("error")
        {
            return None;
        }
        match self.fields.remove("id")? {
            Value::String(id) => Some(id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn draining_tracks_only_top_level_reply_ids_and_keeps_the_next_line() {
        for (end, expected) in [
            (br#"}, "\u0069d" : "reply" }"#.as_slice(), Some("reply")),
            (br#"}, "id":"ping", "method":"ping"}"#.as_slice(), None),
            (br#"}}"#.as_slice(), None),
        ] {
            let mut prefix = br#"{"jsonrpc":"2.0","result":{"id":"nested","text":""#.to_vec();
            prefix.extend_from_slice(&[b'x'; MAX_FIELD_BYTES * 2]);
            // An escaped quote at the chunk boundary cannot end the string.
            prefix.push(b'\\');
            let tail = [br#"",\"id\":\"decoy\"""#.as_slice(), end, b"\nnext\n"].concat();
            let mut reader = tail.as_slice();
            assert_eq!(
                drain(&mut reader, prefix).await.unwrap().as_deref(),
                expected
            );
            assert_eq!(reader, b"next\n");
        }
    }
}
