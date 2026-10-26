use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct WerStats {
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
    pub reference_words: usize,
}

impl WerStats {
    pub fn error_rate(self) -> f64 {
        if self.reference_words == 0 {
            return if self.insertions == 0 { 0.0 } else { 1.0 };
        }
        (self.substitutions + self.deletions + self.insertions) as f64 / self.reference_words as f64
    }

    pub fn merge(&mut self, other: Self) {
        self.substitutions += other.substitutions;
        self.deletions += other.deletions;
        self.insertions += other.insertions;
        self.reference_words += other.reference_words;
    }
}

/// Lowercase and replace punctuation with spaces while preserving letters,
/// numbers, and intra-word apostrophes. This deliberately stays small and
/// deterministic; the benchmark corpus uses the same normalizer for both sides.
pub fn normalized_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '\'' {
            current.push(character);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn word_error_rate(reference: &str, hypothesis: &str) -> WerStats {
    let reference = normalized_words(reference);
    let hypothesis = normalized_words(hypothesis);
    let n = reference.len();
    let m = hypothesis.len();

    // Two dynamic-programming rows are sufficient for edit distance. Traceback
    // uses a compact operation matrix so S/D/I counts remain inspectable.
    let mut distance = vec![0_u32; m + 1];
    let mut operations = vec![vec![0_u8; m + 1]; n + 1];
    for (column, value) in distance.iter_mut().enumerate() {
        *value = column as u32;
        operations[0][column] = 3; // insertion
    }
    operations[0][0] = 0;

    for (i, reference_word) in reference.iter().enumerate() {
        let mut previous_diagonal = distance[0];
        distance[0] = (i + 1) as u32;
        operations[i + 1][0] = 2; // deletion
        for (j, hypothesis_word) in hypothesis.iter().enumerate() {
            let column = j + 1;
            let above = distance[column];
            let substitution_cost = u32::from(reference_word != hypothesis_word);
            let substitution = previous_diagonal + substitution_cost;
            let deletion = above + 1;
            let insertion = distance[column - 1] + 1;
            let (best, operation) = if substitution <= deletion && substitution <= insertion {
                (substitution, if substitution_cost == 0 { 0 } else { 1 })
            } else if deletion <= insertion {
                (deletion, 2)
            } else {
                (insertion, 3)
            };
            distance[column] = best;
            operations[i + 1][column] = operation;
            previous_diagonal = above;
        }
    }

    let mut stats = WerStats {
        reference_words: n,
        ..WerStats::default()
    };
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match operations[i][j] {
            0 => {
                i -= 1;
                j -= 1;
            }
            1 => {
                stats.substitutions += 1;
                i -= 1;
                j -= 1;
            }
            2 => {
                stats.deletions += 1;
                i -= 1;
            }
            3 => {
                stats.insertions += 1;
                j -= 1;
            }
            _ => unreachable!("every edit cell has an operation"),
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_punctuation_and_spacing() {
        assert_eq!(
            normalized_words("Hello,  WORLD! It's local."),
            ["hello", "world", "it's", "local"]
        );
    }

    #[test]
    fn counts_edit_operations() {
        let stats = word_error_rate("one two three four", "one changed three extra four");
        assert_eq!(stats.substitutions, 1);
        assert_eq!(stats.insertions, 1);
        assert_eq!(stats.deletions, 0);
        assert_eq!(stats.reference_words, 4);
        assert_eq!(stats.error_rate(), 0.5);
    }

    #[test]
    fn deletion_is_counted() {
        let stats = word_error_rate("hello local world", "hello world");
        assert_eq!(stats.deletions, 1);
        assert_eq!(stats.error_rate(), 1.0 / 3.0);
    }
}
