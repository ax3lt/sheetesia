use opencv::core::*;
use opencv::imgproc::*;

use crate::piano::octave::*;

// Semitone offset of each white key from the C that starts its octave
const WHITE_KEY_CLASSES: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

pub struct Piano {
	pub octaves: Vec<Octave>,
}

// A horizontal run of same-brightness pixels along the sampled row
struct Run {
	light: bool,
	start: i32,
	end: i32, // exclusive
}

impl Run {
	fn width(&self) -> i32 { self.end - self.start }
	fn center(&self) -> i32 { (self.start + self.end) / 2 }
}

// One detected key. White keys carry their own index along the keyboard,
// black keys carry the index of the white key immediately to their left.
struct Key {
	white_index: usize,
	black: bool,
	x: i32,
}

impl Piano {

	// Returns None if this frame has no usable keyboard on screen,
	//	which covers intro frames, fades and title cards.
	pub fn new(image: &Mat, transpose: i32) -> Option<Piano> {
		let mut gray: Mat = Mat::default();
		cvt_color_def(image, &mut gray, COLOR_BGR2GRAY).ok()?;

		let (band_top, band_bottom) = find_keyboard_band(&gray)?;

		// Sample near the top of the keys, where the black keys are still drawn,
		//	so that a single row shows the full 12 semitone pattern
		let sample_y = band_top + ((band_bottom - band_top) * 30) / 100;
		let runs = encode_row(&gray, sample_y)?;

		let (keys, white_count, c_white) = read_keys(&runs)?;
		let c_midi = anchor_octave(&keys, white_count, c_white);

		// Sample every key, and work out the typical colour of a white and of a
		//	black key so that any key lit up in this frame can be spotted below
		let mut samples: Vec<Vec3b> = Vec::with_capacity(keys.len());
		for key in &keys {
			samples.push(*image.at_2d(sample_y, key.x).ok()?);
		}
		let white_resting = median_color(&keys, &samples, false);
		let black_resting = median_color(&keys, &samples, true);

		// Build the notes, grouped by the octave they belong to
		let mut octaves: Vec<Octave> = Vec::new();
		let mut current: Vec<Note> = Vec::new();
		let mut current_octave: i32 = i32::MIN;

		for (i, key) in keys.iter().enumerate() {
			let code = key_midi(key, c_white, c_midi) + transpose;

			// Anything outside this range cannot be written to a MIDI file
			if code < 0 || code > 127 {
				continue;
			}

			let octave = code / 12;
			if octave != current_octave && !current.is_empty() {
				octaves.push(Octave::new(std::mem::take(&mut current)));
			}
			current_octave = octave;

			// A key that was already lit in the detection frame would otherwise
			//	record its pressed colour as its resting colour
			let resting = if key.black { black_resting } else { white_resting };
			let mut diff: i32 = 0;
			for channel in 0..3 {
				diff = diff + (samples[i][channel] as i32 - resting[channel] as i32).abs();
			}
			let default_color = if diff > 100 { resting } else { samples[i] };

			current.push(Note::new(
				code as u8,
				Point { x: key.x, y: sample_y },
				default_color
			));
		}
		if !current.is_empty() {
			octaves.push(Octave::new(current));
		}
		if octaves.is_empty() {
			return None;
		}

		return Some(Piano { octaves: octaves });
	}

}

// Find the rows the keyboard occupies. It is the lowest solid band of lit rows
// in the frame, far brighter than the falling note background above it.
fn find_keyboard_band(gray: &Mat) -> Option<(i32, i32)> {
	let rows = gray.rows();
	if rows < 40 {
		return None;
	}

	let mut means: Vec<f64> = Vec::with_capacity(rows as usize);
	for y in 0..rows {
		let row = gray.row(y).ok()?;
		means.push(mean_def(&row).ok()?[0]);
	}

	let brightest = means.iter().cloned().fold(0.0, f64::max);
	let thresh = f64::max(20.0, brightest * 0.15);

	// Videos often leave a black margin under the keyboard, so walk up to it
	//	rather than assuming it reaches the bottom edge of the frame
	let mut bottom = rows - 1;
	while bottom > 0 && means[bottom as usize] < thresh {
		bottom = bottom - 1;
	}
	let mut top = bottom;
	while top > 0 && means[(top - 1) as usize] >= thresh {
		top = top - 1;
	}

	// A real keyboard is a band in the lower part of the frame, not a stray line
	if bottom - top < 20 || bottom < rows/2 {
		return None;
	}
	return Some((top, bottom));
}

