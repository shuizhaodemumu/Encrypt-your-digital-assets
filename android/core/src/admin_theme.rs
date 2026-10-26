//! Reuses the Windows/Ubuntu workspace artwork and lavender palette.
use anyhow::{ensure, Result};
use std::ptr;
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*};
pub struct Theme {
    pub dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl Theme {
    pub unsafe fn new(width: i32, height: i32) -> Result<Self> {
        let display = GetDC(0);
        let dc = CreateCompatibleDC(display);
        let bitmap = CreateCompatibleBitmap(display, width, height);
        ReleaseDC(0, display);
        ensure!(dc != 0 && bitmap != 0, "无法创建界面画布");
        let previous = SelectObject(dc, bitmap);
        let theme = Self {
            dc,
            bitmap,
            previous,
        };
        let image =
            image::load_from_memory(include_bytes!("../assets/dragon-side.png"))?.to_rgba8();
        let (w, h) = image.dimensions();
        let mut pixels = image.into_raw();
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
            for (c, b) in p[..3].iter_mut().zip([251u16, 241, 245]) {
                *c = ((*c as u16 * 10 + b * 90) / 100) as u8;
            }
            p[3] = 255;
        }
        let scale = (width as f64 / w as f64).max(height as f64 / h as f64);
        let (dw, dh) = ((w as f64 * scale) as i32, (h as f64 * scale) as i32);
        let info = bitmap_info(w as i32, h as i32);
        SetStretchBltMode(dc, HALFTONE);
        StretchDIBits(
            dc,
            (width - dw) / 2,
            (height - dh) / 2,
            dw,
            dh,
            0,
            0,
            w as i32,
            h as i32,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        panel(dc, 28, 166, 537, 450);
        panel(dc, 587, 166, 537, 450);
        let icon = image::load_from_memory(include_bytes!("../assets/app-icon.png"))?
            .thumbnail(320, 320)
            .to_rgba8();
        let (iw, ih) = icon.dimensions();
        let mut p = icon.into_raw();
        for v in p.chunks_exact_mut(4) {
            v.swap(0, 2);
            let a = v[3] as u16;
            for c in &mut v[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
        let mut bits = ptr::null_mut();
        let bm = CreateDIBSection(
            dc,
            &bitmap_info(iw as i32, ih as i32),
            DIB_RGB_COLORS,
            &mut bits,
            0,
            0,
        );
        let mem = CreateCompatibleDC(dc);
        if bm != 0 && mem != 0 && !bits.is_null() {
            ptr::copy_nonoverlapping(p.as_ptr(), bits.cast(), p.len());
            let old = SelectObject(mem, bm);
            AlphaBlend(
                dc,
                876,
                628,
                154,
                154,
                mem,
                0,
                0,
                iw as i32,
                ih as i32,
                BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                },
            );
            SelectObject(mem, old);
        }
        if mem != 0 {
            DeleteDC(mem);
        }
        if bm != 0 {
            DeleteObject(bm);
        }
        Ok(theme)
    }
    pub unsafe fn copy(&self, to: HDC, x: i32, y: i32, w: i32, h: i32) {
        BitBlt(to, 0, 0, w, h, self.dc, x, y, SRCCOPY);
    }
}
impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
fn bitmap_info(w: i32, h: i32) -> BITMAPINFO {
    unsafe {
        let mut i: BITMAPINFO = std::mem::zeroed();
        i.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        i.bmiHeader.biWidth = w;
        i.bmiHeader.biHeight = -h;
        i.bmiHeader.biPlanes = 1;
        i.bmiHeader.biBitCount = 32;
        i.bmiHeader.biCompression = BI_RGB;
        i
    }
}
pub unsafe fn rounded(dc: HDC, r: &RECT, fill: u32, border: u32, radius: i32) {
    let b = CreateSolidBrush(fill);
    let p = CreatePen(PS_SOLID, 1, border);
    let ob = SelectObject(dc, b);
    let op = SelectObject(dc, p);
    RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, ob);
    SelectObject(dc, op);
    DeleteObject(b);
    DeleteObject(p);
}
unsafe fn panel(dc: HDC, x: i32, y: i32, w: i32, h: i32) {
    let saved = SaveDC(dc);
    let region = CreateRoundRectRgn(x, y, x + w + 1, y + h + 1, 32, 32);
    SelectClipRgn(dc, region);
    let mem = CreateCompatibleDC(dc);
    let bm = CreateCompatibleBitmap(dc, 1, 1);
    let old = SelectObject(mem, bm);
    SetPixel(mem, 0, 0, 0xffffff);
    AlphaBlend(
        dc,
        x,
        y,
        w,
        h,
        mem,
        0,
        0,
        1,
        1,
        BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 204,
            AlphaFormat: 0,
        },
    );
    SelectObject(mem, old);
    DeleteObject(bm);
    DeleteDC(mem);
    RestoreDC(dc, saved);
    DeleteObject(region);
    let pen = CreatePen(PS_SOLID, 1, 0x00E4CCD9);
    let op = SelectObject(dc, pen);
    let ob = SelectObject(dc, GetStockObject(NULL_BRUSH));
    RoundRect(dc, x, y, x + w, y + h, 32, 32);
    SelectObject(dc, ob);
    SelectObject(dc, op);
    DeleteObject(pen);
}
