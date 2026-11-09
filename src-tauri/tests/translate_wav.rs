//! Live test against OpenAI: translates a WAV file with `gpt-realtime-translate`.
//!
//! Ignored by default (needs a real key, network, and spends about $0.034 per audio minute).
//! Run it from `src-tauri/` in PowerShell:
//!
//! ```powershell
//! $env:OPENAI_API_KEY = "sk-..."
//! $env:LIVEDUB_TEST_WAV = "C:\path\to\speech-24k-mono.wav"
//! $env:LIVEDUB_TEST_TARGET = "en"   # optional, "es" or "en" (default "en")
//! cargo test --test translate_wav -- --ignored --nocapture
//! ```
//!
//! The WAV must be PCM16, mono, 24 kHz. The translated audio is written to
//! `src-tauri/target/translated.wav` and the transcripts are printed.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use livedub_voice_lib::credentials::ApiKey;
use livedub_voice_lib::settings::Language;
use livedub_voice_lib::translator::pcm::{CHUNK_MS, CHUNK_SAMPLES, SAMPLE_RATE, samples_for_ms};
use livedub_voice_lib::translator::{TranslatorOutput, connect};

/// Silence sent after the speech so the model finishes translating.
const TRAILING_SILENCE_MS: u32 = 2_000;

/// Upper bound for the whole run after the audio is sent.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

type TestResult = Result<(), Box<dyn Error>>;

fn read_wav(path: &Path) -> Result<Vec<i16>, Box<dyn Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(format!(
            "{} must be PCM16 mono 24 kHz, found {spec:?} (convert it with: \
             ffmpeg -i input -ac 1 -ar 24000 -c:a pcm_s16le output.wav)",
            path.display()
        )
        .into());
    }
    Ok(reader.samples::<i16>().collect::<Result<_, _>>()?)
}

fn write_wav(path: &Path, samples: &[i16]) -> TestResult {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs OPENAI_API_KEY, LIVEDUB_TEST_WAV and network access; spends API credit"]
async fn translates_a_wav_file() -> TestResult {
    let key = ApiKey::parse(&std::env::var("OPENAI_API_KEY").map_err(|_| "set OPENAI_API_KEY")?)?;
    let wav_path = PathBuf::from(
        std::env::var("LIVEDUB_TEST_WAV")
            .map_err(|_| "set LIVEDUB_TEST_WAV to a PCM16 mono 24 kHz WAV with speech")?,
    );
    let target = match std::env::var("LIVEDUB_TEST_TARGET").as_deref() {
        Ok("es") => Language::Spanish,
        Ok("en") | Err(_) => Language::English,
        Ok(other) => {
            return Err(format!("LIVEDUB_TEST_TARGET must be es or en, not {other}").into());
        }
    };

    let mut input = read_wav(&wav_path)?;
    input.extend(std::iter::repeat_n(0, samples_for_ms(TRAILING_SILENCE_MS)));
    println!(
        "input: {} ({:.1} s incl. silence), target: {target:?}",
        wav_path.display(),
        input.len() as f64 / f64::from(SAMPLE_RATE)
    );

    let mut session = connect(&key, target).await?;
    println!("session: {}", session.session_id().unwrap_or("unknown"));

    // Stream in real time, 200 ms per chunk, from another task.
    let audio = session.audio_sender();
    let sender = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(u64::from(CHUNK_MS)));
        for chunk in input.chunks(CHUNK_SAMPLES) {
            ticker.tick().await;
            if audio.send(chunk).await.is_err() {
                break;
            }
        }
    });

    let mut translated: Vec<i16> = Vec::new();
    let mut input_text = String::new();
    let mut output_text = String::new();
    let mut errors = Vec::new();
    let mut sender = Some(sender);
    let mut deadline = None;

    loop {
        let next = match (deadline, sender.as_mut()) {
            (Some(deadline), _) => tokio::time::timeout_at(deadline, session.next_output())
                .await
                .map_err(|_| "timed out waiting for the session to close")?,
            (None, Some(streaming)) => tokio::select! {
                output = session.next_output() => output,
                finished = streaming => {
                    // All audio sent: ask OpenAI to flush and close.
                    finished?;
                    sender = None;
                    session.close().await;
                    deadline = Some(tokio::time::Instant::now() + DRAIN_TIMEOUT);
                    continue;
                }
            },
            (None, None) => session.next_output().await,
        };
        match next {
            Some(TranslatorOutput::Audio(samples)) => translated.extend(samples),
            Some(TranslatorOutput::InputTranscriptDelta(delta)) => input_text.push_str(&delta),
            Some(TranslatorOutput::OutputTranscriptDelta(delta)) => output_text.push_str(&delta),
            Some(TranslatorOutput::Error { code, message }) => {
                println!("error: {code}: {message}");
                errors.push(code);
            }
            Some(TranslatorOutput::Closed) | None => break,
        }
    }

    let out_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/translated.wav");
    write_wav(&out_path, &translated)?;
    println!("input transcript:  {input_text}");
    println!("output transcript: {output_text}");
    println!(
        "translated audio: {} ({:.1} s)",
        out_path.display(),
        translated.len() as f64 / f64::from(SAMPLE_RATE)
    );

    assert!(errors.is_empty(), "OpenAI reported errors: {errors:?}");
    assert!(!translated.is_empty(), "no translated audio received");
    Ok(())
}
