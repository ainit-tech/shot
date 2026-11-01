//! The short sound that says the microphone has begun to listen to a question asked aloud: two
//! soft notes going up, made here as a WAV file, so that the user hears when to speak without
//! looking at the frozen screen.

use std::f32::consts::PI;

/// Samples a second.
const RATE: u32 = 44_100;
/// The notes, in hertz, and how long each sounds, in milliseconds: G and the D above it.
const NOTES: [(f32, u32); 2] = [(784.0, 70), (1175.0, 90)];
/// The silence between the notes, in milliseconds.
const GAP: u32 = 15;
/// How loud the notes are, of the loudest sound there is: soft.
const LOUDNESS: f32 = 0.2;

/// The chime as a WAV file: 16-bit PCM, one channel.
pub fn wav() -> Vec<u8> {
    let mut samples: Vec<i16> = Vec::new();
    for (i, &(pitch, milliseconds)) in NOTES.iter().enumerate() {
        if i > 0 {
            samples.extend(std::iter::repeat_n(0, (RATE * GAP / 1000) as usize));
        }
        let count = (RATE * milliseconds / 1000) as usize;
        for n in 0..count {
            // Each note swells and fades, so that it starts and ends without a click.
            let envelope = (PI * n as f32 / count as f32).sin();
            let wave = (2.0 * PI * pitch * n as f32 / RATE as f32).sin();
            samples.push((wave * envelope * LOUDNESS * f32::from(i16::MAX)) as i16);
        }
    }
    let data = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    // PCM, one channel, the rate, the bytes a second, the bytes a sample, the bits a sample.
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chime_is_a_short_soft_wav() {
        let wav = wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        let data = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(wav.len(), 44 + data);
        let seconds = data as f32 / (RATE * 2) as f32;
        assert!((0.15..0.25).contains(&seconds), "{seconds} s");
        let loudest = wav[44..].chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs()).max().unwrap();
        assert!(loudest > 1000 && f32::from(loudest) <= LOUDNESS * f32::from(i16::MAX) + 1.0, "{loudest}");
    }
}
