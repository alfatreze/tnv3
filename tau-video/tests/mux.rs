use std::{
    cell::Cell,
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    process::Command,
    rc::Rc,
};
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
    // The random-access API returns exactly the TVC picture payload, excluding
    // the 16-byte chunk header and alignment padding.
    let expected = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut at = 20usize;
    for frame in 0..8 {
        let len = u32::from_le_bytes(expected[at..at + 4].try_into().unwrap()) as usize;
        let mut reader = Tnv3Reader::open(Cursor::new(&bytes)).unwrap();
        assert_eq!(
            reader.read_picture(frame).unwrap(),
            expected[at + 4..at + 4 + len]
        );
        at += 4 + len;
    }
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
fn read_picture_rejects_corrupt_or_inconsistent_chunks() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let clean = output.into_inner();
    let mut damaged = clean.clone();
    damaged[80] ^= 1; // First chunk payload; CRC is enabled by mux_tvc.
    let mut reader = Tnv3Reader::open(Cursor::new(damaged)).unwrap();
    assert!(reader.read_picture(0).is_err());

    let mut damaged = clean.clone();
    damaged[72] = 1; // Reserved chunk-header byte.
    let mut reader = Tnv3Reader::open(Cursor::new(damaged)).unwrap();
    assert!(reader.read_picture(0).is_err());

    let mut reader = Tnv3Reader::open(Cursor::new(clean)).unwrap();
    assert!(reader.read_picture(8).is_err());

    // Header-level open permits lazy index access; read_picture rejects a
    // first offset that skips bytes and a later offset overlapping its peer.
    let clean = {
        let mut out = Cursor::new(Vec::new());
        mux_tvc(&source[..], &mut out).unwrap();
        out.into_inner()
    };
    let mut bad_first = clean.clone();
    let index = u32::from_le_bytes(bad_first[40..44].try_into().unwrap()) as usize;
    bad_first[index..index + 4].copy_from_slice(&68u32.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(bad_first)).unwrap();
    assert!(reader.read_picture(0).is_err());

    let mut bad_second = clean.clone();
    let index = u32::from_le_bytes(bad_second[40..44].try_into().unwrap()) as usize;
    bad_second[index + 16..index + 20].copy_from_slice(&64u32.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(bad_second)).unwrap();
    assert!(reader.read_picture(1).is_err());

    let mut bad_previous = clean;
    let index = u32::from_le_bytes(bad_previous[40..44].try_into().unwrap()) as usize;
    bad_previous[index..index + 4].copy_from_slice(&65u32.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(bad_previous)).unwrap();
    assert!(reader.read_picture(1).is_err());
}

