use std::io::{self, BufRead, Read, Write};

const MAX_LINE: usize = 64 * 1024;
const MAX_C_STREAM_HEADER: usize = 511;
const MAX_C_FRAME_METADATA: usize = 127;
const COPY_BUFFER: usize = 64 * 1024;

/// Reads a YUV4MPEG2 stream and emits one frame group at a time into a caller-owned sink.
///
/// The source is consumed incrementally. Generic mode accepts lines up to 64 KiB. C-compatible
/// mode enforces the reference reader's 511-byte stream-header and 127-byte FRAME-line limits.
/// Memory use is bounded by those lines and a fixed 64 KiB payload copy buffer; frame payloads
/// are never retained. The caller controls each group's destination and can schedule its encoder
/// as soon as `write_next_group` returns. A returned group contains the original stream header
/// followed by each frame's exact metadata line and payload bytes.
pub struct Y4mFrameGroups<R> {
    input: R,
    header: Vec<u8>,
    frame_bytes: u64,
    frames_per_group: usize,
    max_frame_metadata: usize,
    c_compatible: bool,
    total_frames: u64,
    ended: bool,
    failed: bool,
}

impl<R: BufRead> Y4mFrameGroups<R> {
    /// Read and validate the stream header. Only explicit 8-bit 4:2:0 chroma formats are accepted.
    pub fn new(input: R, frames_per_group: usize) -> io::Result<Self> {
        Self::with_line_limits(input, frames_per_group, MAX_LINE, MAX_LINE, false)
    }

    /// Construct a reader whose header and metadata match the C reference parser buffers.
    ///
    /// Lines must fit 511/127 bytes, including newline. Only printable ASCII plus LF is accepted
    /// so C string truncation and tokenization cannot change the interpreted stream.
    pub fn new_c_compatible(input: R, frames_per_group: usize) -> io::Result<Self> {
        Self::with_line_limits(
            input,
            frames_per_group,
            MAX_C_STREAM_HEADER,
            MAX_C_FRAME_METADATA,
            true,
        )
    }

    fn with_line_limits(
        mut input: R,
        frames_per_group: usize,
        max_header_line: usize,
        max_frame_metadata: usize,
        c_compatible: bool,
    ) -> io::Result<Self> {
        if frames_per_group == 0 {
            return Err(invalid("frames per group must be positive"));
        }
        let header = read_line(&mut input, "Y4M stream header", max_header_line)?
            .ok_or_else(|| invalid("empty Y4M input"))?;
        if c_compatible && !c_compatible_line(&header) {
            return Err(invalid("Y4M header contains a C-incompatible byte"));
        }
        let frame_bytes = y4m_420_frame_bytes(&header)?;
        Ok(Self {
            input,
            header,
            frame_bytes,
            frames_per_group,
            max_frame_metadata,
            c_compatible,
            total_frames: 0,
            ended: false,
            failed: false,
        })
    }

    /// Return the original Y4M stream header exactly as read.
    pub fn header(&self) -> &[u8] {
        &self.header
    }

    /// Number of bytes in one 8-bit 4:2:0 frame payload.
    pub fn frame_bytes(&self) -> u64 {
        self.frame_bytes
    }

    /// Stream the next group into `output`, returning its number of frames.
    ///
    /// Returns `Ok(None)` at clean end of stream. On malformed or truncated input, returns an
    /// error. Any error permanently poisons this reader because input or sink state may have
    /// advanced. The caller should discard the current sink because it may contain an incomplete
    /// group. Earlier groups remain complete and usable.
    pub fn write_next_group<W: Write>(&mut self, output: &mut W) -> io::Result<Option<usize>> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Y4M group reader is unusable after a previous error",
            ));
        }
        let result = self.write_next_group_inner(output);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn write_next_group_inner<W: Write>(&mut self, output: &mut W) -> io::Result<Option<usize>> {
        if self.ended {
            return Ok(None);
        }
        let Some(first_metadata) =
            read_line(&mut self.input, "FRAME metadata", self.max_frame_metadata)?
        else {
            self.ended = true;
            return if self.total_frames == 0 {
                Err(invalid("Y4M input contains no frames"))
            } else {
                Ok(None)
            };
        };
        validate_frame_metadata(&first_metadata)?;
        if self.c_compatible && !c_compatible_line(&first_metadata) {
            return Err(invalid("Y4M FRAME metadata contains a C-incompatible byte"));
        }
        output.write_all(&self.header)?;
        let mut written = 0usize;
        let mut metadata = Some(first_metadata);
        while written < self.frames_per_group {
            let frame_header = if let Some(first) = metadata.take() {
                first
            } else {
                match read_line(&mut self.input, "FRAME metadata", self.max_frame_metadata)? {
                    Some(line) => {
                        validate_frame_metadata(&line)?;
                        if self.c_compatible && !c_compatible_line(&line) {
                            return Err(invalid(
                                "Y4M FRAME metadata contains a C-incompatible byte",
                            ));
                        }
                        line
                    }
                    None => {
                        self.ended = true;
                        break;
                    }
                }
            };
            output.write_all(&frame_header)?;
            copy_exact(&mut self.input, output, self.frame_bytes)?;
            written = written
                .checked_add(1)
                .ok_or_else(|| invalid("group frame count overflow"))?;
            self.total_frames = self
                .total_frames
                .checked_add(1)
                .ok_or_else(|| invalid("stream frame count overflow"))?;
        }
        Ok(Some(written))
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }
}

