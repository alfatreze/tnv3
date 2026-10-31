"""Study clips.  Tag "800" is the full output size (800 wide), "400" the half-size version."""
import pathlib

MEDIA = pathlib.Path("<tau-v-worktree>/media")
WORK = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
TEARS = MEDIA / "tears-of-steel.mp4"
# Owner-supplied anime test file, never committed.  It has holes, a broken seek index and silent slice
# corruption.  slime_800.mkv / slime_400.mkv are lossless FFV1 intermediates from ONE sequential decode
# (slime_intermediate.py); windows were chosen from a pixel-level glitch scan (scan_glitch.py).
SLIME_800 = WORK / "slime_800.mkv"
SLIME_400 = WORK / "slime_400.mkv"

CLIPS = {
    # live action, Tears of Steel 1920x800 (2.4:1) -> 800x334
    "dialog": dict(src=TEARS, ss=355, w=800, h=334, w2=400, h2=166, why="outdoor close-up, static, fine texture"),
    "dark":   dict(src=TEARS, ss=115, w=800, h=334, w2=400, h2=166, why="dark interior, low key"),
    "action": dict(src=TEARS, ss=436, w=800, h=334, w2=400, h2=166, why="VFX tower, camera and object motion"),
    # anime, 1920x1080 HEVC 10-bit -> 800x450 (16:9)
    "anime_wide":   dict(src=SLIME_800, src2=SLIME_400, ss=755, w=800, h=450, w2=400, h2=225,
                         why="courtroom, several characters, flat colour"),
    "anime_dialog": dict(src=SLIME_800, src2=SLIME_400, ss=24, w=800, h=450, w2=400, h2=225,
                         why="magic circles (bright, saturated), then a talking head on a flat sky"),
    "anime_motion": dict(src=SLIME_800, src2=SLIME_400, ss=1285, w=800, h=450, w2=400, h2=225,
                         why="sword draw, fast character motion"),
}
# Owner-supplied 1960s black-and-white TV episode, 640x480 4:3 H.264 at ~0.7 Mbit/s (already lossy and soft, film grain
# partly smoothed by that encode; upscaled 1.25x to 800x600 for the 800 tag).  Never committed.
BW = MEDIA / "MPEG-4 movie.mp4"
CLIPS.update({
    "bw_garden":  dict(src=BW, ss=250, w=800, h=600, w2=400, h2=300, why="B&W greenhouse, foliage, two figures moving"),
    "bw_closeup": dict(src=BW, ss=780, w=800, h=600, w2=400, h2=300, why="B&W close-ups, wicker texture, little motion"),
    "bw_room":    dict(src=BW, ss=1470, w=800, h=600, w2=400, h2=300, why="B&W interior, two characters, small motion"),
})
ORIGINAL = ["dialog", "dark", "action"]
ALL = list(CLIPS)