fn audio_fixture(frames: &[Vec<u8>], counts: &[usize], samples: u32) -> Vec<u8> {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let mut video = output.into_inner();
    if frames.len() > 1 {
        let mut reader = Tnv3Reader::open(Cursor::new(&video)).unwrap();
        let key = reader.index_entry(4).unwrap();
        let at = key.offset as usize;
        let index = reader.info().index_offset as usize;
        drop(reader);
        let len = u32::from_le_bytes(video[at..at + 4].try_into().unwrap()) as usize;
        video[at + 16] = 0; // Promote a later anchor to an I picture.
        video[at + 10..at + 12].copy_from_slice(&1u16.to_le_bytes());
        let crc = tau_video::crc32(&video[at + 16..at + len]);
        video[at + 12..at + 16].copy_from_slice(&crc.to_le_bytes());
        video[index + 4 * 16 + 12..index + 4 * 16 + 16]
            .copy_from_slice(&1u32.to_le_bytes());
    }
    let mut reader = Tnv3Reader::open(Cursor::new(&video)).unwrap();
    let count = reader.info().frame_count;
    let entries = (0..count)
        .map(|i| reader.index_entry(i).unwrap())
        .collect::<Vec<_>>();
    let mut result = video[..64].to_vec();
    let mut new_offsets = Vec::new();
    let mut first_tac = Vec::new();
    let mut total_tac = 0u32;
    let mut frame_cursor = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let at = entry.offset as usize;
        let chunk_len = u32::from_le_bytes(video[at..at + 4].try_into().unwrap()) as usize;
        let pic_len = u32::from_le_bytes(video[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &video[at + 16..at + chunk_len];
        new_offsets.push(result.len() as u32);
        first_tac.push(total_tac);
        let mut chunk = video[at..at + 16].to_vec();
        let mut new_body = body.to_vec();
        for _ in 0..counts[i] {
            let frame = &frames[frame_cursor];
            new_body.extend_from_slice(&(frame.len() as u16).to_le_bytes());
            new_body.extend_from_slice(frame);
            frame_cursor += 1;
        }
        total_tac += counts[i] as u32;
        chunk[8..10].copy_from_slice(&(counts[i] as u16).to_le_bytes());
        chunk[0..4].copy_from_slice(&((16 + new_body.len()) as u32).to_le_bytes());
        chunk[12..16].copy_from_slice(&tau_video::crc32(&new_body).to_le_bytes());
        result.extend_from_slice(&chunk);
        result.extend_from_slice(&new_body);
        while result.len() % 4 != 0 {
            result.push(0);
        }
        assert_eq!(
            pic_len,
            u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as usize
        );
    }
    let new_index = result.len() as u32;
    let reset_frames = frames.iter().enumerate().filter_map(|(i, frame)| {
        let code = (u32::from(*frame.get(1).unwrap_or(&0)) << 24)
            | (u32::from(*frame.get(2).unwrap_or(&0)) << 16)
            | (u32::from(*frame.get(3).unwrap_or(&0)) << 8)
            | u32::from(*frame.get(4).unwrap_or(&0));
        (code >= 0x7fff_ffff).then_some(i as u32)
    }).collect::<Vec<_>>();
    for (i, entry) in entries.iter().enumerate() {
        result.extend_from_slice(&new_offsets[i].to_le_bytes());
        result.extend_from_slice(&entry.presentation_sample.to_le_bytes());
        result.extend_from_slice(&first_tac[i].to_le_bytes());
        let mut flags_seek = entry.flags_and_seek & 3;
        if flags_seek & 1 != 0 {
            let seek = reset_frames.iter().copied().take_while(|r| u64::from(*r) * 1024 <= u64::from(entry.presentation_sample)).last().unwrap_or(0);
            flags_seek |= seek << 8;
        }
        result.extend_from_slice(&flags_seek.to_le_bytes());
    }
    result[6..8].copy_from_slice(&3u16.to_le_bytes()); // CRC + audio
    result[21] = 1;
    result[24..28].copy_from_slice(&48_000u32.to_le_bytes());
    result[32..36].copy_from_slice(&samples.to_le_bytes());
    result[36..40].copy_from_slice(&total_tac.to_le_bytes());
    result[40..44].copy_from_slice(&new_index.to_le_bytes());
    let checksum = tau_video::crc32(&result[..60]);
    result[60..64].copy_from_slice(&checksum.to_le_bytes());
    result
}

fn mixed_audio_fixture() -> Vec<u8> {
    let mut audio = vec![0u8; 5000];
    audio[1..5].copy_from_slice(&0u32.to_be_bytes());
    audio_fixture(&[audio], &[1, 0, 0, 0, 0, 0, 0, 0], 0)
}

fn multi_reset_audio_fixture() -> Vec<u8> {
    let make = |reset: bool| {
        let mut frame = vec![0u8; 5];
        frame[1..5].copy_from_slice(&(if reset { u32::MAX } else { 0 }).to_be_bytes());
        frame
    };
    let frames = [make(true), make(true), make(false), make(true)];
    // Reset 3 lives in a chunk after the promoted key picture at chunk 4;
    // seek validation must scan the later cross-chunk audio tail as needed.
    audio_fixture(&frames, &[3, 1, 0, 0, 0, 0, 0, 0], 2049)
}

fn expanded_rate_fixture(
    picture_size: usize,
    fps_num: u32,
    audio_frames: usize,
    tac_size: usize,
    first_tac_extra: usize,
    second_picture_extra: bool,
) -> Vec<u8> {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let video = output.into_inner();
    let mut reader = Tnv3Reader::open(Cursor::new(&video)).unwrap();
    let frames = reader.info().frame_count;
    let entries = (0..frames)
        .map(|n| reader.index_entry(n).unwrap())
        .collect::<Vec<_>>();
    let mut result = video[..64].to_vec();
    let mut offsets = Vec::new();
    let mut first_tac = 0u32;
    for (n, entry) in entries.iter().enumerate() {
        let at = entry.offset as usize;
        let old_pic_len = u32::from_le_bytes(video[at + 4..at + 8].try_into().unwrap()) as usize;
        let mut picture = video[at + 16..at + 16 + old_pic_len].to_vec();
        let this_picture_size = picture_size + usize::from(n == 1 && second_picture_extra);
        picture.resize(this_picture_size, 0);
        offsets.push(result.len() as u32);
        let mut body = picture;
        let count = if n == 0 { audio_frames } else { 0 };
        for i in 0..count {
            let this_tac_size = tac_size + usize::from(first_tac == 0 && i == 0) * first_tac_extra;
            let mut tac = vec![0u8; this_tac_size];
            if first_tac == 0 && i == 0 {
                tac[1..5].copy_from_slice(&u32::MAX.to_be_bytes());
            }
            body.extend_from_slice(&(tac.len() as u16).to_le_bytes());
            body.extend_from_slice(&tac);
        }
        let mut header = video[at..at + 16].to_vec();
        header[0..4].copy_from_slice(&((16 + body.len()) as u32).to_le_bytes());
        header[4..8].copy_from_slice(&(this_picture_size as u32).to_le_bytes());
        header[8..10].copy_from_slice(&(count as u16).to_le_bytes());
        header[12..16].copy_from_slice(&tau_video::crc32(&body).to_le_bytes());
        result.extend_from_slice(&header);
        result.extend_from_slice(&body);
        while result.len() % 4 != 0 {
            result.push(0);
        }
        first_tac += count as u32;
    }
    let index_offset = result.len() as u32;
    let mut cumulative = 0u32;
    for (n, entry) in entries.iter().enumerate() {
        let display = entry.presentation_sample / 2000;
        let pts = u64::from(display) * 48_000 / u64::from(fps_num);
        result.extend_from_slice(&offsets[n].to_le_bytes());
        result.extend_from_slice(&(pts as u32).to_le_bytes());
        result.extend_from_slice(&cumulative.to_le_bytes());
        let flags = entry.flags_and_seek & 3;
        result.extend_from_slice(&flags.to_le_bytes());
        cumulative += if n == 0 { audio_frames as u32 } else { 0 };
    }
    result[6..8].copy_from_slice(&(if audio_frames == 0 { 1u16 } else { 3u16 }).to_le_bytes());
    result[12..16].copy_from_slice(&fps_num.to_le_bytes());
    result[16..20].copy_from_slice(&1u32.to_le_bytes());
    result[20] = 3;
    if audio_frames != 0 {
        result[21] = 1;
        result[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        let sample_count = if audio_frames == 2 { 1024u32 } else { 0 };
        result[32..36].copy_from_slice(&sample_count.to_le_bytes());
        result[36..40].copy_from_slice(&(audio_frames as u32).to_le_bytes());
    }
    result[40..44].copy_from_slice(&index_offset.to_le_bytes());
    let crc = tau_video::crc32(&result[..60]);
    result[60..64].copy_from_slice(&crc.to_le_bytes());
    result
}

fn expanded_video_pictures(video: &[u8], sizes: &[usize]) -> Vec<u8> {
    let mut reader = Tnv3Reader::open(Cursor::new(video)).unwrap();
    assert_eq!(sizes.len(), reader.info().frame_count as usize);
    let entries = (0..reader.info().frame_count)
        .map(|n| reader.index_entry(n).unwrap())
        .collect::<Vec<_>>();
    let mut result = video[..64].to_vec();
    let mut offsets = Vec::new();
    for (n, entry) in entries.iter().enumerate() {
        let at = entry.offset as usize;
        let old_pic_len = u32::from_le_bytes(video[at + 4..at + 8].try_into().unwrap()) as usize;
        let mut picture = video[at + 16..at + 16 + old_pic_len].to_vec();
        assert!(sizes[n] >= old_pic_len);
        picture.resize(sizes[n], 0);
        offsets.push(result.len() as u32);
        let mut chunk = video[at..at + 16].to_vec();
        chunk[0..4].copy_from_slice(&((16 + picture.len()) as u32).to_le_bytes());
        chunk[4..8].copy_from_slice(&(picture.len() as u32).to_le_bytes());
        chunk[12..16].copy_from_slice(&tau_video::crc32(&picture).to_le_bytes());
        result.extend_from_slice(&chunk);
        result.extend_from_slice(&picture);
        while result.len() % 4 != 0 { result.push(0); }
    }
    let index_offset = result.len() as u32;
    for (n, entry) in entries.iter().enumerate() {
        result.extend_from_slice(&offsets[n].to_le_bytes());
        result.extend_from_slice(&entry.presentation_sample.to_le_bytes());
        result.extend_from_slice(&0u32.to_le_bytes());
        result.extend_from_slice(&(entry.flags_and_seek & 3).to_le_bytes());
    }
    result[40..44].copy_from_slice(&index_offset.to_le_bytes());
    let crc = tau_video::crc32(&result[..60]);
    result[60..64].copy_from_slice(&crc.to_le_bytes());
    result
}

fn fractional_24_frame_rate_fixture(last_size: usize) -> Vec<u8> {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut tvc = source[..20].to_vec();
    tvc[8..10].copy_from_slice(&24_000u16.to_le_bytes());
    tvc[10..12].copy_from_slice(&1001u16.to_le_bytes());
    tvc[12..16].copy_from_slice(&24u32.to_le_bytes());
    let mut source_frames = Vec::new();
    let mut at = 20usize;
    while at < source.len() {
        let len = u32::from_le_bytes(source[at..at + 4].try_into().unwrap()) as usize;
        source_frames.push(source[at + 4..at + 4 + len].to_vec());
        at += 4 + len;
    }
    assert_eq!(source_frames.len(), 8);
    for _ in 0..3 {
        for frame in &source_frames {
            tvc.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            tvc.extend_from_slice(frame);
        }
    }
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&tvc[..], &mut output).unwrap();
    let sizes = (0..24).map(|n| if n == 23 { last_size } else if n < 23 { 8_000 } else { 8_000 }).collect::<Vec<_>>();
    expanded_video_pictures(&output.into_inner(), &sizes)
}

#[test]
fn read_picture_accepts_mixed_audio_and_checks_audio_crc() {
    let bytes = mixed_audio_fixture();
    let mut reader = Tnv3Reader::open(Cursor::new(&bytes)).unwrap();
    assert_eq!(reader.read_picture(0).unwrap().len(), 700);
    assert_eq!(reader.read_picture(1).unwrap().len(), 626);

    let mut corrupt = bytes;
    let first_chunk = 64usize;
    let picture_len = u32::from_le_bytes(
        corrupt[first_chunk + 4..first_chunk + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    let audio_at = first_chunk + 16 + picture_len + 2 + 2345;
    corrupt[audio_at] ^= 1;
    let mut reader = Tnv3Reader::open(Cursor::new(corrupt)).unwrap();
    assert!(reader.read_picture(0).is_err());

    let mut corrupt_length = mixed_audio_fixture();
    let picture_len = u32::from_le_bytes(corrupt_length[68..72].try_into().unwrap()) as usize;
    let length_at = 64 + 16 + picture_len;
    corrupt_length[length_at..length_at + 2].copy_from_slice(&0u16.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(corrupt_length)).unwrap();
    assert!(reader.read_picture(0).is_err());

    let mut excess_count = mixed_audio_fixture();
    excess_count[72..74].copy_from_slice(&2u16.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(excess_count)).unwrap();
    assert!(reader.read_picture(0).is_err());
}

#[test]
fn read_picture_rejects_chunk_above_normative_level_window_before_audio_scan() {
    // A level-3 chunk may be at most 665,536 bytes including its header and
    // excluding alignment padding. The file is large enough for the declared
    // chunk, but its advertised size is one byte over the limit.
    let chunk_len = 665_537u32;
    let padded_len = (chunk_len + 3) & !3;
    let index_offset = 64 + padded_len as usize;
    let mut bytes = vec![0u8; index_offset + 16];
    bytes[..4].copy_from_slice(b"TNV3");
    bytes[6..8].copy_from_slice(&2u16.to_le_bytes()); // audio, no CRC
    bytes[8..10].copy_from_slice(&16u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&16u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&24u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes[20] = 3;
    bytes[21] = 1;
    bytes[24..28].copy_from_slice(&48_000u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    let samples = 65_534u32 * 1024;
    bytes[32..36].copy_from_slice(&samples.to_le_bytes());
    bytes[36..40].copy_from_slice(&65_535u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&(index_offset as u32).to_le_bytes());
    let chunk = 64;
    bytes[chunk..chunk + 4].copy_from_slice(&chunk_len.to_le_bytes());
    bytes[chunk + 4..chunk + 8].copy_from_slice(&3u32.to_le_bytes());
    bytes[chunk + 8..chunk + 10].copy_from_slice(&65_534u16.to_le_bytes());
    bytes[chunk + 10..chunk + 12].copy_from_slice(&1u16.to_le_bytes());
    bytes[index_offset..index_offset + 4].copy_from_slice(&64u32.to_le_bytes());
    bytes[index_offset + 12..index_offset + 16].copy_from_slice(&1u32.to_le_bytes());
    let header_crc = tau_video::crc32(&bytes[..60]);
    bytes[60..64].copy_from_slice(&header_crc.to_le_bytes());

    let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
    assert!(matches!(
        reader.read_picture(0),
        Err(tau_video::Error::Invalid {
            code: "E-TNV3-014",
            ..
        })
    ));
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

#[test]
fn cross_frame_check_rejects_display_index_and_offset_mismatches() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let valid = output.into_inner();
    let index = u32::from_le_bytes(valid[40..44].try_into().unwrap()) as usize;
    for at in [index + 2 * 16 + 4, index + 2 * 16] {
        let mut bad = valid.clone();
        bad[at] ^= 1;
        let mut reader = Tnv3Reader::open(Cursor::new(bad)).unwrap();
        assert!(reader.check_cross_frame().is_err());
    }
}

#[test]
fn cross_frame_check_rejects_picture_without_required_anchors() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let mut bad = output.into_inner();
    let first_len = u32::from_le_bytes(bad[64..68].try_into().unwrap()) as usize;
    let second = (64 + first_len + 3) & !3;
    let second_len = u32::from_le_bytes(bad[second..second + 4].try_into().unwrap()) as usize;
    bad[second + 16] = 2; // B picture before the second anchor.
    bad[second + 10..second + 12].copy_from_slice(&2u16.to_le_bytes());
    let body_end = second + second_len;
    let checksum = tau_video::crc32(&bad[second + 16..body_end]);
    bad[second + 12..second + 16].copy_from_slice(&checksum.to_le_bytes());
    let index = u32::from_le_bytes(bad[40..44].try_into().unwrap()) as usize;
    let entry = index + 16;
    let mut ix_flags = u32::from_le_bytes(bad[entry + 12..entry + 16].try_into().unwrap());
    ix_flags = (ix_flags & !3) | 2;
    bad[entry + 12..entry + 16].copy_from_slice(&ix_flags.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(bad)).unwrap();
    assert!(reader.check_cross_frame().is_err());
}

#[test]
fn cross_frame_check_handles_multiple_resets_in_synthetic_audio() {
    let valid = multi_reset_audio_fixture();
    let mut reader = Tnv3Reader::open(Cursor::new(&valid)).unwrap();
    reader.check_cross_frame().unwrap();
    let index = u32::from_le_bytes(valid[40..44].try_into().unwrap()) as usize;
    let second_key = index + 4 * 16;
    assert_eq!(u32::from_le_bytes(valid[second_key + 12..second_key + 16].try_into().unwrap()) >> 8, 3);
}

#[test]
fn cross_frame_check_rejects_excess_tac_before_reading_frame_payloads() {
    let mut bytes = mixed_audio_fixture();
    bytes[72..74].copy_from_slice(&2u16.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-020", .. })));
}

#[test]
fn cross_frame_check_rejects_malformed_picture_header_after_crc_recalculation() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let mut bytes = output.into_inner();
    let len = u32::from_le_bytes(bytes[64..68].try_into().unwrap()) as usize;
    bytes[80] = 3;
    let crc = tau_video::crc32(&bytes[80..64 + len]);
    bytes[76..80].copy_from_slice(&crc.to_le_bytes());
    let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
    assert!(reader.check_cross_frame().is_err());
}

#[test]
fn cross_frame_check_enforces_picture_and_chunk_rate_windows() {
    // At level 3, two 310,000-byte pictures exceed the 600,000-byte picture
    // window while their combined chunk sizes remain below 665,536 bytes.
    let bytes = expanded_rate_fixture(310_000, 30, 0, 55_000, 0, false);
    let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-028", .. })));

    // Two 280,000-byte pictures fit the picture window; two 55,000-byte TAC
    // frames in the first chunk push the chunk window over 665,536 bytes.
    let bytes = expanded_rate_fixture(280_000, 2, 2, 55_000, 0, false);
    let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-029", .. })));

    let exact_picture = expanded_rate_fixture(300_000, 2, 0, 55_000, 0, false);
    Tnv3Reader::open(Cursor::new(exact_picture.clone()))
        .unwrap()
        .check_cross_frame()
        .unwrap();
    let over_picture = expanded_rate_fixture(300_000, 2, 0, 55_000, 0, true);
    let mut reader = Tnv3Reader::open(Cursor::new(over_picture)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-028", .. })));

    let exact_chunk = expanded_rate_fixture(277_700, 2, 2, 55_050, 0, false);
    Tnv3Reader::open(Cursor::new(exact_chunk.clone()))
        .unwrap()
        .check_cross_frame()
        .unwrap();
    let over_chunk = expanded_rate_fixture(277_700, 2, 2, 55_050, 1, false);
    let mut reader = Tnv3Reader::open(Cursor::new(over_chunk)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-029", .. })));
}

#[test]
fn cross_frame_check_uses_ceil_of_fractional_fps_for_window_size() {
    // 24,000/1,001 has a 24-picture window. The first 23 pictures total
    // 184,000 bytes and the 24th makes exactly 200,000, which must pass.
    let exact = fractional_24_frame_rate_fixture(16_000);
    Tnv3Reader::open(Cursor::new(exact.clone()))
        .unwrap()
        .check_cross_frame()
        .unwrap();
    // One extra byte crosses the 24-picture limit, while every trailing
    // 23-picture window remains below 200,000 bytes.
    let over = fractional_24_frame_rate_fixture(16_001);
    let mut reader = Tnv3Reader::open(Cursor::new(over)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-028", .. })));
}

#[test]
fn cross_frame_check_enforces_audio_interleaving_bounds() {
    let mut first = vec![0u8; 5];
    first[1..5].copy_from_slice(&u32::MAX.to_be_bytes());
    let late_audio = audio_fixture(&[first.clone()], &[0, 0, 0, 0, 0, 0, 0, 1], 0);
    let mut reader = Tnv3Reader::open(Cursor::new(late_audio)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-030", .. })));

    let frames = (0..10)
        .map(|n| {
            let mut frame = vec![0u8; 5];
            if n == 0 { frame[1..5].copy_from_slice(&u32::MAX.to_be_bytes()); }
            frame
        })
        .collect::<Vec<_>>();
    let early_audio = audio_fixture(&frames, &[8, 0, 0, 0, 0, 0, 0, 2], 8193);
    let mut reader = Tnv3Reader::open(Cursor::new(early_audio)).unwrap();
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Invalid { code: "E-TNV3-031", .. })));
}

#[test]
fn cross_frame_check_exempts_final_chunk_from_audio_upper_bound() {
    let mut video_reader = Tnv3Reader::open(Cursor::new({
        let mut out = Cursor::new(Vec::new());
        mux_tvc(&include_bytes!("../src/tvc/sequence-reference.tvc")[..], &mut out).unwrap();
        out.into_inner()
    })).unwrap();
    let mut max_display = 0u32;
    let mut counts = vec![0usize; video_reader.info().frame_count as usize];
    let mut stored = 0u32;
    for n in 0..counts.len() - 1 {
        let disp = video_reader.index_entry(n as u32).unwrap().presentation_sample / 2000;
        max_display = max_display.max(disp);
        let samples = u64::from(max_display + 1) * 2000;
        let need = (samples / 1024 + 2).min(100) as u32;
        counts[n] = need.saturating_sub(stored) as usize;
        stored += counts[n] as u32;
    }
    counts[7] = (100 - stored) as usize;
    let mut frames = (0..100).map(|_| vec![0u8; 5]).collect::<Vec<_>>();
    frames[0][1..5].copy_from_slice(&u32::MAX.to_be_bytes());
    let bytes = audio_fixture(&frames, &counts, 99 * 1024);
    let mut reader = Tnv3Reader::open(Cursor::new(&bytes)).unwrap();
    reader.check_cross_frame().unwrap();
    let last = reader.index_entry(7).unwrap();
    let last_audio = u32::from(u16::from_le_bytes(bytes[last.offset as usize + 8..last.offset as usize + 10].try_into().unwrap()));
    let f_last = last.first_tac + last_audio;
    let need_last = (8 * 2000 / 1024 + 2) as u32;
    assert!(f_last > need_last + 4);
}

#[test]
fn cross_frame_check_rejects_invalid_chunk_offsets_before_seeking() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let clean = output.into_inner();
    let index = u32::from_le_bytes(clean[40..44].try_into().unwrap()) as usize;
    let index_offset = u32::from_le_bytes(clean[40..44].try_into().unwrap());
    for bad_offset in [u32::MAX - 3, index_offset - 8] {
        let mut bytes = clean.clone();
        bytes[index..index + 4].copy_from_slice(&bad_offset.to_le_bytes());
        let mut reader = Tnv3Reader::open(Cursor::new(bytes)).unwrap();
        assert!(matches!(
            reader.check_cross_frame(),
            Err(tau_video::Error::Invalid { code: "E-TNV3-027", .. })
        ));
    }
}

struct FailChunkSeek {
    inner: Cursor<Vec<u8>>,
    armed: Rc<Cell<bool>>,
}
impl Read for FailChunkSeek {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}
impl Seek for FailChunkSeek {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if self.armed.get() && position == SeekFrom::Start(64) {
            return Err(io::Error::other("injected chunk seek failure"));
        }
        self.inner.seek(position)
    }
}

#[test]
fn cross_frame_check_preserves_real_underlying_seek_failures() {
    let source = include_bytes!("../src/tvc/sequence-reference.tvc");
    let mut output = Cursor::new(Vec::new());
    mux_tvc(&source[..], &mut output).unwrap();
    let armed = Rc::new(Cell::new(false));
    let input = FailChunkSeek {
        inner: Cursor::new(output.into_inner()),
        armed: armed.clone(),
    };
    let mut reader = Tnv3Reader::open(input).unwrap();
    armed.set(true);
    assert!(matches!(reader.check_cross_frame(), Err(tau_video::Error::Io(_))));
}
