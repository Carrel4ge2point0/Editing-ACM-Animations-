# Puppet Killer – animation tools (AC Mirage, single player)

Small tools to **edit vault animations and put them back in the game by ID**, made for the Puppet Killer parkour mod.
Puppet Native (MIT, © venvyl) is credited: its JSON animation format (Unity-style `Tracks` / `Keyframes`) is the
input of the editor and of the encoder, and its `DataPC_extra_patch_02.forge` is used as a reference file.

**No game file is included.** You extract what you need from your own copy of the game (molds, skeleton,
reference forge). Tested on game version 1.1.1.

## What's inside

| Folder | What it is |
|---|---|
| `editor/` | Browser animation editor + tiny local server (Python 3, no dependency) |
| `skeleton/` | Builds `editor/squelette.json` (bones + rest pose) from the game's human skeleton |
| `forge-reader/` | Rust: lists and extracts resources from `.forge` files (read only) |
| `encoder/` | Rust: encodes a Puppet Native JSON into a game animation, using the original animation as a "mold" |
| `forge-writer/` | Python: builds a small `DataPC_extra_patch_02.forge` that replaces animations by ID |

Build the Rust tools with `cargo build --release` in each folder.

## Workflow (≈10 min per animation, game reload included)

1. **Skeleton (once):**
   `forge-reader extraire "<Mirage>\DataPC.forge" 0x18cc2999098 skeleton.bin`
   then `python skeleton/make_skeleton.py skeleton.bin editor/squelette.json`
2. **Editor:** `python editor/server.py <folder of Puppet Native JSON>` then open http://127.0.0.1:8765/
   - pick the animation and the **game slot** (top bar) – check the slot before saving;
   - select a range on the timeline (drag), then: speed ×k (0.5 = twice slower, 2 = twice faster), hold, cut,
     *fit to slot length* (scale everything) or *trim end to fit slot*; rotate a bone with a fade;
   - the original stays visible as a grey ghost; Space = play/pause, ←/→ = frame by frame;
   - **save for the game** writes `editor/edits/<name>.json` (+ `.edition.json` with the list of edits).
3. **Slot length:** `python encoder/set_slot_length.py edits/<name>.json ready.json <slot seconds>`
4. **Mold:** extract the original animation of that slot, e.g.
   `forge-reader extraire "<Mirage>\DataPC.forge" 0x7fc9c4eed mold.bin`
5. **Encode:** `animation-encodeur importer-pn mold.bin ready.json out.bin --essai-sans-canaux-globaux --ignorer-pistes-sans-destination`
   (the encoder refuses to overwrite an existing output and requires Length == mold length).
6. **Forge:** `python forge-writer/build_patch.py --model "<Mirage>\DataPC_extra_DX12.forge" --like <working patch_02> --out DataPC_extra_patch_02.forge --anim 0x7fc9c4eed:out.bin [--anim ...]`
7. Close the game, copy (don't move) the new `DataPC_extra_patch_02.forge` into the game folder, launch.

## Vault slots (sprint)

| ID | Animation (slot) | Length |
|---|---|---|
| `0xfc32a8ee7` | ACE_Nav_Sprint_SkipOver_Fd010cm_Up200cm | 2.000 s |
| `0x89b1766e2` | SkipOver_Fd100cm_Up050cm | 1.633 s |
| `0x296f0c7d9` | SkipOver_Fd100cm_Up100cm | 2.100 s |
| `0xe87191afa` | SkipOver_Fd100cm_Up150cm_02 | 2.233 s |
| `0x89b1766ff` | SkipOver_Fd200cm_Up050cm | 1.667 s |
| `0x296f0f8b8` | SkipOver_Fd200cm_Up100cm | 2.233 s |
| `0x7fc9c4eed` | SkipOver_Fd200cm_Up150cm | 2.567 s |
| `0x7fc9c7405` | SlideOver_Fd100cm_Up050cm | 1.933 s |
| `0x2e1406ef9` | SlideOver_Fd200cm_Up100cm | 1.767 s |
| `0xfc32a8d7a` | SlideOver_Fd200cm_Up200cm | 2.000 s |
| `0x7fc9c7401` | SlideOver_Fd300cm_Up050cm | 2.600 s |
| `0x47bfb02eb` | SlideOver_Fd300cm_Up100cm | 2.533 s |
| `0x7fc9c529c` | SlideOver_Fd300cm_Up150cm | 2.267 s |
| `0x1f252ef0ea0` | Nav_Sprint_SkipOver_Fd010cm_Up100cm_LeftForward | 2.600 s |
| `0x1f252ef2c81` | Nav_Sprint_SkipOver_Fd010cm_Up100cm_LeftForward_02 | 2.600 s |

## What we learned

- The game reads `DataPC_extra_patch_02.forge` by itself and replaces resources **by ID**, including IDs that live in `DataPC.forge`.
  Uncompressed blocks are accepted.
- **An animation shorter than its slot is fine**: the engine blends out by itself, no freeze. Only animations longer than the slot get their end cut.
- Retiming parts of an animation (faster here, slower there) works in game and is a cheap way to smooth jank.
- The two `Nav_Sprint_SkipOver_Fd010cm_Up100cm` slots are picked very often on low obstacles (they take over the Up050 slots).
- All vaults are listed in `ACK_Main_DEV` (type `3527d1ae`, animation graph, latest in `DataPC_patch_01.forge`); the selection logic itself is not decoded.
