//! BRR encoder

// SPDX-FileCopyrightText: © 2023 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

use crate::decoder::{filter0, filter1, filter2, filter3, I15Sample};
use crate::{
    BrrFilter, BrrSample, BRR_HEADER_END_FLAG, BRR_HEADER_LOOP_FLAG, BYTES_PER_BRR_BLOCK,
    SAMPLES_PER_BLOCK,
};

const MAX_SHIFT: u8 = 12;

const I4_MIN: i32 = -8;
const I4_MAX: i32 = 7;

#[derive(Debug, Clone)]
pub enum EncodeError {
    NoSamples,
    InvalidNumberOfSamples,
    TooManySamples,
    InvalidLoopPoint,
    LoopPointTooLarge(usize, usize),
    DupeBlockHackNotAllowedWithLoopPoint,
    DupeBlockHackNotAllowedWithLoopResetsFilter,
    DupeBlockHackTooLarge,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::NoSamples => write!(f, "no samples"),
            EncodeError::InvalidNumberOfSamples => write!(
                f,
                "number of samples is not a multiple of {SAMPLES_PER_BLOCK}"
            ),
            EncodeError::TooManySamples => write!(f, "too many samples"),
            EncodeError::InvalidLoopPoint => {
                write!(f, "loop_point is not a multiple of {SAMPLES_PER_BLOCK}")
            }
            EncodeError::LoopPointTooLarge(lp, s_len) => write!(
                f,
                "loop_point is too large ({}, max {})",
                lp,
                s_len - SAMPLES_PER_BLOCK
            ),
            EncodeError::DupeBlockHackNotAllowedWithLoopPoint => {
                write!(f, "dupe_block_hack not allowed when loop_point is set")
            }
            EncodeError::DupeBlockHackNotAllowedWithLoopResetsFilter => {
                write!(
                    f,
                    "dupe_block_hack does nothing when loop_resets_filter is set"
                )
            }
            EncodeError::DupeBlockHackTooLarge => write!(f, "dupe_block_hack value is too large"),
        }
    }
}

struct BrrBlock {
    filter: BrrFilter,
    shift: u8,
    // signed 4-bit values
    nibbles: [i8; 16],
    decoded_samples: [I15Sample; SAMPLES_PER_BLOCK],
}

fn build_block(
    samples: &[I15Sample; SAMPLES_PER_BLOCK],
    shift: u8,
    filter: BrrFilter,
    filter_fn: fn(I15Sample, I15Sample) -> i32,
    prev1: I15Sample,
    prev2: I15Sample,
) -> BrrBlock {
    assert!(shift <= MAX_SHIFT);

    let mut nibbles = [0; SAMPLES_PER_BLOCK];
    let mut decoded_samples: [I15Sample; SAMPLES_PER_BLOCK] = Default::default();

    let div: i32 = 1 << shift;

    let mut prev1 = prev1;
    let mut prev2 = prev2;

    for (i, s) in samples.iter().enumerate() {
        let offset = filter_fn(prev1, prev2);

        // Using division instead of `>> shift` to round towards 0 when s is negative
        let n = (((s.value() - offset) << 1) / div).clamp(I4_MIN, I4_MAX);

        // Decode nibble (no shift out-of-range test required)
        let d = I15Sample::clamp_and_clip(((n << shift) >> 1) + offset);

        prev2 = prev1;
        prev1 = d;

        nibbles[i] = n.try_into().unwrap();
        decoded_samples[i] = d;
    }

    BrrBlock {
        filter,
        shift,
        nibbles,
        decoded_samples,
    }
}

fn calc_squared_error(block: &BrrBlock, samples: &[I15Sample; SAMPLES_PER_BLOCK]) -> i64 {
    assert!(block.decoded_samples.len() == samples.len());

    let mut square_error = 0;

    for (b, s) in block.decoded_samples.iter().zip(samples) {
        let delta = i64::from(b.value()) - i64::from(s.value());

        square_error += delta * delta;
    }

    square_error
}