fn read_line<R: BufRead>(
    reader: &mut R,
    context: &str,
    max_len: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("truncated {context} line"),
            ));
        }
        let take = available
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if line
            .len()
            .checked_add(take)
            .filter(|&size| size <= max_len)
            .is_none()
        {
            return Err(invalid_owned(format!("{context} exceeds {max_len} bytes")));
        }
        let complete = available.get(take - 1) == Some(&b'\n');
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if complete {
            return Ok(Some(line));
        }
    }
}

fn y4m_420_frame_bytes(header: &[u8]) -> io::Result<u64> {
    if !header.starts_with(b"YUV4MPEG2 ") || header.last() != Some(&b'\n') {
        return Err(invalid("invalid Y4M stream header"));
    }
    if !header.is_ascii() {
        return Err(invalid("Y4M header must contain ASCII fields"));
    }
    let text =
        std::str::from_utf8(header).map_err(|_| invalid("Y4M header must contain ASCII fields"))?;
    let mut width = None;
    let mut height = None;
    let mut chroma = None;
    for token in text.trim_end_matches('\n').split_ascii_whitespace().skip(1) {
        if let Some(value) = token.strip_prefix('W') {
            if width.is_some() {
                return Err(invalid("duplicate Y4M width"));
            }
            width = Some(parse_dimension(value)?);
        } else if let Some(value) = token.strip_prefix('H') {
            if height.is_some() {
                return Err(invalid("duplicate Y4M height"));
            }
            height = Some(parse_dimension(value)?);
        } else if let Some(value) = token.strip_prefix('C') {
            if chroma.is_some() {
                return Err(invalid("duplicate Y4M chroma format"));
            }
            chroma = Some(value);
        }
    }
    let width = width
        .filter(|&value| value > 0)
        .ok_or_else(|| invalid("missing/zero Y4M width"))?;
    let height = height
        .filter(|&value| value > 0)
        .ok_or_else(|| invalid("missing/zero Y4M height"))?;
    match chroma {
        Some("420" | "420jpeg" | "420mpeg2" | "420paldv") => {}
        Some(_) => return Err(invalid("only explicit 8-bit 4:2:0 Y4M chroma is supported")),
        None => return Err(invalid("missing explicit Y4M chroma format")),
    }
    let luma = width
        .checked_mul(height)
        .ok_or_else(|| invalid("Y4M luma size overflow"))?;
    let chroma_width = width
        .checked_add(1)
        .ok_or_else(|| invalid("Y4M width overflow"))?
        / 2;
    let chroma_height = height
        .checked_add(1)
        .ok_or_else(|| invalid("Y4M height overflow"))?
        / 2;
    let chroma = chroma_width
        .checked_mul(chroma_height)
        .and_then(|samples| samples.checked_mul(2))
        .ok_or_else(|| invalid("Y4M chroma size overflow"))?;
    luma.checked_add(chroma)
        .ok_or_else(|| invalid("Y4M frame size overflow"))
}

fn parse_dimension(value: &str) -> io::Result<u64> {
    value
        .parse::<u64>()
        .map_err(|_| invalid("invalid Y4M dimension"))
}

fn validate_frame_metadata(line: &[u8]) -> io::Result<()> {
    if !line.starts_with(b"FRAME")
        || (line.len() > 5 && !line[5].is_ascii_whitespace() && line[5] != b'\n')
        || line.last() != Some(&b'\n')
    {
        return Err(invalid("invalid Y4M FRAME metadata"));
    }
    Ok(())
}

