# DiCam 850 — JSON recording profiles

Bake custom recording settings into a flashable `SPHOST.BRN` from a JSON profile.

This is the **patch-time** version of the "JSON profiles" idea: you describe the
settings in a JSON on the PC, the tool writes them into the firmware's video-mode
table, fixes the checksum, and you flash the result. (The runtime version — the
camera reading a JSON straight off the SD card every boot — needs a JSON parser
injected into the firmware and is a later step.)

## Usage

```
python -I diprofile.py --list                       # show all video modes + stock bitrates
python -I diprofile.py myprofile.json               # -> ./SPHOST.BRN (from clean rev4)
python -I diprofile.py myprofile.json --base ../patches/SPHOST_smooth_br.BRN --out OUT.BRN
```

Profiles **stack**: pass a patched BRN as `--base` to keep smooth-auto / bitrate /
NR edits and add the profile on top. Flash the output as `SPHOST.BRN` via the
camera menu `FW Update`. Revert anytime by flashing `flash_ready/SPHOST.BRN`.

## Profile schema (all keys optional)

```json
{
  "name": "my-profile",
  "bitrate_scale": 1.5,
  "bitrate_mbit": {
    "3840x2160@30": [80, 70, 60],
    "1920x1080@30": 30
  },
  "resolution": {
    "2704x1520@60": { "width": 2560, "height": 1440 }
  },
  "vbrc_cap_kbps": 120000
}
```

- `bitrate_scale` — multiply every mode's three bitrates (High/Std/Low).
- `bitrate_mbit` — per-mode override. List `[High,Std,Low]` or a single int (High only).
  Units are **Mbit/s**. Mode keys are `WIDTHxHEIGHT@FPS` exactly as `--list` prints them.
- `resolution` — **EXPERIMENTAL** custom output size / stretch (edits the mode's
  width/height). The sensor crop + scaler may not follow every value; test and be
  ready to revert. Non-standard sizes can break recording.
- `vbrc_cap_kbps` — upper VBR cap so it never clamps your new bitrate. Auto by
  default (max target + headroom).

## What it edits (safe, same-size, reversible)

- Video-mode table @ VA `0x409aa060` — per-mode bitrate triples and width/height.
- VBRC cap fields @ `0x407c4420 / 443c / 4458`.
- Fixes the BRN checksum (u32 sum ≡ 0, compensation word @ `0x1fc`).

Bitrate edits are low risk (encoder config, not boot code). A card that can't keep
up with a high bitrate makes recording stutter/stop — lower it or reflash the base.
`resolution` edits are experimental; see above.

## Video ISO limit (`max_iso` / `max_gain`)

```json
{ "name": "iso800", "max_iso": 800 }
```

Caps the gain the auto-exposure may use in **video** (every `RO_RES/IQ/AEPCHART*.BIN`
except the `CAP` photo charts). Fewer noise "particles" at night, darker image.
Scale is an estimate: gain byte 96 = ISO 100 (the `ISO100` charts stop there),
16 per stop → 144 ≈ ISO 800, 160 ≈ ISO 1600 (stock video max 160–168).
`max_gain` sets the raw byte instead. Needs `../brntool` built (resource repack).

## Custom formats config (`formats`)

The ergonomic way to design your own recording formats: a named list, each
assigning a custom format to one menu **slot**. Run `--list` to see the slots.

```json
{
  "name": "my-custom-formats",
  "formats": [
    {"slot": "3840x2160@30", "name": "4K Scope",    "aspect": 2.35, "bitrate": 80},
    {"slot": "2704x1520@60", "name": "2.7K Flat 60", "aspect": 1.85, "bitrate": 60},
    {"slot": "1920x1080@60", "name": "1080 Scope",   "aspect": 2.35, "bitrate": 45},
    {"slot": "1920x1080@30", "name": "1080 HiBR",    "bitrate": 40}
  ]
}
```

Per format: `slot` (which menu entry to repurpose), optional `name` (yours, for
readability), and any of `aspect` / `width` / `height` / `bitrate`. The tool
prints the **resulting recording menu** and warns when a size is not a multiple
of 16 or exceeds the slot's sensor readout (upscale → likely broken).

Rules of thumb for what actually works:
- **Safe:** change `bitrate` on any slot.
- **Crop (keep width, smaller height / wider aspect):** experimental but plausible —
  may crop (good) or squish (bad). Test. `fps` is fixed by the slot.
- **Avoid:** width/height larger than the slot's stock size (the sensor can't
  read more than it already does for that mode).

## Examples

- `highbitrate.json` — every mode ×1.5.
- `maxquality.json` — explicit high bitrates for the main modes.
- `customres.json` — experimental 2.7K60 → 2560×1440 stretch.