fn find_best_block(
    samples: &[I15Sample; SAMPLES_PER_BLOCK],
    prev1: I15Sample,
    prev2: I15Sample,
) -> BrrBlock {
    let mut best_block = None;
    let mut best_block_score = i64::MAX;

    let mut test_filter = |filter, filter_fn| {
        for shift in 0..=MAX_SHIFT {
            let block = build_block(samples, shift, filter, filter_fn, prev1, prev2);

            let score = calc_squared_error(&block, samples);
            if score < best_block_score {
                best_block = Some(block);
                best_block_score = score;
            }
        }
    };

    test_filter(BrrFilter::Filter0, filter0);
    test_filter(BrrFilter::Filter1, filter1);
    test_filter(BrrFilter::Filter2, filter2);
    test_filter(BrrFilter::Filter3, filter3);

    best_block.unwrap()
}

fn find_best_block_filter(
    samples: &[I15Sample; SAMPLES_PER_BLOCK],
    filter: BrrFilter,
    prev1: I15Sample,
    prev2: I15Sample,
) -> BrrBlock {
    let test_filter = |filter, filter_fn| {
        (0..=MAX_SHIFT)
            .map(|shift| build_block(samples, shift, filter, filter_fn, prev1, prev2))
            .min_by_key(|block| calc_squared_error(block, samples))
            .unwrap()
    };

    match filter {
        BrrFilter::Filter0 => test_filter(BrrFilter::Filter0, filter0),
        BrrFilter::Filter1 => test_filter(BrrFilter::Filter1, filter1),
        BrrFilter::Filter2 => test_filter(BrrFilter::Filter2, filter2),
        BrrFilter::Filter3 => test_filter(BrrFilter::Filter3, filter3),
    }
}

// Loop flag only set if end_flag is set.
fn encode_block(block: BrrBlock, end_flag: bool, loop_flag: bool) -> [u8; BYTES_PER_BRR_BLOCK] {
    assert!(block.shift <= MAX_SHIFT);

    let mut out = [0; BYTES_PER_BRR_BLOCK];

    // Header
    let mut header = ((block.shift & 0xf) << 4) | ((block.filter.as_u8()) << 2);
    if end_flag {
        header |= BRR_HEADER_END_FLAG;

        if loop_flag {
            header |= BRR_HEADER_LOOP_FLAG;
        }
    }

    out[0] = header;

    for (i, o) in out.iter_mut().skip(1).enumerate() {
        let nibble0 = block.nibbles[i * 2].to_le_bytes()[0];
        let nibble1 = block.nibbles[i * 2 + 1].to_le_bytes()[0];

        *o = ((nibble0 & 0xf) << 4) | (nibble1 & 0xf);
    }

    out
}

