use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use quil_audio::read_wav;
use quil_core::{AudioBuffer, ModelSpec, TranscribeOptions, WerStats, word_error_rate};
use quil_inference::Execution;
use serde::{Deserialize, Serialize};

use crate::{CliError, system};

#[derive(Debug, Deserialize)]
struct Corpus {
    name: String,
    files: Vec<CorpusEntry>,
}

#[derive(Debug, Deserialize)]
struct CorpusEntry {
    audio: PathBuf,
    reference: String,
}

struct Clip {
    audio: AudioBuffer,
    reference: String,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub recorded_unix: u64,
    pub tool_versions: system::ToolVersions,
    pub model_id: String,
    pub model_revision: String,
    pub model_bytes: u64,
    pub execution: String,
    pub hardware: system::Hardware,
    pub corpus: String,
    pub clips: usize,
    pub audio_seconds: f64,
    pub warm_runs: usize,
    pub load_ms: f64,
    pub first_transcription_ms: f64,
    pub warm_run_ms: Vec<f64>,
    pub rss_after_warm_run_bytes: Vec<u64>,
    pub median_warm_run_ms: f64,
    pub real_time_factor: f64,
    pub peak_rss_bytes: u64,
    pub rss_after_load_bytes: u64,
    pub rss_after_inference_bytes: u64,
    pub word_error_rate: f64,
    pub wer_stats: WerStats,
}

pub fn run(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
    corpus_path: &Path,
    runs: usize,
) -> Result<BenchmarkReport, CliError> {
    if runs == 0 || runs > 20 {
        return Err(CliError::Invalid(
            "benchmark runs must be between 1 and 20".into(),
        ));
    }
    let corpus_json = std::fs::read_to_string(corpus_path).map_err(|source| CliError::Io {
        path: corpus_path.to_path_buf(),
        source,
    })?;
    let corpus: Corpus = serde_json::from_str(&corpus_json).map_err(|source| CliError::Json {
        path: corpus_path.to_path_buf(),
        source,
    })?;
    if corpus.files.is_empty() {
        return Err(CliError::Invalid("benchmark corpus is empty".into()));
    }
    let parent = corpus_path.parent().unwrap_or_else(|| Path::new("."));
    let mut clips = Vec::with_capacity(corpus.files.len());
    let mut audio_seconds = 0.0;
    for entry in corpus.files {
        let audio = read_wav(&parent.join(&entry.audio))?;
        audio_seconds += audio.duration_seconds();
        clips.push(Clip {
            audio,
            reference: entry.reference,
        });
    }
    if audio_seconds <= 0.0 || audio_seconds > 600.0 {
        return Err(CliError::Invalid(
            "benchmark corpus duration must be between 0 and 600 seconds".into(),
        ));
    }

    let load_start = Instant::now();
    let loaded = quil_inference::load(model, &model_dir.join(&model.id), execution)?;
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.0;
    let rss_after_load_bytes = system::current_rss_bytes();
    let mut transcriber = loaded.transcriber;
    let options = TranscribeOptions::default();

    let mut hypotheses = Vec::with_capacity(clips.len());
    let mut first_transcription_ms = 0.0;
    for (index, clip) in clips.iter().enumerate() {
        let start = Instant::now();
        let text = transcriber.transcribe(clip.audio.samples(), &options)?;
        if index == 0 {
            first_transcription_ms = start.elapsed().as_secs_f64() * 1000.0;
        }
        hypotheses.push(text);
    }

    let mut warm_run_ms = Vec::with_capacity(runs);
    let mut rss_after_warm_run_bytes = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        for clip in &clips {
            drop(transcriber.transcribe(clip.audio.samples(), &options)?);
        }
        warm_run_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        rss_after_warm_run_bytes.push(system::current_rss_bytes());
    }
    let median_warm_run_ms = median(&warm_run_ms);
    let mut wer_stats = WerStats::default();
    for (clip, hypothesis) in clips.iter().zip(&hypotheses) {
        wer_stats.merge(word_error_rate(&clip.reference, hypothesis));
    }

    Ok(BenchmarkReport {
        schema_version: 2,
        recorded_unix: system::unix_time(),
        tool_versions: system::tool_versions(),
        model_id: model.id.clone(),
        model_revision: model.revision.clone(),
        model_bytes: model.files.iter().map(|file| file.bytes).sum(),
        execution: loaded.execution,
        hardware: system::hardware(),
        corpus: corpus.name,
        clips: clips.len(),
        audio_seconds,
        warm_runs: runs,
        load_ms,
        first_transcription_ms,
        warm_run_ms,
        rss_after_warm_run_bytes,
        median_warm_run_ms,
        real_time_factor: median_warm_run_ms / (audio_seconds * 1000.0),
        peak_rss_bytes: system::peak_rss_bytes(),
        rss_after_load_bytes,
        rss_after_inference_bytes: system::current_rss_bytes(),
        word_error_rate: wer_stats.error_rate(),
        wer_stats,
    })
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}
