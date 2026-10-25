//! OpenAI Privacy Filter: a model that marks the private data in a text, names, addresses, dates,
//! e-mail addresses, phone numbers, links, account numbers and secrets. It runs on the computer,
//! with ONNX Runtime; masking gives it the text read in a screenshot, and what it marks is covered
//! as what the rules find is.
//!
//! The model tells for each token of the text how likely each of 33 labels is: outside anything
//! private, or the beginning, inside, end or the whole of one of 8 kinds of data. The labels of
//! all the tokens are then chosen together, the most likely sequence in which every piece of data
//! begins before it goes on and ends after it began, as the model's authors decode it.

use std::path::Path;

use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::mask::{Found, Kind};

/// The kinds of data in the order of the model's labels.
const KINDS: [Kind; 8] =
    [Kind::Account, Kind::Address, Kind::Date, Kind::Email, Kind::Person, Kind::Phone, Kind::Url, Kind::Secret];
/// The labels: outside, then the beginning, inside, end and whole of each kind.
const LABELS: usize = 1 + 4 * KINDS.len();

/// The model, loaded and ready.
pub struct Filter {
    session: Session,
    tokenizer: Tokenizer,
    _environment: Environment,
}

impl Filter {
    /// Loads the model from its folder, with ONNX Runtime from its library.
    pub fn load(runtime: &Path, folder: &Path) -> Result<Filter, String> {
        let environment = ort::init_from(runtime)
            .map_err(|error| error.to_string())?
            .with_telemetry(false)
            .build()
            .map_err(|error| error.to_string())?;
        let session = Session::builder(&environment)
            .and_then(|builder| builder.with_optimization_level(GraphOptimizationLevel::Level3).map_err(Into::into))
            .and_then(|mut builder| builder.commit_from_file(folder.join("model_q4.onnx")))
            .map_err(|error| error.to_string())?;
        let tokenizer = Tokenizer::from_file(folder.join("tokenizer.json")).map_err(|error| error.to_string())?;
        Ok(Filter { session, tokenizer, _environment: environment })
    }

    /// What the model marks in the text, by its bytes.
    pub fn find(&mut self, text: &str) -> Result<Vec<Found>, String> {
        let encoding = self.tokenizer.encode(text, false).map_err(|error| error.to_string())?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| i64::from(id)).collect();
        let count = ids.len();
        if count == 0 {
            return Ok(Vec::new());
        }
        let inputs = Tensor::from_array(([1, count], ids))
            .and_then(|ids| Ok((ids, Tensor::from_array(([1, count], vec![1i64; count]))?)))
            .map_err(|error| error.to_string())?;
        let outputs = self
            .session
            .run(ort::inputs!["input_ids" => inputs.0, "attention_mask" => inputs.1])
            .map_err(|error| error.to_string())?;
        let (_, logits) = outputs["logits"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        if logits.len() != count * LABELS {
            return Err(format!("the model gave {} scores for {count} tokens", logits.len()));
        }
        Ok(spans(&decode(logits), encoding.get_offsets()))
    }
}

/// What a label says of its token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Outside,
    Begin(usize),
    Inside(usize),
    End(usize),
    Whole(usize),
}

fn part(label: usize) -> Part {
    match label {
        0 => Part::Outside,
        _ => {
            let kind = (label - 1) / 4;
            match (label - 1) % 4 {
                0 => Part::Begin(kind),
                1 => Part::Inside(kind),
                2 => Part::End(kind),
                _ => Part::Whole(kind),
            }
        }
    }
}

/// Whether a label may follow another: a piece of data goes on and ends only after it began, and
/// a new one begins only after the one before has ended.
fn may_follow(before: Part, after: Part) -> bool {
    match before {
        Part::Begin(kind) | Part::Inside(kind) => matches!(after, Part::Inside(k) | Part::End(k) if k == kind),
        Part::Outside | Part::End(_) | Part::Whole(_) => matches!(after, Part::Outside | Part::Begin(_) | Part::Whole(_)),
    }
}

/// The most likely labels of the tokens, from the model's scores, `LABELS` of them a token, that
/// make a sequence in which every piece of data begins, goes on and ends in order.
fn decode(scores: &[f32]) -> Vec<usize> {
    let tokens = scores.len() / LABELS;
    // The likelihoods of the labels of each token, as logarithms.
    let likely: Vec<f32> = scores
        .chunks_exact(LABELS)
        .flat_map(|row| {
            let top = row.iter().copied().fold(f32::MIN, f32::max);
            let sum: f32 = row.iter().map(|s| (s - top).exp()).sum();
            row.iter().map(move |s| s - top - sum.ln())
        })
        .collect();
    let first = |label: usize| matches!(part(label), Part::Outside | Part::Begin(_) | Part::Whole(_));
    let last = |label: usize| matches!(part(label), Part::Outside | Part::End(_) | Part::Whole(_));
    let mut best: Vec<f32> = (0..LABELS).map(|l| if first(l) { likely[l] } else { f32::NEG_INFINITY }).collect();
    let mut from = vec![0usize; tokens * LABELS];
    for token in 1..tokens {
        let mut next = vec![f32::NEG_INFINITY; LABELS];
        for label in 0..LABELS {
            for (before, &score) in best.iter().enumerate() {
                if score > next[label] && may_follow(part(before), part(label)) {
                    next[label] = score;
                    from[token * LABELS + label] = before;
                }
            }
            next[label] += likely[token * LABELS + label];
        }
        best = next;
    }
    let mut label = (0..LABELS).filter(|&l| last(l)).max_by(|&a, &b| best[a].total_cmp(&best[b])).unwrap_or(0);
    let mut labels = vec![0; tokens];
    for token in (0..tokens).rev() {
        labels[token] = label;
        label = from[token * LABELS + label];
    }
    labels
}

