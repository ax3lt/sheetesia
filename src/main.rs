use opencv::core::*;
use opencv::videoio::*;

use opencv::highgui::*;

use ghakuf::messages::*;
use ghakuf::writer::*;
use std::path;

mod piano;
use crate::piano::piano::*;

fn main() {
	// --glow can go anywhere on the command line. It switches to the recognition
	//	for stylised videos, where lit keys fade in and the lines between white
	//	keys are faint. Without it the original recognition runs unchanged.
	let args: Vec<String> = std::env::args().skip(1).collect();
	let glow: bool = args.iter().any(|arg| arg == "--glow");
	let positional: Vec<&String> = args.iter().filter(|arg| *arg != "--glow").collect();

	// Load files
	let video_path: &str = positional.get(0).expect("Please provide a video as an argument");
	let mut video: VideoCapture = VideoCapture::from_file(video_path, CAP_ANY).unwrap();

	// Optional second argument, in case the keyboard on screen isn't a full 88
	//	keys and the detected octave ends up off by one
	let transpose: i32 = match positional.get(1) {
		Some(arg) => arg.parse().expect("Transpose must be a whole number of semitones"),
		None => 0,
	};

	if !video.is_opened().unwrap() {
		println!(":(");
		std::process::exit(1);
	}
	println!("Loaded video: {}", video_path);

	// Make window
	named_window("Gabo", WINDOW_AUTOSIZE).unwrap();
	
	// ---------------
	// FIND THE PIANO
	//	Detect from a frame well into the video. The opening frames are often
	//	black or half way through a fade in, and the key colours sampled there
	//	would be recorded as the resting colours for the whole run.
	println!("Finding piano...");
	let mut frame: Mat = Mat::default();
	let total_frames = video.get(CAP_PROP_FRAME_COUNT).unwrap();

	let mut piano: Piano = {
		// A key that is lit up merges into its neighbours and disappears from the
		//	pattern, so sample frames across the whole video and keep the reading
		//	that recovered the most keys. A frame with nothing pressed wins.
		//	Glow mode throws away any reading that is not perfectly consistent,
		//	so it looks at more frames to be sure of finding a clean one.
		let samples = if glow { 48 } else { 12 };
		let mut found: Option<Piano> = None;
		let mut best_keys: usize = 0;

		for step in 1..=samples {
			let at = (total_frames * step as f64 / (samples as f64 + 1.0)).floor();
			video.set(CAP_PROP_POS_FRAMES, at).unwrap();
			if !video.read(&mut frame).unwrap() {
				continue;
			}
			if let Some(candidate) = Piano::new(&frame, transpose, glow) {
				let keys: usize = candidate.octaves.iter().map(|o| o.notes.len()).sum();
				if keys > best_keys {
					best_keys = keys;
					found = Some(candidate);
				}
			}
		}

		match found {
			Some(p) => {
				let lowest = p.octaves.first().unwrap().notes.first().unwrap().to_string();
				let highest = p.octaves.last().unwrap().notes.last().unwrap().to_string();
				println!("Found piano: {} keys, {} octaves, {} to {}", best_keys, p.octaves.len(), lowest, highest);
				p
			},
			None => {
				println!("Couldn't find a piano in this video");
				std::process::exit(1);
			}
		}
	};

	// Rewind, the detection frame was only for measuring the keyboard
	video.set(CAP_PROP_POS_FRAMES, 0.0).unwrap();

	// ----------
	// PLAY VIDEO
	println!("Detecting notes!");

	// MIDI variables
	let path = path::Path::new("out.mid");

	// Some containers report no frame rate at all
	let fps: f64 = match video.get(CAP_PROP_FPS).unwrap() {
		f if f > 1.0 => f,
		_ => 30.0,
	};

	// One tick per video frame. A quarter note is worth `ticks_per_quarter_note`
	//	frames, and the tempo stretches that to match the real frame rate, so a
	//	fractional rate like 29.97 stays in sync instead of drifting.
	let ticks_per_quarter_note: u16 = fps.round() as u16;
	let microseconds_per_quarter_note: u32 = (1_000_000.0 * ticks_per_quarter_note as f64 / fps).round() as u32;
	let ticks_per_frame: u32 = 1;

	// MIDI messages
	let mut midi_messages: Vec<Message> = vec![
		// Set video FPS as tempo
		Message::MetaEvent {
			delta_time: 0,
			event: MetaEvent::SetTempo,
			data: [(microseconds_per_quarter_note >> 16) as u8, (microseconds_per_quarter_note >> 8) as u8, microseconds_per_quarter_note as u8].to_vec(),
		},
	
		Message::MetaEvent {
			delta_time: 0,
			event: MetaEvent::EndOfTrack,
			data: Vec::new(),
		},
	
		Message::TrackChange
	];

	// Vector storing each note and the color it had on the previous frame
	//	Seed it from the opening frame rather than from the resting colors: a
	//	video that opens on black would otherwise look like every key on the
	//	keyboard was struck at once the moment the picture appears.
	video.read(&mut frame).unwrap();
	let mut previous_frame_note_colors: Vec<Vec<Vec3b>> = Vec::new();
	for octave in &piano.octaves {
		let mut note_colors_vec: Vec<Vec3b> = Vec::new();
		for note in &octave.notes {
			note_colors_vec.push(*frame.at_2d(note.location.y, note.location.x).unwrap());
		}
		previous_frame_note_colors.push(note_colors_vec);
	}

	// Video variables
	let mut frame_count: u32 = 0; // number of played frames
	let mut frame_count_on_last_event: u32 = 0; // because SMF uses time since last event rather than absolute time

	// LOOP UNTIL THE VIDEO ENDS
	//	Default: a key only changes state when its color jumps between two frames,
	//	which suits videos where keys light up instantly
	let between_frames_thresh: i32 = 150;
	let color_thresh: i32 = 200;

	//	--glow: lit keys fade in over a few frames, so a key counts as pressed
	//	once it moves clearly away from its resting color, and as released only
	//	once it is nearly back
	let press_thresh: i32 = 100;
	let release_thresh: i32 = 50;

	loop {
		if !video.read(&mut frame).unwrap() {
			println!("Donete");
			break;
		}

		// Color of every key in this frame, and its distance from the resting color
		let mut colors: Vec<Vec<Vec3b>> = Vec::new();
		let mut diffs: Vec<Vec<i32>> = Vec::new();
		for octave in &piano.octaves {
			let mut octave_colors: Vec<Vec3b> = Vec::new();
			let mut octave_diffs: Vec<i32> = Vec::new();
			for note in &octave.notes {
				let note_color: Vec3b = *frame.at_2d(note.location.y, note.location.x).unwrap();
				let mut diff: i32 = 0;
				for i in 0..3 {
					diff += (note_color[i] as i32 - note.default_color[i] as i32).abs();
				}
				octave_colors.push(note_color);
				octave_diffs.push(diff);
			}
			colors.push(octave_colors);
			diffs.push(octave_diffs);
		}

		// Nobody holds down half the keyboard, so while playing the typical key is
		//	at rest. When it is not, the whole picture is changing (a black intro,
		//	a fade, a title card) and in glow mode every key is left as it was.
		let mut diff_by_code: [i32; 128] = [0; 128];
		for (octave, octave_diffs) in piano.octaves.iter().zip(diffs.iter()) {
			for (note, diff) in octave.notes.iter().zip(octave_diffs.iter()) {
				diff_by_code[note.code as usize] = *diff;
			}
		}
		let mut all_diffs: Vec<i32> = diffs.iter().flatten().cloned().collect();
		all_diffs.sort();
		if glow && all_diffs[all_diffs.len()/2] > release_thresh {
			frame_count = frame_count + 1;
			continue;
		}

		let mut key_pressed_or_released_in_this_frame = false;

		// For each note in every octave
		let mut octave_index = 0;
		for octave in &mut piano.octaves {
			let mut note_index = 0;
			for note in &mut octave.notes {
				let note_color: Vec3b = colors[octave_index][note_index];
				let diff_with_default_color: i32 = diffs[octave_index][note_index];

				let state: Option<bool> = if glow {
					let thresh = if note.is_pressed() { release_thresh } else { press_thresh };
					Some(diff_with_default_color > thresh && !is_spill(note.code, &diff_by_code))
				} else {
					// Skip check if the color is close to the one in the previous frame
					let mut diff_with_previous_frame: i32 = 0;
					for i in 0..3 {
						diff_with_previous_frame += (note_color[i] as i32 - previous_frame_note_colors[octave_index][note_index][i] as i32).abs();
					}

					// If the color isn't close to the one in the previous frame
					//	Check if the color is close to the default color (Key released) or not (key pressed)
					if diff_with_previous_frame > between_frames_thresh {
						Some(diff_with_default_color > color_thresh)
					} else {
						None
					}
				};

				if let Some(state) = state {
					let result: Result<bool, bool> = note.set_pressed(state);
					match result {
						Ok(pressed) => {
							if pressed {
								println!("{}\tpressed \t@ frame {} of {}\t({:.2}%)", note.to_string(), frame_count, total_frames, (frame_count as f64 / total_frames)*100.0);
								midi_messages.push(
									Message::MidiEvent {
										delta_time: (frame_count - frame_count_on_last_event) * ticks_per_frame,
										event: MidiEvent::NoteOn {
											ch: 0,
											note: note.code,
											velocity: 0x7f,
										},
									}
								);
							} else {
								println!("{}\treleased\t@ frame {} of {}\t({:.2}%)", note.to_string(), frame_count, total_frames, (frame_count as f64 / total_frames)*100.0);
								midi_messages.push(
									Message::MidiEvent {
										delta_time: (frame_count - frame_count_on_last_event) * ticks_per_frame,
										event: MidiEvent::NoteOff {
											ch: 0,
											note: note.code,
											velocity: 0x7f,
										},
									}
								);
							}

							// Show frame
							key_pressed_or_released_in_this_frame = true;

							// Update variable to keep track of the last pushed MIDI event
							frame_count_on_last_event = frame_count;
						},
						Err(_) => {}
					};
				}

				// Update previous frame pixel color
				previous_frame_note_colors[octave_index][note_index] = note_color;
				note_index = note_index+1;
			}

			octave_index = octave_index+1;
		}

		// Show frame
		if key_pressed_or_released_in_this_frame {
			imshow("Gabo", &frame).unwrap();
			wait_key(1).unwrap();
		}

		// Increment frame count
		frame_count = frame_count + 1;
	}
	

	// Ending MIDI messages
	midi_messages.push(
		Message::MetaEvent {
			delta_time: 0,
			event: MetaEvent::EndOfTrack,
			data: Vec::new(),
    	}
	);

	// Write MIDI
	let mut midi_writer = Writer::new();
	midi_writer.running_status(true);
	midi_writer.time_base(ticks_per_quarter_note);
	for message in &midi_messages {
    	midi_writer.push(message);
	}
	midi_writer.write(&path).unwrap();

	// Cleanup
	video.release().unwrap();
	destroy_window("Gabo").unwrap();
}

// The glow of a lit white key spills onto the next white key over when no black
// key sits between them (E|F and B|C). Treat a key as spill when that neighbour
// is clearly brighter. Two keys about as bright as each other are both pressed.
fn is_spill(code: u8, diff_by_code: &[i32; 128]) -> bool {
	let partner: i32 = match code % 12 {
		4 | 11 => code as i32 + 1,
		5 | 0 => code as i32 - 1,
		_ => return false,
	};
	if partner < 0 || partner > 127 {
		return false;
	}
	return diff_by_code[code as usize] * 100 < diff_by_code[partner as usize] * 90;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn only_the_dimmer_of_two_touching_white_keys_is_spill() {
		// B4 lit, its glow spilling onto C5, as measured on a real video
		let mut diffs: [i32; 128] = [0; 128];
		diffs[71] = 199;
		diffs[72] = 159;
		assert!(!is_spill(71, &diffs));
		assert!(is_spill(72, &diffs));

		// Both pressed, both equally lit
		diffs[72] = 199;
		assert!(!is_spill(71, &diffs) && !is_spill(72, &diffs));

		// A key with a black key on both sides never takes spill this way
		diffs[74] = 50;
		diffs[73] = 0;
		assert!(!is_spill(74, &diffs));
	}
}
