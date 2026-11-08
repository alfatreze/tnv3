//! The pure-Rust muxer against the C reference muxer (`tnv3 mux`) and against the repository's vectors.
use std::{fs, io::Cursor, path::PathBuf, process::Command};
use tau_video::container::{MuxOptions, extract_tvc, mux_streams};

fn vector(name: &str) -> Vec<u8> {
    fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/vectors/tnv3_15_16").join(format!("{name}.tnv3")))
        .expect("repository vector")
}
fn area_of(file: &[u8]) -> Vec<u8> {
    let so = u32::from_le_bytes(file[44..48].try_into().unwrap()) as usize;
    let sl = u32::from_le_bytes(file[48..52].try_into().unwrap()) as usize;
    file[so..so + sl].to_vec()
}

#[test]
fn extract_then_mux_reproduces_the_c_muxed_vectors_byte_for_byte() {
    // These files were made by `tnv3 mux` (video + subtitle area, no audio, no chunk CRC). Taking the TVC1
    // stream out and muxing it again with the same options must give the same bytes.
    for name in ["ok15_w0_draft14_compatible", "ok15_w1_w2_two_sizes", "ok15_w4_largest", "ok15_event_bottom_exact"] {
        let file = vector(name);
        let tvc = extract_tvc(Cursor::new(&file)).unwrap();
        let area = area_of(&file);
        let mut out = Cursor::new(Vec::new());
        let s = mux_streams(&tvc, &MuxOptions { subtitles: Some(&area), level: Some(file[20]), ..Default::default() }, &mut out).unwrap();
        assert_eq!((s.frames, s.audio_frames), (4, 0));
        assert_eq!(out.into_inner(), file, "{name}");
    }
}

#[test]
fn rejects_bad_inputs_without_writing() {
    let file = vector("ok15_w0_draft14_compatible");
    let tvc = extract_tvc(Cursor::new(&file)).unwrap();
    let mut out = Cursor::new(Vec::new());
    for (what, video, opts) in [
        ("not a TVC stream", b"nope".to_vec(), MuxOptions::default()),
        ("truncated", tvc[..tvc.len() - 1].to_vec(), MuxOptions::default()),
        ("trailing bytes", [tvc.clone(), vec![0]].concat(), MuxOptions::default()),
        ("level 0 override of a TVC0 stream", {
            let mut v = tvc.clone();
            v[3] = b'0';
            v.drain(16..20);
            v
        }, MuxOptions::default()),
        ("bad level", tvc.clone(), MuxOptions { level: Some(4), ..Default::default() }),
        ("zero rate", tvc.clone(), MuxOptions { rate: Some((0, 1)), ..Default::default() }),
        ("not TAC", tvc.clone(), MuxOptions { audio: Some(b"TAC1 not audio at all."), ..Default::default() }),
        ("broken subtitle area", tvc.clone(), MuxOptions { subtitles: Some(&[1, 0, 0, 0]), ..Default::default() }),
    ] {
        assert!(mux_streams(&video, &opts, &mut out).is_err(), "{what}");
        assert!(out.get_ref().is_empty(), "{what}: nothing may be written");
    }
}

