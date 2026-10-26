use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::Path,
};

/// Minimal local byte-level BPE decoder for Moonshine's tokenizer.json. Keeping
/// this small avoids pulling a network-capable model-hub/tokenizer dependency
/// into the dictation runtime.
pub struct MoonshineTokenizer {
    vocab: HashMap<u32, String>,
    special_tokens: HashSet<u32>,
}

impl MoonshineTokenizer {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let json: serde_json::Value =
            serde_json::from_reader(BufReader::new(file)).map_err(|error| error.to_string())?;
        let mut vocab = HashMap::new();
        let entries = json
            .get("model")
            .and_then(|model| model.get("vocab"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| "tokenizer model.vocab is missing".to_owned())?;
        for (token, id) in entries {
            let id = id
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or_else(|| format!("invalid token id for {token}"))?;
            vocab.insert(id, token.clone());
        }
        if vocab.is_empty() {
            return Err("tokenizer vocabulary is empty".into());
        }

        let mut special_tokens = HashSet::new();
        if let Some(added) = json.get("added_tokens").and_then(|value| value.as_array()) {
            for token in added {
                if token.get("special").and_then(|value| value.as_bool()) == Some(true)
                    && let Some(id) = token.get("id").and_then(|value| value.as_u64())
                {
                    let id = u32::try_from(id).map_err(|_| "special token id overflow")?;
                    special_tokens.insert(id);
                }
            }
        }
        Ok(Self {
            vocab,
            special_tokens,
        })
    }

    pub fn decode(&self, ids: &[i64]) -> Result<String, String> {
        let mut bytes = Vec::new();
        for id in ids {
            let id = u32::try_from(*id).map_err(|_| "negative decoder token")?;
            if self.special_tokens.contains(&id) {
                continue;
            }
            let token = self
                .vocab
                .get(&id)
                .ok_or_else(|| format!("token id {id} is absent from the vocabulary"))?;
            if let Some(byte) = parse_byte_token(token) {
                bytes.push(byte);
            } else {
                bytes.extend(token.replace('\u{2581}', " ").as_bytes());
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        Ok(text.strip_prefix(' ').unwrap_or(&text).to_owned())
    }
}

fn parse_byte_token(token: &str) -> Option<u8> {
    if token.starts_with("<0x") && token.ends_with('>') && token.len() == 6 {
        u8::from_str_radix(&token[3..5], 16).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenizer() -> MoonshineTokenizer {
        MoonshineTokenizer {
            vocab: HashMap::from([
                (1, "<s>".into()),
                (2, "</s>".into()),
                (3, "▁local".into()),
                (4, "▁speech".into()),
                (0xE5, "<0xE5>".into()),
                (0xB0, "<0xB0>".into()),
                (0xBC, "<0xBC>".into()),
            ]),
            special_tokens: HashSet::from([1, 2]),
        }
    }

    #[test]
    fn decodes_word_markers_and_skips_special_tokens() {
        assert_eq!(tokenizer().decode(&[1, 3, 4, 2]).unwrap(), "local speech");
    }

    #[test]
    fn reassembles_utf8_byte_fallback_tokens() {
        assert_eq!(tokenizer().decode(&[0xE5, 0xB0, 0xBC]).unwrap(), "尼");
    }

    #[test]
    fn validates_byte_token_syntax() {
        assert_eq!(parse_byte_token("<0xFF>"), Some(255));
        assert_eq!(parse_byte_token("<0xGG>"), None);
        assert_eq!(parse_byte_token("hello"), None);
    }
}
