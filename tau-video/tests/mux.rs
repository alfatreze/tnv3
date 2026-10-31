use std::{fs, io::Cursor, process::Command};
use tau_video::{Tnv3Reader, container::mux_tvc};
#[test]
fn video_only_container_is_readable_and_matches_c_checker() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    let summary = mux_tvc(&source[..], &mut output).unwrap();
    assert_eq!((summary.frames, summary.level), (8, 1));
    let bytes = output.into_inner();
    let mut r = Tnv3Reader::open(Cursor::new(&bytes)).unwrap();
    assert_eq!(r.info().frame_count, 8);
    let presentations = (0..8)
        .map(|i| r.index_entry(i).unwrap().presentation_sample)
        .collect::<Vec<_>>();
    assert_eq!(
        presentations,
        vec![0, 6000, 2000, 4000, 12000, 8000, 10000, 14000]
    );
    if let Some(tool) = std::env::var_os("TAU_TNV3_TOOL") {
        let path = std::env::temp_dir().join(format!("tau-video-mux-{}.tnv3", std::process::id()));
        fs::write(&path, &bytes).unwrap();
        assert!(
            Command::new(tool)
                .arg("check")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let _ = fs::remove_file(path);
    }
}
#[test]
fn rejects_invalid_input_and_nonempty_output() {
    let input = include_bytes!("../src/tvc/sequence-reference.tvc");
    assert!(mux_tvc(&input[..], Cursor::new(vec![1])).is_err());
    assert!(mux_tvc(&b"bad"[..], Cursor::new(Vec::new())).is_err());
    let mut trailing = input.to_vec();
    trailing.push(1);
    assert!(mux_tvc(&trailing[..], Cursor::new(Vec::new())).is_err());
}

#[test]
fn local_tvc1_vectors_pass_c_container_checker_if_available() {
    let (Some(dir), Some(tool)) = (
        std::env::var_os("TAU_TVC_VECTOR_DIR"),
        std::env::var_os("TAU_TNV3_TOOL"),
    ) else {
        return;
    };
    let path =
        std::env::temp_dir().join(format!("tau-video-mux-vectors-{}.tnv3", std::process::id()));
    for name in [
        "v16_dialog_l2_3slices.tvc",
        "v17_bw_l3_2slices.tvc",
        "v19_l1_crop_slices.tvc",
    ] {
        let input = fs::read(std::path::Path::new(&dir).join(name)).unwrap();
        let mut output = Cursor::new(Vec::new());
        mux_tvc(&input[..], &mut output).unwrap();
        fs::write(&path, output.into_inner()).unwrap();
        assert!(
            Command::new(&tool)
                .arg("check")
                .arg(&path)
                .status()
                .unwrap()
                .success(),
            "{name}"
        );
    }
    let _ = fs::remove_file(path);
}

#[test]
fn muxed_tnv3_video_decodes_to_reference_pixels() {
    use tau_video::container::decode_video_y4m;
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut container = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut container).unwrap();
    let encoded = container.into_inner();
    let mut output = Vec::new();
    decode_video_y4m(Cursor::new(&encoded), &mut output).unwrap();
    let mut at = output.windows(6).position(|x| x == b"FRAME\n").unwrap();
    let mut pixels = Vec::new();
    for _ in 0..8 {
        assert_eq!(&output[at..at + 6], b"FRAME\n");
        at += 6;
        pixels.extend_from_slice(&output[at..at + 1536]);
        at += 1536;
    }
    assert_eq!(pixels, include_bytes!("../src/tvc/sequence-pixels.bin"));
    let mut corrupt = encoded;
    corrupt[90] ^= 1;
    assert!(decode_video_y4m(Cursor::new(corrupt), Vec::new()).is_err());
}

#[test]
fn local_tnv3_video_matches_c_demux_and_decoder() {
    use tau_video::container::decode_video_y4m;
    let (Some(dir), Some(mux), Some(decoder)) = (
        std::env::var_os("TAU_VIDEO_VECTOR_DIR"),
        std::env::var_os("TAU_TNV3_TOOL"),
        std::env::var_os("TAU_TVC_DECODER"),
    ) else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tau-tnv3-video-{}", std::process::id()));
    let tvc = base.with_extension("tvc");
    let y4m = base.with_extension("y4m");
    let mut count = 0;
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if !p.file_name().unwrap().to_string_lossy().starts_with('p')
            || p.extension().and_then(|v| v.to_str()) != Some("tnv3")
        {
            continue;
        }
        let file = fs::File::open(&p).unwrap();
        let mut output = Vec::new();
        decode_video_y4m(file, &mut output).unwrap();
        assert!(
            Command::new(&mux)
                .arg("demux")
                .arg(&p)
                .arg(&tvc)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new(&decoder)
                .arg(&tvc)
                .arg(&y4m)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(output, fs::read(&y4m).unwrap(), "{}", p.display());
        count += 1;
    }
    let _ = fs::remove_file(tvc);
    let _ = fs::remove_file(y4m);
    assert!(count > 0);
    eprintln!("Compared {count} TNV3 video tracks");
}