// Per channel median colour across every key of one type. Keys lit up in the
// sampled frame are a minority, so the median lands on the resting colour.
fn median_color(keys: &Vec<Key>, samples: &Vec<Vec3b>, black: bool) -> Vec3b {
	let mut out: [u8; 3] = [0, 0, 0];
	for channel in 0..3 {
		let mut values: Vec<u8> = keys.iter().zip(samples.iter())
			.filter(|(key, _)| key.black == black)
			.map(|(_, sample)| sample[channel])
			.collect();
		if values.is_empty() {
			continue;
		}
		values.sort();
		out[channel] = values[values.len()/2];
	}
	return Vec3b::from(out);
}

// Split one pixel row into alternating light and dark runs
fn encode_row(gray: &Mat, y: i32) -> Option<Vec<Run>> {
	let cols = gray.cols();
	let mut values: Vec<u8> = Vec::with_capacity(cols as usize);
	for x in 0..cols {
		values.push(*gray.at_2d::<u8>(y, x).ok()?);
	}

	let lo = *values.iter().min()? as i32;
	let hi = *values.iter().max()? as i32;
	// A flat row has no keys to segment
	if hi - lo < 40 {
		return None;
	}
	let thresh = ((lo + hi) / 2) as u8;

	let mut runs: Vec<Run> = Vec::new();
	let mut start: i32 = 0;
	let mut light = values[0] > thresh;
	for x in 1..cols {
		let is_light = values[x as usize] > thresh;
		if is_light != light {
			runs.push(Run { light: light, start: start, end: x });
			start = x;
			light = is_light;
		}
	}
	runs.push(Run { light: light, start: start, end: cols });

	return Some(runs);
}

// Turn the runs into keys, and work out where the first C sits
fn read_keys(runs: &Vec<Run>) -> Option<(Vec<Key>, usize, usize)> {
	let mut white_widths: Vec<i32> = runs.iter().filter(|r| r.light).map(|r| r.width()).collect();
	if white_widths.len() < 8 {
		return None;
	}
	white_widths.sort();
	let white_width = white_widths[white_widths.len()/2];

	// A black key is roughly as wide as the visible part of a white key, while
	//	the separator drawn between two adjacent white keys is only a few pixels
	let black_min_width = std::cmp::max(white_width/2, 2);

	let mut keys: Vec<Key> = Vec::new();
	let mut gaps: Vec<usize> = Vec::new();
	let mut white_count: usize = 0;

	for (i, run) in runs.iter().enumerate() {
		if run.light {
			keys.push(Key { white_index: white_count, black: false, x: run.center() });
			white_count = white_count + 1;
			continue;
		}

		// Skip the dark borders of the frame itself
		if i == 0 || i == runs.len()-1 || white_count == 0 {
			continue;
		}

		if run.width() >= black_min_width {
			keys.push(Key { white_index: white_count-1, black: true, x: run.center() });
		} else {
			gaps.push(white_count - 1);
		}
	}
	if keys.is_empty() {
		return None;
	}

	// Two white keys with nothing between them are only ever E|F and B|C.
	//	Those two gaps are 3 white keys apart one way and 4 the other, which is
	//	enough to pin every key on the keyboard to a real note name.
	let mut c_white: Option<usize> = None;
	for pair in gaps.windows(2) {
		match pair[1] as i32 - pair[0] as i32 {
			3 => { c_white = Some(pair[0] + 1); break; } // pair[0] is B|C, so the next white key is C
			4 => { c_white = Some(pair[0] + 5); break; } // pair[0] is E|F, so C is 4 white keys further on
			_ => {}
		}
	}

	return Some((keys, white_count, c_white? % 7));
}