pub fn encode_brr(
    samples: &[i16],
    loop_offset: Option<usize>,
    dupe_block_hack: Option<usize>,
    loop_filter: Option<BrrFilter>,
) -> Result<BrrSample, EncodeError> {
    if samples.is_empty() {
        return Err(EncodeError::NoSamples);
    }

    if samples.len() % SAMPLES_PER_BLOCK != 0 {
        return Err(EncodeError::InvalidNumberOfSamples);
    }

    if samples.len() > u16::MAX.into() {
        return Err(EncodeError::TooManySamples);
    }

    let (loop_flag, loop_block, loop_offset) = match (loop_offset, dupe_block_hack) {
        (None, None) => (false, usize::MAX, None),
        (Some(lp), None) => {
            if lp % SAMPLES_PER_BLOCK != 0 {
                return Err(EncodeError::InvalidLoopPoint);
            }
            if lp >= samples.len() {
                return Err(EncodeError::LoopPointTooLarge(lp, samples.len()));
            }

            let loop_block = lp / SAMPLES_PER_BLOCK;

            // safe, `samples.len() is <= u16::MAX`
            let loop_offset = u16::try_from(loop_block * BYTES_PER_BRR_BLOCK).unwrap();

            (true, loop_block, Some(loop_offset))
        }
        (None, Some(dbh)) => {
            if dbh > 64 {
                return Err(EncodeError::DupeBlockHackTooLarge);
            }

            if loop_filter == Some(BrrFilter::Filter0) {
                return Err(EncodeError::DupeBlockHackNotAllowedWithLoopResetsFilter);
            }

            let loop_block = dbh;
            let loop_offset = u16::try_from(dbh * BYTES_PER_BRR_BLOCK).unwrap();

            (true, loop_block, Some(loop_offset))
        }
        (Some(_), Some(_)) => {
            return Err(EncodeError::DupeBlockHackNotAllowedWithLoopPoint);
        }
    };

    let n_blocks = samples.len() / SAMPLES_PER_BLOCK + dupe_block_hack.unwrap_or(0);
    let last_block_index = n_blocks - 1;

    let mut brr_data = Vec::with_capacity(n_blocks * BYTES_PER_BRR_BLOCK);

    let mut prev1 = I15Sample::default();
    let mut prev2 = I15Sample::default();

    for (i, samples) in samples
        .chunks_exact(SAMPLES_PER_BLOCK)
        .cycle()
        .take(n_blocks)
        .enumerate()
    {
        let samples: [i16; SAMPLES_PER_BLOCK] = samples.try_into().unwrap();
        let samples = samples.map(I15Sample::from_sample);

        let block = if i == 0 {
            // The first block always uses filter 0
            find_best_block_filter(&samples, BrrFilter::Filter0, prev1, prev2)
        } else if i == loop_block {
            match loop_filter {
                None => find_best_block(&samples, prev1, prev2),
                Some(loop_filter) => find_best_block_filter(&samples, loop_filter, prev1, prev2),
            }
        } else {
            find_best_block(&samples, prev1, prev2)
        };

        prev1 = block.decoded_samples[SAMPLES_PER_BLOCK - 1];
        prev2 = block.decoded_samples[SAMPLES_PER_BLOCK - 2];

        brr_data.extend(encode_block(block, i == last_block_index, loop_flag));
    }

    if let Some(lo) = loop_offset {
        assert!(usize::from(lo) < brr_data.len());
    }

    Ok(BrrSample {
        loop_offset,
        brr_data,
    })
}

#[cfg(test)]
mod test_decoded_samples {
    use crate::decoder::decode_brr_block;

    use super::*;

    /// Encodes a block of samples using all 4 filters and tests if `BrrBlock::decoded_samples` matches `decode_brr_block()`
    fn _test(p2: i16, p1: i16, input: [i16; 16]) {
        const ALL_FILTERS: [BrrFilter; 4] = [
            BrrFilter::Filter0,
            BrrFilter::Filter1,
            BrrFilter::Filter2,
            BrrFilter::Filter3,
        ];

        let i15_input = input.map(I15Sample::from_sample);
        let i15_p1 = I15Sample::from_sample(p1);
        let i15_p2 = I15Sample::from_sample(p2);

        for filter in ALL_FILTERS {
            let best_block = find_best_block_filter(&i15_input, filter, i15_p1, i15_p2);
            let brr_block_samples = best_block.decoded_samples.map(I15Sample::to_sample);

            let brr_block = encode_block(best_block, false, false);
            let decoded_samples = decode_brr_block(&brr_block, p1, p2);

            assert_eq!(brr_block_samples, decoded_samples, "ERROR: {:?}", input);
        }
    }

    #[test]
    fn linear() {
        // (i - 6) / 10 * 0.8 * i16::MAX
        #[rustfmt::skip]
        _test(
            -20970,
            -18349,
            [-15728, -13106, -10485, -7864, -5242, -2621, 0, 2621, 5242, 7864, 10485, 13106, 15728, 18349, 20970, 23592]
        );
    }

    #[test]
    fn sine() {
        // sin(tau * i / 16) * 0.95 * i16::MAX
        #[rustfmt::skip]
        _test(
            -22011,
            -11912,
            [0, 11912, 22011, 28759, 31128, 28759, 22011, 11912, 0, -11912, -22011, -28759, -31128, -28759, -22011, -11912],
        );
    }

    /// Tests a sample that glitches if there is no 15-bit wrapping
    #[test]
    fn wrapping_test() {
        #[rustfmt::skip]
        _test(
            -820,
            -800,
            [
                -450, -450,  800,  6000,  30000,  32000,  400,  200,
                 400,  450, -800, -6000, -30000, -32000, -400, -200,
            ],
        );
    }
}
