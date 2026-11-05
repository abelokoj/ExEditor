# ExEditor dependency compatibility patch

Upstream: https://github.com/bbqsrc/box
Commit: `bff1974bdc3290759a86c5cebd123178a3b882f7`
Original source archive SHA-256: `8fbb1018494219da81a46bc182b55f2dcdb5681c013e4a8134e71bc8a7d7704a`

The only upstream-file change is the workspace `xz2` dependency in `Cargo.toml`: it now aliases `liblzma` 0.4.4, with static native linking and default bindgen disabled. liblzma is the maintained xz2 fork and exposes the same `stream::{Action, Check, Status, Stream}` API used by this crate, including `new_easy_encoder`, `new_stream_decoder`, `process`, `total_in`, and `total_out`. All source-code behavior and supported codecs remain unchanged.

This makes Divvun and Harper's transitive graph use the same `liblzma-sys` package instead of two packages with the conflicting `links = "lzma"` value. It does not enable Harper's optional CUDA/LLVM features or replace its CPU grammar model. The application's Cargo.lock records the exact resolved dependency versions.

Original Apache-2.0 and MIT license texts are retained. This patch has been checked against the wrapper's published source API; compilation and regression tests run only on GitHub after the feature batch is complete.