/// Byte-for-byte comparison with `tnv3 mux` over several option sets, with real audio. Needs the C tools:
/// `TAU_TEST_TVC_ENC`, `TAU_TEST_TAC`, `TAU_TEST_TNV3` (set by `make rust-conformance`); without them it is skipped.
#[test]
fn matches_the_c_muxer_with_audio_subtitles_crc_trim_shift_and_rate() {
    let (Some(tvc_enc), Some(tac), Some(tnv3)) = (
        std::env::var_os("TAU_TEST_TVC_ENC"),
        std::env::var_os("TAU_TEST_TAC"),
        std::env::var_os("TAU_TEST_TNV3"),
    ) else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tau-mux-native-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    // synthetic 96x64 video (12 frames, 24 fps) and a 1.2 s stereo tone
    let mut y4m = b"YUV4MPEG2 W96 H64 F24:1 Ip A1:1 C420jpeg\n".to_vec();
    for k in 0..12usize {
        y4m.extend(b"FRAME\n");
        y4m.extend((0..96 * 64).map(|i| ((i % 96) * 2 + (i / 96) + k * 5) as u8));
        y4m.extend((0..2 * 48 * 32).map(|i| (110 + (i + k) % 30) as u8));
    }
    fs::write(dir.join("in.y4m"), &y4m).unwrap();
    let mut wav = Vec::new();
    let data = 57600usize * 4;
    wav.extend(b"RIFF");
    wav.extend((36 + data as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend([1, 0, 2, 0]);
    wav.extend(48000u32.to_le_bytes());
    wav.extend(192000u32.to_le_bytes());
    wav.extend([4, 0, 16, 0]);
    wav.extend(b"data");
    wav.extend((data as u32).to_le_bytes());
    for i in 0..57600usize {
        let v = ((i as f64 * 0.0587).sin() * 9000.0) as i16;
        wav.extend(v.to_le_bytes());
        wav.extend(((v as i32 / 2) as i16).to_le_bytes());
    }
    fs::write(dir.join("in.wav"), &wav).unwrap();
    let ok = |c: &mut Command| assert!(c.output().unwrap().status.success());
    ok(Command::new(&tvc_enc).args(["in.y4m", "v.tvc", "-q", "30", "-b", "2", "-l", "1"]).current_dir(&dir));
    ok(Command::new(&tac).args(["enc", "in.wav", "a.tac"]).current_dir(&dir));
    let video = fs::read(dir.join("v.tvc")).unwrap();
    let audio = fs::read(dir.join("a.tac")).unwrap();
    let area = area_of(&vector("ok15_w1_w2_two_sizes"));
    fs::write(dir.join("area.bin"), &area).unwrap();
    type Case = (&'static [&'static str], MuxOptions<'static>);
    let leak = |v: Vec<u8>| -> &'static [u8] { Box::leak(v.into_boxed_slice()) };
    let (audio, area) = (leak(audio), leak(area));
    let cases: Vec<Case> = vec![
        (&[], MuxOptions::default()),
        (&["-c"], MuxOptions { crc: true, ..Default::default() }),
        (&["-a", "a.tac", "-c"], MuxOptions { crc: true, audio: Some(audio), ..Default::default() }),
        (&["-a", "a.tac", "-T"], MuxOptions { audio: Some(audio), trim_audio: true, ..Default::default() }),
        (&["-a", "a.tac", "-c", "-T", "-A", "3"], MuxOptions { crc: true, audio: Some(audio), trim_audio: true, audio_shift: 3, ..Default::default() }),
        (&["-a", "a.tac", "-c", "-s", "area.bin"], MuxOptions { crc: true, audio: Some(audio), subtitles: Some(area), ..Default::default() }),
        (&["-a", "a.tac", "-c", "-r", "25/1"], MuxOptions { crc: true, audio: Some(audio), rate: Some((25, 1)), ..Default::default() }),
        (&["-L", "1", "-c"], MuxOptions { crc: true, level: Some(1), ..Default::default() }),
    ];
    for (n, (c_args, options)) in cases.iter().enumerate() {
        let c_out = dir.join(format!("c{n}.tnv3"));
        let r = Command::new(&tnv3).arg("mux").arg("v.tvc").args(*c_args).arg("-o").arg(&c_out).current_dir(&dir).output().unwrap();
        assert!(r.status.success(), "case {n}: C mux failed: {}", String::from_utf8_lossy(&r.stderr));
        let mut out = Cursor::new(Vec::new());
        mux_streams(&video, options, &mut out).unwrap_or_else(|e| panic!("case {n}: {e}"));
        assert!(out.into_inner() == fs::read(&c_out).unwrap(), "case {n} {c_args:?}: Rust and C muxers differ");
    }
    // a negative shift makes the audio arrive too late: both muxers refuse the result
    let neg = MuxOptions { crc: true, audio: Some(audio), audio_shift: -2, ..Default::default() };
    assert!(mux_streams(&video, &neg, &mut Cursor::new(Vec::new())).is_err());
    fs::remove_dir_all(&dir).unwrap();
}
