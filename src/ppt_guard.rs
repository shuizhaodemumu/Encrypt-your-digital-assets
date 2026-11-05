//! Apartment-bound, presentation-scoped PowerPoint event guard. Not a DRM boundary.
use crate::automation::{Dispatch, Value};
use anyhow::{ensure, Result};
use std::{
    cell::Cell,
    ffi::c_void,
    ptr,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};
use windows_sys::{
    core::GUID,
    Win32::System::{
        Com::{DISPPARAMS, EXCEPINFO},
        Variant::{VARIANT, VT_BOOL, VT_BYREF, VT_DISPATCH},
    },
};
const UNKNOWN: GUID = GUID {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const DISPATCH: GUID = GUID {
    data1: 0x20400,
    ..UNKNOWN
};
const CONTAINER: GUID = GUID {
    data1: 0xb196b284,
    data2: 0xbab4,
    data3: 0x101a,
    data4: [0xb6, 0x9c, 0, 0xaa, 0, 0x34, 0x1d, 7],
};
const EVENTS: GUID = GUID {
    data1: 0x914934c2,
    data2: 0x5a91,
    data3: 0x11cf,
    data4: [0x87, 0, 0, 0xaa, 0, 0x60, 0x26, 0x3b],
};
type Query = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
type Ref = unsafe extern "system" fn(*mut c_void) -> u32;
#[repr(C)]
struct UnknownTable {
    query: Query,
    add: Ref,
    release: Ref,
}
unsafe fn query(raw: *mut c_void, id: &GUID) -> Result<*mut c_void> {
    let t = &**(raw as *const *const UnknownTable);
    let mut out = ptr::null_mut();
    let hr = (t.query)(raw, id, &mut out);
    ensure!(
        hr >= 0 && !out.is_null(),
        "PowerPoint 事件接口不可用：{hr:08X}"
    );
    Ok(out)
}
unsafe fn release(raw: *mut c_void) {
    if !raw.is_null() {
        let t = &**(raw as *const *const UnknownTable);
        (t.release)(raw);
    }
}
#[repr(C)]
struct ContainerTable {
    base: UnknownTable,
    enumerate: usize,
    find: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
}
#[repr(C)]
struct PointTable {
    base: UnknownTable,
    interface: usize,
    container: usize,
    advise: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut u32) -> i32,
    unadvise: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    enumerate: usize,
}
#[derive(Default)]
struct State {
    ended: Cell<bool>,
    closing: Cell<bool>,
    blocked: Cell<u32>,
}
#[repr(C)]
struct Sink {
    table: *const SinkTable,
    refs: AtomicU32,
    identity: *mut c_void,
    pres: Dispatch,
    state: Rc<State>,
}
#[repr(C)]
struct SinkTable {
    base: UnknownTable,
    count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    info: unsafe extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> i32,
    ids: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        *const *const u16,
        u32,
        u32,
        *mut i32,
    ) -> i32,
    invoke: unsafe extern "system" fn(
        *mut c_void,
        i32,
        *const GUID,
        u32,
        u16,
        *mut DISPPARAMS,
        *mut VARIANT,
        *mut EXCEPINFO,
        *mut u32,
    ) -> i32,
}
fn equal(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}
unsafe extern "system" fn qi(raw: *mut c_void, id: *const GUID, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return 0x80004003u32 as i32;
    }
    *out = ptr::null_mut();
    if id.is_null() {
        return 0x80004003u32 as i32;
    }
    if [UNKNOWN, DISPATCH, EVENTS].iter().any(|v| equal(v, &*id)) {
        *out = raw;
        add(raw);
        0
    } else {
        0x80004002u32 as i32
    }
}
unsafe extern "system" fn add(raw: *mut c_void) -> u32 {
    (*(raw as *mut Sink)).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn drop_ref(raw: *mut c_void) -> u32 {
    let n = (*(raw as *mut Sink)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        let sink = Box::from_raw(raw as *mut Sink);
        release(sink.identity);
        drop(sink);
    }
    n
}
unsafe extern "system" fn count(_: *mut c_void, n: *mut u32) -> i32 {
    if !n.is_null() {
        *n = 0;
    }
    0
}
unsafe extern "system" fn info(_: *mut c_void, _: u32, _: u32, out: *mut *mut c_void) -> i32 {
    if !out.is_null() {
        *out = ptr::null_mut();
    }
    0x80004001u32 as i32
}
unsafe extern "system" fn ids(
    _: *mut c_void,
    _: *const GUID,
    _: *const *const u16,
    _: u32,
    _: u32,
    _: *mut i32,
) -> i32 {
    0x80020006u32 as i32
}
unsafe extern "system" fn invoke(
    raw: *mut c_void,
    id: i32,
    _: *const GUID,
    _: u32,
    _: u16,
    params: *mut DISPPARAMS,
    _: *mut VARIANT,
    _: *mut EXCEPINFO,
    _: *mut u32,
) -> i32 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if params.is_null() || !matches!(id, 2014 | 2018 | 2025) {
            return;
        }
        let sink = &*(raw as *const Sink);
        let p = &*params;
        let index = if id == 2014 { 0 } else { 1 };
        if p.cArgs <= index || p.rgvarg.is_null() {
            return;
        }
        let arg = (*p.rgvarg.add(index as usize)).Anonymous.Anonymous;
        if arg.vt != VT_DISPATCH || arg.Anonymous.pdispVal.is_null() {
            return;
        }
        let Ok(identity) = query(arg.Anonymous.pdispVal, &UNKNOWN) else {
            return;
        };
        let matches = identity == sink.identity;
        release(identity);
        if !matches {
            return;
        }
        if id == 2018 || (id == 2025 && !sink.state.closing.get()) {
            let cancel = (*p.rgvarg).Anonymous.Anonymous;
            if cancel.vt == (VT_BYREF | VT_BOOL) && !cancel.Anonymous.pboolVal.is_null() {
                *cancel.Anonymous.pboolVal = -1;
            }
            if id == 2018 {
                sink.state.blocked.set(sink.state.blocked.get() + 1);
            } else {
                sink.state.ended.set(true);
            }
        }
        if id == 2014 {
            let _ = sink.pres.set("Saved", Value::integer(-1));
            sink.state.ended.set(true);
        }
    }));
    if result.is_ok() {
        0
    } else {
        0x80004005u32 as i32
    }
}
static TABLE: SinkTable = SinkTable {
    base: UnknownTable {
        query: qi,
        add,
        release: drop_ref,
    },
    count,
    info,
    ids,
    invoke,
};
pub struct Guard {
    point: *mut c_void,
    cookie: u32,
    sink: *mut c_void,
    state: Rc<State>,
}
impl Guard {
    pub fn attach(app: &Dispatch, pres: &Dispatch) -> Result<Self> {
        unsafe {
            let container = query(app.raw(), &CONTAINER)?;
            let table = &**(container as *const *const ContainerTable);
            let mut point = ptr::null_mut();
            let hr = (table.find)(container, &EVENTS, &mut point);
            release(container);
            ensure!(hr >= 0 && !point.is_null(), "PowerPoint 无法订阅保护事件");
            let identity = match query(pres.raw(), &UNKNOWN) {
                Ok(p) => p,
                Err(e) => {
                    release(point);
                    return Err(e);
                }
            };
            let state = Rc::new(State::default());
            let sink = Box::into_raw(Box::new(Sink {
                table: &TABLE,
                refs: AtomicU32::new(1),
                identity,
                pres: Dispatch::retained(pres.raw()),
                state: state.clone(),
            })) as *mut c_void;
            let table = &**(point as *const *const PointTable);
            let mut cookie = 0;
            let hr = (table.advise)(point, sink, &mut cookie);
            if hr < 0 {
                drop_ref(sink);
                release(point);
                anyhow::bail!("PowerPoint 保存保护初始化失败：{hr:08X}");
            }
            Ok(Self {
                point,
                cookie,
                sink,
                state,
            })
        }
    }
    pub fn ended(&self) -> bool {
        self.state.ended.get()
    }
    pub fn allow_close(&self) {
        self.state.closing.set(true);
    }
    #[cfg(test)]
    pub fn blocked(&self) -> u32 {
        self.state.blocked.get()
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let table = &**(self.point as *const *const PointTable);
            (table.unadvise)(self.point, self.cookie);
            release(self.point);
            drop_ref(self.sink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::Apartment;
    #[test]
    #[ignore = "requires installed PowerPoint and interactive desktop"]
    fn real_powerpoint_save_guard_is_scoped() -> Result<()> {
        let _apartment = Apartment::new()?;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("work/windows-2.5.2-20261104")
            .join(format!("save-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let app = Dispatch::create("PowerPoint.Application", true)?;
        let presentations = app.get("Presentations")?.object()?;
        let pres = presentations
            .method("Add", vec![Value::integer(0)])?
            .object()?;
        pres.get("Slides")?
            .object()?
            .method("Add", vec![Value::integer(1), Value::integer(12)])?;
        pres.method(
            "SaveAs",
            vec![
                Value::string(&dir.join("original.pptx").to_string_lossy())?,
                Value::integer(24),
            ],
        )?;
        let other = presentations
            .method("Add", vec![Value::integer(0)])?
            .object()?;
        other
            .get("Slides")?
            .object()?
            .method("Add", vec![Value::integer(1), Value::integer(12)])?;
        let guard = Guard::attach(&app, &pres)?;
        let original = std::fs::read(dir.join("original.pptx"))?;
        pres.set("Saved", Value::integer(0))?;
        let save = pres.method("Save", vec![]);
        let original_unchanged = std::fs::read(dir.join("original.pptx"))? == original;
        let saved = pres.method(
            "SaveAs",
            vec![
                Value::string(&dir.join("forbidden.pptx").to_string_lossy())?,
                Value::integer(24),
            ],
        );
        Apartment::pump();
        let blocked = guard.blocked();
        let forbidden = dir.join("forbidden.pptx").exists();
        let other_result = other.method(
            "SaveAs",
            vec![
                Value::string(&dir.join("unrelated.pptx").to_string_lossy())?,
                Value::integer(24),
            ],
        );
        let unrelated = dir.join("unrelated.pptx").exists();
        pres.set("Saved", Value::integer(-1))?;
        guard.allow_close();
        pres.method("Close", vec![])?;
        drop(guard);
        other.set("Saved", Value::integer(-1))?;
        other.method("Close", vec![])?;
        if presentations.get("Count")?.number()? == 0 {
            app.method("Quit", vec![])?;
        }
        println!("SAVE_GUARD blocked={blocked} forbidden_file={forbidden} unrelated_saved={unrelated} save_returned_error={}",saved.is_err());
        ensure!(
            blocked >= 2 && !forbidden && save.is_err() && original_unchanged,
            "受保护文稿保存未拦截"
        );
        other_result?;
        ensure!(unrelated, "其他文稿保存受影响");
        Ok(())
    }
}
