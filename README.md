# Sheetesia

## Convert Synthesia piano roll videos into MIDI

Easily turn piano tutorials into music sheet

![video to sheet](https://web.archive.org/web/20201215033456if_/https://camo.githubusercontent.com/135cd5f263c8d93eb37fc47b787673a687d8bb5112d422b77bd5f4877d2c370d/68747470733a2f2f66696c656675636b746f72792e67612f66696c65732f32383637323335656235353531386138386261393166393532333239663535332f50726f796563746f732f5368656574657369612f796565706572732e6a7067)

This is a fork of [alborrajo/sheetesia](https://github.com/alborrajo/sheetesia) that replaces the keyboard detector and brings the project back to a building state on current OpenCV. See [What changed in this fork](#what-changed-in-this-fork).

## How to use

To compile this project you need [cargo](https://doc.rust-lang.org/cargo/index.html) and [OpenCV 4](https://opencv.org/).

- Install OpenCV 4. On macOS, ```brew install opencv@4```. On Linux, your distribution's ```libopencv-dev``` or equivalent. Either way it has to ship a pkg-config file, otherwise [build it yourself](https://github.com/opencv/opencv/issues/13154#issuecomment-456652297)

- Clone this repository

- Point the build at OpenCV and at libclang, which the bindings are generated with. Homebrew keeps opencv@4 keg-only, so on macOS that is:

```sh
export PKG_CONFIG_PATH=/opt/homebrew/opt/opencv@4/lib/pkgconfig
export LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/llvm/lib
```

- Run ```cargo build --release```

- Run it with the video you want to convert as the first argument. On macOS the keg-only install needs to be on the library path at runtime too:

```sh
DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/opencv@4/lib \
	./target/release/sheetesia "your video.mp4"
```

- Wait for it to finish

- Done! The resulting midi will be at ```out.mid```

### If the notes come out in the wrong octave

The keyboard is pinned to real note names by its own black key pattern, and a full 88 key keyboard is then unambiguous. A video showing a shorter keyboard is not, so it gets centred on middle C, which can land an octave off. Pass a number of semitones as a second argument to correct it:

```sh
./target/release/sheetesia "your video.mp4" 12
```

### Stylised videos: `--glow`

Some videos draw the keyboard with a glowing hit line, lit keys that fade in over a few frames, and very faint lines between neighbouring white keys. The default recognition misses most notes there and can misread the keyboard. Add `--glow` anywhere on the command line to switch to the recognition built for them:

```sh
./target/release/sheetesia "your video.mp4" --glow
```

It splits white keys joined by a faint separator, only trusts a keyboard reading whose E|F and B|C gaps alternate perfectly (checking 48 frames instead of 12), decides pressed and released against the resting colour with separate thresholds instead of looking for a jump between frames, skips frames where the whole picture is changing (fades, title cards), and drops the glow that spills from a lit white key onto the white key touching it. Without the flag the original recognition runs unchanged.

Known limit: when a black key lights up, the flare can show on the white keys either side of it rather than on the black key itself. On the video this mode was built against, that lost nearly every black key, so the result sounded out of key. For a solo piano video, transcribing the audio track with a piano transcription model (such as ByteDance's `piano_transcription_inference`) gave a far more accurate MIDI.

## What changed in this fork

### Why the original stopped working

The old detector matched `res/template.png`, a 12x1 pixel strip with one pixel per semitone, pure `#FFFFFF` white keys and `#000000` black keys. It downscaled the whole frame with `INTER_NEAREST` at every width from 12 up to the frame width, ran `match_template` at each one, kept the best `TM_SQDIFF_NORMED` score, then thresholded the result and looped `min_max_loc` to pick off octave instances.

Three separate things were broken:

| Problem | Symptom |
|---|---|
| Bindings crate too old | `opencv` 0.49 hits `unreachable code` generating bindings against OpenCV 4.14. The project did not build at all. |
| Template is colour specific | On a video whose keys are grey rather than pure white, the best match scored **0.9467** on `TM_SQDIFF_NORMED`, where 1.0 means no match at all. It locked onto background noise and reported 13 octaves with MIDI note numbers up to 204, well past the 127 ceiling. |
| First frame assumed usable | A video that opens on black panicked at `piano.rs:79` before doing any work. |

MIDI timing was wrong on top of that, independently of detection. `ticks_per_quarter_note` was dead code, ghakuf writes a time base of 480, and the tempo was set to `fps * 1e6`, which stretched a single video frame to 7.5 seconds. A two minute video would have exported as roughly fourteen hours.

### The new detector

Detection is now geometric rather than colour matched, so the theme of the video does not matter.

1. **Find the keyboard band.** Take the mean brightness of every row and keep the lowest solid lit band, using a threshold of `max(20, 0.15 * brightest)`. It does not assume the keys reach the bottom of the frame, because many videos leave a black margin under them.
2. **Run length encode one row**, 30% of the way down that band so the black keys are still drawn. A light run is a white key, a wide dark run is a black key, and a narrow dark run is the separator where two white keys touch.
3. **Anchor to real pitches.** The only two places where white keys meet with no black key between them are E|F and B|C, and those two alternate 3 and 4 white keys apart. That spacing alone says which gap is which, which names every key on the keyboard.
4. **A full keyboard is then unambiguous.** 52 white keys means 88 keys, so the first C is C1, the lowest key is A0 and the highest is C8.

Three supporting fixes, each one from an observed failure:

- **Lit keys disappear.** A key that is pressed merges into its neighbours and drops out of the run pattern, giving 86 keys instead of 88. Reading the strongest colour channel instead of luminance does not help, because the pressed colour is dark in every channel. So the detector samples twelve frames spread across the video and keeps the reading that recovered the most keys.
- **Resting colours.** These are sampled from a frame in the middle of the video, not the first frame, with a fallback to the median colour for the key type on any key that happens to be lit at that moment. Sampling during an intro fade recorded dim colours as the resting ones, which inverted the pressed test.
- **The opening chord.** Previous frame colours are seeded from the opening frame, otherwise a video that starts on black looks like all 88 keys were struck at once the instant the picture appears.

MIDI timing is now one tick per video frame, with the tempo compensating so a fractional frame rate such as 29.97 stays in sync instead of drifting. Note numbers outside 0 to 127 are dropped rather than written into the file.

### Before and after on the same video

A 1080p 60fps tutorial, 7035 frames, 117.2 seconds:

| | Original | This fork |
|---|---|---|
| Build | fails | clean, no warnings |
| Run | panics on frame 0 | completes |
| Keyboard | 13 "octaves" of noise | 88 keys, 9 octaves |
| Note numbers | up to 204, invalid | 26 to 100, all valid |
| Note on / note off | unbalanced | 947 / 947 |
| MIDI duration | around 14 hours | 115.5 s |

### What this fork can do that the original could not

- Build against current OpenCV 4 and a current Rust toolchain
- Read any colour scheme, since nothing depends on the keys being pure white and black
- Survive black intros, fade ins, and keys already held down when the keyboard is measured
- Handle any keyboard size and position on screen, with no scale search
- Write MIDI that plays back at the speed of the video, at any frame rate
- Correct an ambiguous octave with the `transpose` argument
- `cargo test` covers the pitch logic against a synthetic 88 key keyboard

### What it still assumes

The keyboard has to be horizontal, at the bottom of the frame, with the black keys visible in a single scan row, and it has to be the lowest bright thing on screen. A scrolling, angled or 3D keyboard will not work.

Carried over from the original and not revisited: the press and release thresholds are hardcoded, velocity is fixed, and there is no sustain pedal or velocity extraction.

This has been verified against one video. The 88 key path is covered by tests. The shorter keyboard path centres on middle C and relies on the `transpose` argument, and has not been exercised yet.