// MIDI note number for one key, given which white key is the first C and which
// real octave that C belongs to
fn key_midi(key: &Key, c_white: usize, c_midi: i32) -> i32 {
	let octave_offset = (key.white_index as i32 - c_white as i32).div_euclid(7);
	let class = WHITE_KEY_CLASSES[(key.white_index + 7 - c_white) % 7];
	return c_midi + 12*octave_offset + class + if key.black {1} else {0};
}

// Decide which real octave the keyboard sits in. A full 88 key keyboard is
// unambiguous. Anything else gets centred on middle C, and the caller can
// correct it with the transpose argument.
fn anchor_octave(keys: &Vec<Key>, white_count: usize, c_white: usize) -> i32 {
	// 88 keys start at A0, which puts the first C at C1
	if white_count == 52 {
		return 24;
	}

	let mut best = 48;
	let mut best_err = i32::MAX;
	for c_midi in (12..=96).step_by(12) {
		let lowest = key_midi(keys.first().unwrap(), c_white, c_midi);
		let highest = key_midi(keys.last().unwrap(), c_white, c_midi);
		let err = ((lowest + highest)/2 - 60).abs();
		if err < best_err {
			best_err = err;
			best = c_midi;
		}
	}
	return best;
}

#[cfg(test)]
mod tests {
	use super::*;

	// The run pattern an 88 key keyboard produces: a white key, then either a
	// black key or the thin separator that marks an E|F or B|C pair
	fn keyboard_88() -> Vec<Run> {
		let mut runs: Vec<Run> = Vec::new();
		let mut x = 0;
		let push = |runs: &mut Vec<Run>, light: bool, width: i32, x: &mut i32| {
			runs.push(Run { light: light, start: *x, end: *x + width });
			*x = *x + width;
		};

		push(&mut runs, false, 2, &mut x); // frame edge
		for white in 0..52 {
			push(&mut runs, true, 20, &mut x);
			if white == 51 {
				break;
			}
			// White keys 1 and 4 mod 7 are B and E, the two with no black key after
			let class = WHITE_KEY_CLASSES[(white + 7 - 2) % 7];
			let width = if class == 4 || class == 11 { 3 } else { 23 };
			push(&mut runs, false, width, &mut x);
		}
		push(&mut runs, false, 2, &mut x); // frame edge
		return runs;
	}

	#[test]
	fn reads_a_full_88_key_keyboard() {
		let (keys, white_count, c_white) = read_keys(&keyboard_88()).unwrap();

		assert_eq!(white_count, 52, "an 88 key piano has 52 white keys");
		assert_eq!(keys.len(), 88);
		assert_eq!(c_white, 2, "the keyboard opens on A0, B0, then C1");
	}

	#[test]
	fn names_the_outer_keys_correctly() {
		let (keys, white_count, c_white) = read_keys(&keyboard_88()).unwrap();
		let c_midi = anchor_octave(&keys, white_count, c_white);

		assert_eq!(c_midi, 24, "the first C on an 88 key piano is C1");
		assert_eq!(key_midi(keys.first().unwrap(), c_white, c_midi), 21, "lowest key is A0");
		assert_eq!(key_midi(keys.last().unwrap(), c_white, c_midi), 108, "highest key is C8");
	}

	#[test]
	fn every_key_is_a_semitone_above_the_last() {
		let (keys, _, c_white) = read_keys(&keyboard_88()).unwrap();
		let codes: Vec<i32> = keys.iter().map(|k| key_midi(k, c_white, 24)).collect();

		for pair in codes.windows(2) {
			assert_eq!(pair[1] - pair[0], 1, "gap between {} and {}", pair[0], pair[1]);
		}
	}
}