fn c_compatible_line(line: &[u8]) -> bool {
    line.iter()
        .all(|&byte| byte == b'\n' || (b' '..=b'~').contains(&byte))
}

fn copy_exact<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    mut remaining: u64,
) -> io::Result<()> {
    let mut buffer = [0u8; COPY_BUFFER];
    while remaining > 0 {
        let count = remaining.min(COPY_BUFFER as u64) as usize;
        reader.read_exact(&mut buffer[..count])?;
        writer.write_all(&buffer[..count])?;
        remaining -= count as u64;
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn invalid_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture(frames: usize, width: usize, height: usize) -> Vec<u8> {
        let header =
            format!("YUV4MPEG2 W{width} H{height} F24:1 Ip A1:1 C420jpeg XCOLORRANGE=LIMITED\n");
        let y = width * height;
        let c = width.div_ceil(2) * height.div_ceil(2) * 2;
        let mut bytes = header.into_bytes();
        for frame in 0..frames {
            bytes.extend_from_slice(format!("FRAME Xtimestamp={frame}\n").as_bytes());
            bytes.extend((0..(y + c)).map(|offset| (frame + offset) as u8));
        }
        bytes
    }

    fn collect_groups(bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
        let mut rust = Y4mFrameGroups::new(Cursor::new(bytes), 3).unwrap();
        let mut groups = Vec::new();
        loop {
            let mut group = Vec::new();
            match rust.write_next_group(&mut group).unwrap() {
                Some(count) => groups.push((count, group)),
                None => break,
            }
        }
        assert_eq!(rust.total_frames(), 7);
        groups
    }

    #[test]
    fn generated_groups_preserve_exact_header_metadata_payload_and_boundaries() {
        let bytes = fixture(7, 4, 4);
        let groups = collect_groups(&bytes);
        assert_eq!(
            groups.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            [3, 3, 1]
        );
        let header = b"YUV4MPEG2 W4 H4 F24:1 Ip A1:1 C420jpeg XCOLORRANGE=LIMITED\n";
        let mut expected = Vec::new();
        for (start, end) in [(0usize, 3usize), (3, 6), (6, 7)] {
            let mut group = header.to_vec();
            for frame in start..end {
                group.extend_from_slice(format!("FRAME Xtimestamp={frame}\n").as_bytes());
                group.extend((0..24).map(|offset| (frame + offset) as u8));
            }
            expected.push(group);
        }
        assert_eq!(
            groups
                .into_iter()
                .map(|(_, bytes)| bytes)
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn generated_groups_match_python_when_reference_is_configured() {
        let Some(python) = std::env::var_os("TAU_TEST_PYTHON") else {
            return;
        };
        let bytes = fixture(7, 4, 4);
        let groups = collect_groups(&bytes);
        let python = std::process::Command::new(python)
            .arg("tests/y4m_python_reference.py")
            .arg("3")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("python3 is needed for the differential check");
        let mut python = python;
        python.stdin.take().unwrap().write_all(&bytes).unwrap();
        let output = python.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rust_bytes: Vec<u8> = groups.into_iter().flat_map(|(_, group)| group).collect();
        assert_eq!(rust_bytes, output.stdout);
    }

    #[test]
    fn truncated_metadata_or_payload_fails_and_poisons_reader() {
        let header = b"YUV4MPEG2 W2 H2 F24:1 C420jpeg\n";
        let mut reader = Y4mFrameGroups::new(
            Cursor::new([header.as_slice(), b"FRAME Xok\n12345"].concat()),
            1,
        )
        .unwrap();
        let mut out = Vec::new();
        assert_eq!(
            reader.write_next_group(&mut out).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            reader.write_next_group(&mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let mut reader = Y4mFrameGroups::new(
            Cursor::new([header.as_slice(), b"FRAME Xpartial"].concat()),
            1,
        )
        .unwrap();
        assert_eq!(
            reader.write_next_group(&mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn failing_sink_poisons_reader_after_consuming_group_header() {
        struct BrokenSink;
        impl Write for BrokenSink {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "injected sink failure",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut reader = Y4mFrameGroups::new(Cursor::new(fixture(2, 4, 4)), 1).unwrap();
        assert_eq!(
            reader.write_next_group(&mut BrokenSink).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            reader.write_next_group(&mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(reader.total_frames(), 0);
    }

    #[test]
    fn rejects_non_420_chroma_and_dimension_overflow() {
        for chroma in ["422", "444", "420p10", "420p12", "410", "mono", "420jpeg10"] {
            let header = format!("YUV4MPEG2 W4 H4 C{chroma}\n");
            assert!(
                Y4mFrameGroups::new(Cursor::new(header), 1).is_err(),
                "{chroma}"
            );
        }
        assert!(
            Y4mFrameGroups::new(
                Cursor::new(b"YUV4MPEG2 W18446744073709551615 H2 C420jpeg\n"),
                1
            )
            .is_err()
        );
        assert!(Y4mFrameGroups::new(Cursor::new(b"YUV4MPEG2 W4 H4\n"), 1).is_err());
        assert!(Y4mFrameGroups::new(Cursor::new(b"YUV4MPEG2 W4 H4 C420jpeg\n"), 0).is_err());
    }

    #[test]
    fn c_reference_header_and_frame_line_limits_are_enforced() {
        let prefix = b"YUV4MPEG2 W4 H4 C420jpeg X";
        let mut max_header = prefix.to_vec();
        max_header.resize(MAX_C_STREAM_HEADER - 1, b'x');
        max_header.push(b'\n');
        assert_eq!(max_header.len(), 511);
        assert!(Y4mFrameGroups::new_c_compatible(Cursor::new(max_header), 1).is_ok());

        let mut long_header = prefix.to_vec();
        long_header.resize(MAX_C_STREAM_HEADER, b'x');
        long_header.push(b'\n');
        assert_eq!(long_header.len(), 512);
        assert!(Y4mFrameGroups::new_c_compatible(Cursor::new(long_header), 1).is_err());

        let header = b"YUV4MPEG2 W4 H4 F24:1 C420jpeg\n";
        let mut max_metadata = b"FRAME ".to_vec();
        max_metadata.resize(MAX_C_FRAME_METADATA - 1, b'x');
        max_metadata.push(b'\n');
        assert_eq!(max_metadata.len(), 127);
        let mut input = header.to_vec();
        input.extend_from_slice(&max_metadata);
        input.extend([0u8; 24]);
        let mut reader = Y4mFrameGroups::new_c_compatible(Cursor::new(input), 1).unwrap();
        assert_eq!(reader.write_next_group(&mut Vec::new()).unwrap(), Some(1));

        let mut long_metadata = b"FRAME ".to_vec();
        long_metadata.resize(MAX_C_FRAME_METADATA, b'x');
        long_metadata.push(b'\n');
        assert_eq!(long_metadata.len(), 128);
        let mut input = header.to_vec();
        input.extend_from_slice(&long_metadata);
        input.extend([0u8; 24]);
        let mut reader = Y4mFrameGroups::new_c_compatible(Cursor::new(input), 1).unwrap();
        assert!(reader.write_next_group(&mut Vec::new()).is_err());

        for bad_header in [
            b"YUV4MPEG2 W4\tH4 F24:1 C420jpeg\n".as_slice(),
            b"YUV4MPEG2 W4 H4 F24:1 C420jpeg Xbad\0\n".as_slice(),
        ] {
            assert!(Y4mFrameGroups::new_c_compatible(Cursor::new(bad_header), 1).is_err());
        }
        for bad_metadata in [b"FRAME Xbad\0\n".as_slice(), b"FRAME\tXbad\n".as_slice()] {
            let mut input = header.to_vec();
            input.extend_from_slice(bad_metadata);
            input.extend([0u8; 24]);
            let mut reader = Y4mFrameGroups::new_c_compatible(Cursor::new(input), 1).unwrap();
            assert!(reader.write_next_group(&mut Vec::new()).is_err());
        }
    }

    #[test]
    fn generic_constructor_keeps_previous_long_line_acceptance() {
        let prefix = b"YUV4MPEG2 W4 H4 C420jpeg X";
        let mut header = prefix.to_vec();
        header.resize(MAX_C_STREAM_HEADER + 1, b'x');
        header.push(b'\n');
        assert!(Y4mFrameGroups::new(Cursor::new(header), 1).is_ok());

        let mut input = b"YUV4MPEG2 W4 H4 F24:1 C420jpeg\n".to_vec();
        let mut metadata = b"FRAME ".to_vec();
        metadata.resize(MAX_C_FRAME_METADATA + 1, b'x');
        metadata.push(b'\n');
        input.extend_from_slice(&metadata);
        input.extend([0u8; 24]);
        let mut groups = Y4mFrameGroups::new(Cursor::new(input), 1).unwrap();
        assert_eq!(groups.write_next_group(&mut Vec::new()).unwrap(), Some(1));
    }
}
