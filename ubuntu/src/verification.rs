use crate::{core::*, platform::Device, session::Session};
use anyhow::{ensure, Result};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub fn self_check() -> Result<()> {
    ensure!(
        cfg!(feature = "watchdog-test-mode"),
        "Self-check requires a watchdog-test-mode build"
    );
    let temp = tempfile::tempdir()?;
    std::env::set_var("ENCRYPT_ASSETS_DATA_DIR", temp.path().join("state"));
    let issuer = Issuer::create()?;
    let source = temp.path().join("source.txt");
    std::fs::write(&source, "Ubuntu offline encryption test\n")?;
    let output = temp.path().join("Content.bin");
    crate::packaging::pack(&source, &issuer, &output)?;
    let request = temp.path().join("device.json");
    crate::packaging::enroll(&output, &request)?;
    let license = temp.path().join("license.lic");
    crate::packaging::issue_file(
        &issuer,
        &output,
        &request,
        &license,
        &crate::issuance::Options {
            days: 3,
            value: 600,
            unit: 0,
        },
    )?;
    let info = Package::open_exe(&output)?.info;
    let device = Device::for_info(&info)?;
    let mut session = Session::new(info);
    let key = session.import(&license, &device, &output)?;
    let mut reader = Reader::new(Package::open_exe(&output)?, key)?;
    ensure!(
        &reader.all(65536)?[..] == std::fs::read(source)?.as_slice(),
        "roundtrip mismatch"
    );
    let old = session
        .state
        .current
        .as_ref()
        .unwrap()
        .activated_at_utc_seconds;
    session.import(&license, &device, &output)?;
    ensure!(
        session
            .state
            .current
            .as_ref()
            .unwrap()
            .activated_at_utc_seconds
            == old,
        "replay reset activation"
    );
    let mut resumed = Session::new(session.info.clone());
    ensure!(resumed.resume(&device)?.is_some(), "resume failed");
    println!("LINUX_ENCRYPTED_PACKAGE_ENROLL_ISSUE_ACTIVATE_RESUME_REPLAY_PASS");
    #[cfg(feature = "watchdog-test-mode")]
    {
        ensure!(session.expire_for_test().is_err(), "expiry not enforced");
        println!("LICENSE_EXPIRY_PASS");
    }
    Ok(())
}
pub fn content(path: &Path, seconds: u64) -> Result<()> {
    ensure!(
        cfg!(feature = "watchdog-test-mode"),
        "Content verification requires a test build"
    );
    let p = Package::open_exe(path)?;
    let d = Device::for_info(&p.info)?;
    let mut session = Session::new(p.info.clone());
    let key = session
        .resume(&d)?
        .ok_or_else(|| anyhow::anyhow!("No valid license"))?;
    let mut reader = Reader::new(p, key)?;
    match reader.package.info.kind.as_str() {
        "media" => {
            let video = crate::packaging::is_video(&reader.package.info.display_name);
            let mut v =
                crate::media::Video::open(reader, Some(Arc::new(Mutex::new(session))), video)?;
            let start = Instant::now();
            let mut last = 0;
            let mut loops = 0;
            let mut progress = 0;
            let mut frames = 0;
            while start.elapsed() < Duration::from_secs(seconds) {
                v.tick()?;
                let t = v.time();
                if t + 200 < last {
                    loops += 1;
                }
                if t > last {
                    progress += 1;
                }
                last = t;
                frames = v.frame.lock().unwrap().serial;
                std::thread::sleep(Duration::from_millis(50));
            }
            let audio = v.audio_progress();
            eprintln!("MEDIA_PROBE state={} time={} length={} frames={} player_repeats={} timeline_wraps={} progress={} audio_blocks={audio}",v.state(),v.time(),v.length(),frames,v.loops,loops,progress);
            ensure!(
                progress > 2 || (!video && audio > 5 && v.state() == 6),
                "Media playback did not advance"
            );
            if video {
                ensure!(frames > 5, "No decoded video frames");
                ensure!(
                    v.loops > 0,
                    "Video did not repeat within the verification interval"
                );
            } else {
                ensure!(audio > 5, "Audio decoder produced no blocks");
            }
            println!("VLC_LICENSED_PLAYBACK_PASS frames={frames} player_repeats={} progress_samples={progress} decoded_audio_blocks={audio}",v.loops);
        }
        "pdf" => {
            let p = crate::documents::Pdf::open(&mut reader)?;
            ensure!(p.pages() > 0, "No PDF pages");
            let surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 800, 600)?;
            let context = gtk::cairo::Context::new(&surface)?;
            p.draw(0, &context, 800, 600);
            println!("PDF_MEMORY_RENDER_PASS pages={}", p.pages());
        }
        "image" => {
            let bytes = reader.all(100 * 1024 * 1024)?;
            let texture = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(&bytes[..]))?;
            use gtk::prelude::TextureExt;
            ensure!(texture.width() > 0, "Image empty");
            println!("IMAGE_MEMORY_DECODE_PASS");
        }
        "text" => {
            ensure!(!reader.all(32 * 1024 * 1024)?.is_empty(), "Text empty");
            println!("TEXT_MEMORY_READ_PASS");
        }
        "presentation" => {
            let mut p = crate::documents::Presentation::open(&mut reader, true)?;
            let started = Instant::now();
            while p.poll()? {
                ensure!(
                    started.elapsed() < Duration::from_secs(55),
                    "PPT test timed out"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        _ => anyhow::bail!("Unsupported kind"),
    }
    Ok(())
}
