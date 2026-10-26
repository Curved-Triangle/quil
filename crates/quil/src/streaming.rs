/// Small energy VAD used to avoid running inference during silence. The
/// thresholds are deliberately conservative and evaluated off the audio thread.
pub fn has_speech(samples: &[f32]) -> bool {
    if samples.is_empty() {
        return false;
    }
    let mut energy = 0.0_f64;
    let mut peak = 0.0_f32;
    for sample in samples {
        energy += f64::from(*sample) * f64::from(*sample);
        peak = peak.max(sample.abs());
    }
    let rms = (energy / samples.len() as f64).sqrt();
    rms > 0.008 && peak > 0.02
}

#[derive(Default)]
pub struct PartialCommitter {
    previous: String,
    emitted_words: Vec<String>,
}

impl PartialCommitter {
    /// Emit only words that survived two consecutive rolling-window results.
    /// This trades a little latency for non-duplicating, append-only output.
    pub fn update(&mut self, transcript: &str) -> String {
        let current = transcript.trim().to_owned();
        let current_words = words(&current);
        let previous_words = words(&self.previous);
        let common = previous_words
            .iter()
            .zip(&current_words)
            .take_while(|(left, right)| left == right)
            .count();
        let stable_count = common.saturating_sub(1);
        let delta = self.take_from(&current_words, stable_count);
        self.previous = current;
        delta
    }

    /// Commit the final utterance without re-emitting a stable prefix. If the
    /// model changed an already-emitted prefix, do not duplicate or corrupt it.
    pub fn finish(&mut self, transcript: &str) -> String {
        let current_words = words(transcript.trim());
        let delta = self.take_from(&current_words, current_words.len());
        self.previous.clear();
        delta
    }

    pub fn reset(&mut self) {
        self.previous.clear();
        self.emitted_words.clear();
    }

    fn take_from(&mut self, current_words: &[String], stable_count: usize) -> String {
        if stable_count <= self.emitted_words.len() || stable_count > current_words.len() {
            return String::new();
        }
        if self
            .emitted_words
            .iter()
            .zip(current_words)
            .any(|(left, right)| left != right)
        {
            return String::new();
        }
        let delta = current_words[self.emitted_words.len()..stable_count].join(" ");
        if delta.is_empty() {
            return delta;
        }
        let needs_space = !self.emitted_words.is_empty();
        self.emitted_words
            .extend_from_slice(&current_words[self.emitted_words.len()..stable_count]);
        if needs_space {
            format!(" {delta}")
        } else {
            delta
        }
    }
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_vad_rejects_silence_and_accepts_tone() {
        assert!(!has_speech(&[0.0; 1600]));
        let tone = (0..1600)
            .map(|index| (index as f32 * 0.1).sin() * 0.1)
            .collect::<Vec<_>>();
        assert!(has_speech(&tone));
    }

    #[test]
    fn emits_only_stable_prefixes() {
        let mut committer = PartialCommitter::default();
        assert_eq!(committer.update("the quick"), "");
        assert_eq!(committer.update("the quick brown"), "the");
        assert_eq!(committer.update("the quick brown fox"), " quick");
        assert_eq!(committer.update("the quick brown fox"), " brown");
        assert_eq!(committer.finish("the quick brown fox jumps"), " fox jumps");
    }

    #[test]
    fn never_duplicates_or_reorders_final_text() {
        let mut committer = PartialCommitter::default();
        assert_eq!(committer.update("hello local"), "");
        assert_eq!(committer.update("hello local world"), "hello");
        assert_eq!(committer.finish("completely different"), "");
    }

    #[test]
    fn reset_starts_a_new_utterance() {
        let mut committer = PartialCommitter::default();
        let _ = committer.update("one two");
        let _ = committer.finish("one two");
        committer.reset();
        assert_eq!(committer.update("fresh words"), "");
    }
}
