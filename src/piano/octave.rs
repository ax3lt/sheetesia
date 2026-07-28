use opencv::core::*;

pub struct Octave {
	pub notes: Vec<Note>,
}

pub struct Note {
    pub code: u8,

	pub location: Point,
	pub default_color: Vec3b,

    pressed: bool
}

impl Octave {

    pub fn new(notes: Vec<Note>) -> Octave {
        return Octave {
            notes: notes
        }
    }

}

impl Note {

    pub fn new(code: u8, location: Point, default_color: Vec3b) -> Note {
        return Note {
            code: code,
            location: location,
            default_color: default_color,
            pressed: false
        }
    }

    // Return Err if the note was in a state and is set to the same state:
    //  If the note was pressed and is set to pressed
    //  If the note wasn't pressed and is set to not pressed
    pub fn set_pressed(&mut self, pressed: bool) -> Result<bool,bool> {
        if self.pressed == pressed {
            return Err(false);
        }

        self.pressed = pressed;
        return Ok(pressed);
    }

    // Return human readable note name
    pub fn to_string(&self) -> String {
        let octave_number = (self.code/12) as i32 - 1;
        let note_number: usize = self.code as usize % 12;

        let note_strings = vec!["C ", "C#", "D ", "D#", "E ", "F ", "F#", "G ", "G#", "A ", "A#", "B "];
        return format!("{} {}", note_strings[note_number], octave_number);
    }
}
