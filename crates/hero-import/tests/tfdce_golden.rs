//! TF-DCE checks against a real install (the Korean DOS/V build's `GAME` folder).
//!
//! Runs only when `EIKETSU_ORIGINAL_DIR` is set, and is skipped otherwise, so CI and the
//! repository never need original bytes:
//!
//! ```text
//! EIKETSU_ORIGINAL_DIR=/path/to/GAME cargo test -p hero-import --test tfdce_golden -- --nocapture
//! ```
//!
//! Every image must consume its input exactly and have the size its header gives. The known
//! answers are counts and geometry observed with this decoder and confirmed by looking at the
//! decoded images (240 recognisable faces; 38 screens and illustrations).

use hero_import::install::InstallDir;
use hero_import::{planar, table6, tfdce};

const ENV: &str = "EIKETSU_ORIGINAL_DIR";

fn install() -> Option<InstallDir> {
    let dir = std::env::var_os(ENV).filter(|v| !v.is_empty());
    let Some(dir) = dir else {
        eprintln!("skipped: set {ENV} to the folder holding FACEDAT.R3");
        return None;
    };
    Some(InstallDir::open(std::path::Path::new(&dir)).expect("install folder is readable"))
}

/// Decode every entry of a 6-byte-table file; returns each image's (width, height).
fn decode_all(install: &InstallDir, name: &str) -> Vec<tfdce::Image> {
    let data = install
        .read(name)
        .expect("readable")
        .unwrap_or_else(|| panic!("{name} missing"));
    let table = table6::Table6::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
    (0..table.len())
        .map(|i| {
            let payload = table.get(i).unwrap();
            let header = tfdce::parse_header(payload).unwrap();
            assert_eq!(header.tag, b"T", "{name} entry {i}: header tag");
            let image = tfdce::decode(payload).unwrap_or_else(|e| panic!("{name} entry {i}: {e}"));
            assert_eq!(
                image.planar.len(),
                planar::planar_len(image.width, image.height),
                "{name} entry {i}: decoded length"
            );
            image
        })
        .collect()
}

#[test]
fn portraits_decode_exactly() {
    let Some(install) = install() else { return };
    let faces = decode_all(&install, "FACEDAT.R3");
    assert_eq!(faces.len(), 240, "FACEDAT.R3 entry count");
    for (i, face) in faces.iter().enumerate() {
        assert_eq!((face.width, face.height), (64, 80), "face {i} size");
        assert_eq!(face.planar.len(), 2560, "face {i} decoded length");
        // The portraits are 8-colour images: plane 3 is always cleared.
        assert!(
            face.planar[3 * 640..].iter().all(|&b| b == 0),
            "face {i}: plane 3 is not empty"
        );
    }
}

#[test]
fn packgrp_decodes_exactly() {
    let Some(install) = install() else { return };
    let images = decode_all(&install, "PACKGRP.R3");
    assert_eq!(images.len(), 38, "PACKGRP.R3 entry count");
    let sizes: Vec<(usize, usize)> = images.iter().map(|i| (i.width, i.height)).collect();
    let mut expected = vec![(640, 400), (640, 400), (512, 320)];
    expected.extend([(224, 144); 31]);
    expected.extend([(208, 112), (208, 24), (272, 40), (176, 112)]);
    assert_eq!(sizes, expected, "PACKGRP.R3 image sizes");
    for (i, img) in images.iter().enumerate() {
        eprintln!(
            "PACKGRP {i:2}: {}x{} sha256 {}",
            img.width,
            img.height,
            &hero_import::sha256_hex(&img.planar)[..16]
        );
    }
}
