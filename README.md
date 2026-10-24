<h1><img src="icon.png" style="width: 30px"> MP3 &amp; SRT Maker</h1>

Turn videos into captions (subtitles) on your own computer. Add some videos,
press a button, and it makes a **.srt** caption file you can upload to YouTube.
It can also save an **.mp3** of the sound. Great for YouTube Shorts.

It runs locally using [Whisper](https://github.com/openai/whisper) (via
[whisper.cpp](https://github.com/ggml-org/whisper.cpp)) — your videos never
leave your computer. The app is built with [Tauri](https://tauri.app) and Rust.

---

## What it can do

- 🎬 Add many videos at once (mp4, mov, avi, mkv, webm)
- 📝 Make a **.srt** caption file
- 🎵 Also save an **.mp3** of the audio
- 📁 Put the results in a new folder named after your video
- 📦 Optionally move the video into that folder too, so everything stays together
- 📂 Saves next to your video automatically (you can change where)
- ⏳ A progress bar for each video, with Start / Stop buttons
- ▶️ **Batch Run** — tick 2 or more videos and run them one after another
- 🌍 Auto-detects the language (or you can pick one)

---

## Setup — do this **once**

### Step 1 — Install ffmpeg

ffmpeg is a helper that reads the sound out of videos.

| Your computer | Type this in the Terminal |
| ------------- | ------------------------- |
| **Mac**       | `brew install ffmpeg`     |
| **Windows**   | `winget install ffmpeg`   |
| **Linux**     | `sudo apt install ffmpeg` |

> On a Mac, if `brew` doesn't work, install Homebrew first from **https://brew.sh**.

### Step 2 — Install the build tools

You need these to build the app from source:

1. **Rust** — https://rustup.rs
2. **Node.js** and **pnpm** — https://nodejs.org, then `npm install -g pnpm`
3. **CMake** — Mac: `brew install cmake` · Windows: `winget install Kitware.CMake` · Linux: `sudo apt install cmake`
4. The Tauri system requirements for your computer — https://tauri.app/start/prerequisites/

### Step 3 — Get the app ready

In the Terminal, inside the project folder:

```bash
pnpm install
```

✅ **You're done setting up!**

---

## How to start the app

Run it straight from the project:

```bash
pnpm dev
```

The first build takes a few minutes (it compiles Whisper). After that it's quick.

Or build a real app you can double-click:

```bash
pnpm build
```

The finished app ends up in `src-tauri/target/release/bundle/`
(a `.app` / `.dmg` on Mac, an installer on Windows).

> **Whisper models download on first use.** The first time you pick a model,
> the app downloads it (the default `medium` is about 1.5 GB). It's saved, so
> this only happens once per model.

---

## How to make captions

1. Click **Add Files** and choose one or more videos.
2. Each video gets its own row. The output folder fills in by itself (where your
   video is). Change it with **Browse** if you want.
3. Pick the options for each row:
   - **SRT** — make captions
   - **MP3** — also save the sound as an .mp3
   - **Folder** — keep things tidy in their own folder
   - **Move video** — move the video into that folder too
4. Pick a **model** (bigger = better but slower) and a **language** (or leave it on Auto-detect).
5. Click **Start** on a row — or tick several rows and click **Batch Run**.
6. When it's done, hover over the green ✓ to see where your files were saved.

### Which model should I pick?

| Model      | Size   | Speed   | Notes        |
| ---------- | ------ | ------- | ------------ |
| `tiny`     | 75 MB  | Fastest | Quick drafts |
| `base`     | 142 MB | Fast    | Okay quality |
| `small`    | 466 MB | Medium  | Good quality |
| `medium`   | 1.5 GB | Slower  | **Default**  |
| `large-v3` | 3 GB   | Slowest | Best quality |

Models are stored in the app's data folder
(on a Mac: `~/Library/Application Support/com.edudevteam.mp3srtmaker/models`).

---

## Help! Something went wrong

- **"ffmpeg not found"** — do Step 1 above.
- **The build fails mentioning cmake** — install CMake (Step 2).
- **It's slow** — that's normal for big models. Try the `tiny` or `base` model.
