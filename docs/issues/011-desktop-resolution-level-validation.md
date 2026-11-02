# 011: Desktop output dimensions and codec level validation

The desktop app previously fixed the output bounding box to 400×360 and chose
its level automatically, preventing higher-resolution evaluation. Version
0.4.0 permits preset or custom even output bounds (up to 800×720) and selects
an automatic or explicit TVC level in either C workflow. The experimental pure
Rust encoder continues to select its own level. Both C workflows reject a
level incompatible with the actual aspect-preserved dimensions. The C encoder
may raise per-picture QP for rate limits and fails validation if it cannot
produce a conforming stream; retry counts do not fully measure quality loss.

**Host:** short generated 4:3 inputs encoded with both C workflows at levels
1, 2 and 3, with TNV3 dimensions and level checked. A level-1/800×720 request
was rejected. This is not a full-film test or evidence of Pocket playback at
higher levels. The safe default is the prior 400×360 automatic-level setting;
validate the exact output on the target before relying on it. Next gate: test
representative full-length clips on Pocket and assess quality and performance.
