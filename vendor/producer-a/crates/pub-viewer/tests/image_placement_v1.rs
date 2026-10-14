#![allow(dead_code)]

// Compile the extracted image owner as the test crate's source module. This
// exercises the exact images.rs implementation and its owned tests without
// building the full pub-viewer lib-test harness.
#[path = "../src/images.rs"]
mod images;
