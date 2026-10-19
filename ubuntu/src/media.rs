use crate::{
    core::Reader,
    session::{authorized, Authorization},
};
use anyhow::{ensure, Result};
use std::{
    ffi::{c_char, c_void, CString},
    ptr,
    sync::{Mutex, MutexGuard},
};

type P = *mut c_void;
type Open = unsafe extern "C" fn(P, *mut P, *mut u64) -> i32;
type Read = unsafe extern "C" fn(P, P, usize) -> isize;
type Seek = unsafe extern "C" fn(P, u64) -> i32;
type Close = unsafe extern "C" fn(P);
#[repr(C)]
#[derive(Default)]
struct MediaStats {
    read_bytes: i32,
    input_bitrate: f32,
    demux_read_bytes: i32,
    demux_bitrate: f32,
    demux_corrupted: i32,
    demux_discontinuity: i32,
    decoded_video: i32,
    decoded_audio: i32,
    displayed_pictures: i32,
    lost_pictures: i32,
    played_abuffers: i32,
    lost_abuffers: i32,
    sent_packets: i32,
    sent_bytes: i32,
    send_bitrate: f32,
}
#[link(name = "vlc")]
extern "C" {
    fn libvlc_new(argc: i32, argv: *const *const c_char) -> P;
    fn libvlc_release(instance: P);
    fn libvlc_media_new_callbacks(
        i: P,
        open: Option<Open>,
        read: Option<Read>,
        seek: Option<Seek>,
        close: Option<Close>,
        data: P,
    ) -> P;
    fn libvlc_media_release(media: P);
    fn libvlc_media_add_option(media: P, option: *const c_char);
    fn libvlc_media_get_stats(media: P, stats: *mut MediaStats) -> i32;
    fn libvlc_media_player_new_from_media(media: P) -> P;
    fn libvlc_media_player_release(player: P);
    fn libvlc_media_player_play(player: P) -> i32;
    fn libvlc_media_player_stop(player: P);
    fn libvlc_media_player_pause(player: P);
    fn libvlc_media_player_get_state(player: P) -> i32;
    fn libvlc_media_player_get_time(player: P) -> i64;
    fn libvlc_media_player_set_time(player: P, time: i64);
    fn libvlc_media_player_get_length(player: P) -> i64;
    fn libvlc_audio_set_volume(player: P, volume: i32) -> i32;
    fn libvlc_video_set_callbacks(
        player: P,
        lock: Option<unsafe extern "C" fn(P, *mut P) -> P>,
        unlock: Option<unsafe extern "C" fn(P, P, *const P)>,
        display: Option<unsafe extern "C" fn(P, P)>,
        data: P,
    );
    fn libvlc_video_set_format_callbacks(
        player: P,
        format: Option<
            unsafe extern "C" fn(
                *mut P,
                *mut c_char,
                *mut u32,
                *mut u32,
                *mut u32,
                *mut u32,
            ) -> u32,
        >,
        cleanup: Option<unsafe extern "C" fn(P)>,
    );
}
struct Input {
    reader: Mutex<Reader>,
    auth: Option<Authorization>,
}
#[derive(Default)]
pub struct Frame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub serial: u64,
    pub stride: u32,
    offset: usize,
}
impl Frame {
    pub fn bytes(&self) -> &[u8] {
        &self.pixels[self.offset..self.offset + (self.stride as usize) * (self.height as usize)]
    }
}
unsafe extern "C" fn input_open(opaque: P, data: *mut P, size: *mut u64) -> i32 {
    let i = &*(opaque as *const Input);
    if authorized(&i.auth).is_err() {
        return -1;
    }
    match i.reader.lock() {
        Ok(mut r) => {
            r.position = 0;
            *size = r.package.info.plain_length;
            *data = opaque;
            0
        }
        Err(_) => -1,
    }
}
unsafe extern "C" fn input_read(opaque: P, out: P, size: usize) -> isize {
    let i = &*(opaque as *const Input);
    if authorized(&i.auth).is_err() || size > 16 * 1024 * 1024 {
        return -1;
    }
    match i.reader.lock() {
        Ok(mut r) => r
            .read(std::slice::from_raw_parts_mut(out as *mut u8, size))
            .map(|n| n as isize)
            .unwrap_or(-1),
        Err(_) => -1,
    }
}
unsafe extern "C" fn input_seek(opaque: P, pos: u64) -> i32 {
    let i = &*(opaque as *const Input);
    if authorized(&i.auth).is_err() {
        return -1;
    }
    match i.reader.lock() {
        Ok(mut r) => {
            if r.seek(pos).is_ok() {
                0
            } else {
                -1
            }
        }
        Err(_) => -1,
    }
}
unsafe extern "C" fn input_close(_: P) {}
unsafe extern "C" fn format(
    opaque: *mut P,
    chroma: *mut c_char,
    width: *mut u32,
    height: *mut u32,
    pitches: *mut u32,
    lines: *mut u32,
) -> u32 {
    if *width == 0 || *height == 0 || *width > 16384 || *height > 16384 {
        return 0;
    }
    let scale = (*width as f64 / 1920.0)
        .max(*height as f64 / 1080.0)
        .max(1.0);
    *width = (*width as f64 / scale).round().max(1.0) as u32;
    *height = (*height as f64 / scale).round().max(1.0) as u32;
    ptr::copy_nonoverlapping(b"RV32".as_ptr(), chroma as *mut u8, 4);
    *pitches = (*width * 4).div_ceil(32) * 32;
    *lines = *height;
    let m = &*(*opaque as *const Mutex<Frame>);
    if let Ok(mut f) = m.lock() {
        f.width = *width;
        f.height = *height;
        f.stride = *pitches;
        f.pixels = vec![0; (*pitches as usize) * (*height as usize) + 31];
        f.offset = (32 - f.pixels.as_ptr() as usize % 32) % 32;
        1
    } else {
        0
    }
}
unsafe extern "C" fn frame_lock(opaque: P, planes: *mut P) -> P {
    let m = &*(opaque as *const Mutex<Frame>);
    let mut guard = match m.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    *planes = guard.pixels.as_mut_ptr().add(guard.offset) as P;
    // LibVLC returns this picture handle to unlock before destroying the player.
    Box::into_raw(Box::new(guard)) as P
}
unsafe extern "C" fn frame_unlock(_: P, picture: P, _: *const P) {
    let mut guard: Box<MutexGuard<'_, Frame>> =
        Box::from_raw(picture as *mut MutexGuard<'_, Frame>);
    let offset = guard.offset;
    let len = guard.stride as usize * guard.height as usize;
    for px in guard.pixels[offset..offset + len].chunks_exact_mut(4) {
        px[3] = 255;
    }
    guard.serial = guard.serial.wrapping_add(1);
}
pub struct Video {
    instance: P,
    media: P,
    player: P,
    _input: Box<Input>,
    pub frame: Box<Mutex<Frame>>,
    pub repeat: bool,
    pub loops: u64,
}
impl Video {
    pub fn open(reader: Reader, auth: Option<Authorization>, repeat: bool) -> Result<Self> {
        unsafe {
            let args: Vec<CString> = [
                "--no-video-title-show",
                "--no-snapshot-preview",
                "--no-osd",
                "--avcodec-hw=none",
                "--ignore-config",
                "--quiet",
            ]
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();
            let argv: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
            let instance = libvlc_new(argv.len() as i32, argv.as_ptr());
            ensure!(!instance.is_null(), "VLC 初始化失败，请安装 VLC 运行库");
            let is_mov = std::path::Path::new(&reader.package.info.display_name)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mov" | "mp4" | "m4a"));
            let input = Box::new(Input {
                reader: Mutex::new(reader),
                auth,
            });
            let media = libvlc_media_new_callbacks(
                instance,
                Some(input_open),
                Some(input_read),
                Some(input_seek),
                Some(input_close),
                (&*input as *const Input) as P,
            );
            if media.is_null() {
                libvlc_release(instance);
                anyhow::bail!("VLC 无法打开加密内容");
            }
            // The Linux callback output retains the last texture at EOF. Restart only
            // this player pipeline, keeping the process and GTK window unchanged.
            if is_mov {
                let option = CString::new(":demux=avformat")?;
                libvlc_media_add_option(media, option.as_ptr());
            }
            let player = libvlc_media_player_new_from_media(media);
            if player.is_null() {
                libvlc_media_release(media);
                libvlc_release(instance);
                anyhow::bail!("VLC 播放器创建失败");
            }
            let frame = Box::new(Mutex::new(Frame::default()));
            libvlc_video_set_callbacks(
                player,
                Some(frame_lock),
                Some(frame_unlock),
                None,
                (&*frame as *const Mutex<Frame>) as P,
            );
            libvlc_video_set_format_callbacks(player, Some(format), None);
            let v = Self {
                instance,
                media,
                player,
                _input: input,
                frame,
                repeat,
                loops: 0,
            };
            ensure!(libvlc_media_player_play(v.player) == 0, "VLC 播放失败");
            Ok(v)
        }
    }
    pub fn state(&self) -> i32 {
        unsafe { libvlc_media_player_get_state(self.player) }
    }
    pub fn time(&self) -> i64 {
        unsafe { libvlc_media_player_get_time(self.player) }
    }
    pub fn length(&self) -> i64 {
        unsafe { libvlc_media_player_get_length(self.player) }
    }
    pub fn audio_progress(&self) -> i32 {
        let mut stats = MediaStats::default();
        unsafe {
            libvlc_media_get_stats(self.media, &mut stats);
        }
        stats.decoded_audio
    }
    pub fn pause(&self) {
        unsafe {
            if self.state() == 6 {
                libvlc_media_player_stop(self.player);
                libvlc_media_player_play(self.player);
            } else {
                libvlc_media_player_pause(self.player);
            }
        }
    }
    pub fn seek(&self, time: i64) {
        unsafe { libvlc_media_player_set_time(self.player, time.max(0).min(self.length().max(0))) }
    }
    pub fn volume(&self, v: i32) {
        unsafe {
            libvlc_audio_set_volume(self.player, v.clamp(0, 100));
        }
    }
    pub fn tick(&mut self) -> Result<()> {
        ensure!(self.state() != 7, "VLC 解码失败");
        if self.state() == 6 && self.repeat {
            unsafe {
                libvlc_media_player_stop(self.player);
                ensure!(
                    libvlc_media_player_play(self.player) == 0,
                    "VLC 循环播放失败"
                );
            }
            self.loops += 1;
        }
        Ok(())
    }
}
impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            libvlc_media_player_stop(self.player);
            libvlc_media_player_release(self.player);
            libvlc_media_release(self.media);
            libvlc_release(self.instance);
        }
    }
}
