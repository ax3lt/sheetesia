# Sheetesia

## Convert Synthesia piano roll videos into MIDI

Easily turn piano tutorials into music sheet

![video to sheet](https://web.archive.org/web/20201215033456if_/https://camo.githubusercontent.com/135cd5f263c8d93eb37fc47b787673a687d8bb5112d422b77bd5f4877d2c370d/68747470733a2f2f66696c656675636b746f72792e67612f66696c65732f32383637323335656235353531386138386261393166393532333239663535332f50726f796563746f732f5368656574657369612f796565706572732e6a7067)

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