/// The pieces of data that the labels mark, by the bytes of the tokens' text.
fn spans(labels: &[usize], offsets: &[(usize, usize)]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut start = None;
    for (&label, &(from, to)) in labels.iter().zip(offsets) {
        match part(label) {
            Part::Begin(_) => start = Some(from),
            Part::End(kind) => {
                if let Some(start) = start.take() {
                    found.push(Found { range: start..to, kind: KINDS[kind] });
                }
            }
            Part::Whole(kind) => found.push(Found { range: from..to, kind: KINDS[kind] }),
            Part::Outside | Part::Inside(_) => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scores that make the given labels the likeliest, each by far.
    fn scores(labels: &[usize]) -> Vec<f32> {
        labels.iter().flat_map(|&label| (0..LABELS).map(move |l| if l == label { 5.0 } else { 0.0 })).collect()
    }

    #[test]
    fn labels_tell_the_part_and_the_kind() {
        assert_eq!(part(0), Part::Outside);
        assert_eq!(part(17), Part::Begin(4));
        assert_eq!(KINDS[4], Kind::Person);
        assert_eq!(part(32), Part::Whole(7));
        assert_eq!(KINDS[7], Kind::Secret);
    }

    #[test]
    fn the_likeliest_labels_are_kept_when_they_make_sense() {
        // "Anna Petrova lives here": a person in two tokens.
        assert_eq!(decode(&scores(&[17, 19, 0, 0])), [17, 19, 0, 0]);
    }

    /// A piece of data that the model would leave unfinished is finished, or not begun.
    #[test]
    fn a_piece_of_data_always_ends() {
        // The likeliest labels would be: begin a person, then outside.
        let mut scores = scores(&[17, 0, 0]);
        // The end of a person is the second likeliest for the second token.
        scores[LABELS + 19] = 4.0;
        assert_eq!(decode(&scores), [17, 19, 0]);
        // A token inside a person with nothing before it can't be.
        let labels = decode(&self::scores(&[18, 0]));
        assert!(labels.iter().all(|&l| l == 0 || matches!(part(l), Part::Begin(_) | Part::End(_) | Part::Whole(_))));
    }

    /// Runs the model downloaded into the program's folder of models on lines of the kind that
    /// screenshots have, and shows what it marks and how long it takes:
    /// `cargo test -p ainitshot the_model_marks -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_model_marks_private_data() {
        let models = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("ainitShot\\models");
        let started = std::time::Instant::now();
        let runtime = models.join("onnxruntime-1.31.0\\onnxruntime.dll");
        let mut filter = Filter::load(&runtime, &models.join("privacy-filter")).unwrap();
        println!("loaded in {:?}", started.elapsed());
        let text = "From: Anna Petrova <anna.petrova@example.com>\nTo: John Smith\nDate: 25 October 2026\n\
            Call me at +1 212 555 0123 or come to 221B Baker Street, London.\n\
            My password is hunter2 and the API key is sk-proj-abcdefghijklmnopqrstuvwx1234.\n\
            Сергей Иванов, Москва, ул. Ленина, д. 5, тел. 8 (916) 123-45-67\n\
            Card 4242 4242 4242 4242, see https://example.com/private/report";
        for round in 0..2 {
            let started = std::time::Instant::now();
            let found = filter.find(text).unwrap();
            println!("round {round}: found {} in {:?}", found.len(), started.elapsed());
            for f in found.iter().filter(|_| round == 0) {
                println!("{:>14}: {:?}", f.kind.name(), &text[f.range.clone()]);
            }
        }
        let found = filter.find(text).unwrap();
        assert!(found.iter().any(|f| f.kind == Kind::Person));
        assert!(found.iter().any(|f| f.kind == Kind::Email));
    }

    #[test]
    fn labels_become_pieces_of_the_text() {
        let offsets = [(0, 4), (4, 12), (12, 18), (18, 23), (23, 40)];
        let found = spans(&[17, 19, 0, 0, 16], &offsets);
        assert_eq!(found, [Found { range: 0..12, kind: Kind::Person }, Found { range: 23..40, kind: Kind::Email }]);
    }
}
