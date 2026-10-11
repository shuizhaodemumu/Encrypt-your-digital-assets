//! Small, apartment-bound IDispatch adapter. PowerPoint itself remains an external dependency.
use crate::platform::wide;
use anyhow::{ensure, Result};
use std::{ffi::c_void, marker::PhantomData, ptr, rc::Rc};
use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::*,
        System::{Com::*, Variant::*},
    },
};
const IID_NULL: GUID = GUID {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0; 8],
};
const IID_DISPATCH: GUID = GUID {
    data1: 0x00020400,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
#[repr(C)]
struct Vtable {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    type_count: usize,
    type_info: usize,
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
pub struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    pub fn new() -> Result<Self> {
        unsafe {
            let hr = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
            ensure!(hr >= 0, "COM 初始化失败：0x{:08X}", hr as u32);
        }
        Ok(Self(PhantomData))
    }
    pub fn pump() {
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let mut msg = std::mem::zeroed();
            while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
pub struct Value(VARIANT);
impl Value {
    pub fn empty() -> Self {
        Self(unsafe { std::mem::zeroed() })
    }
    pub fn integer(value: i32) -> Self {
        let mut v = Self::empty();
        v.0.Anonymous.Anonymous.vt = VT_I4;
        v.0.Anonymous.Anonymous.Anonymous.lVal = value;
        v
    }
    #[cfg(test)]
    pub fn boolean(value: bool) -> Self {
        let mut v = Self::empty();
        v.0.Anonymous.Anonymous.vt = VT_BOOL;
        v.0.Anonymous.Anonymous.Anonymous.boolVal = if value { -1 } else { 0 };
        v
    }
    pub fn string(value: &str) -> Result<Self> {
        let mut v = Self::empty();
        unsafe {
            let b = SysAllocString(wide(value).as_ptr());
            ensure!(!b.is_null(), "无法分配 COM 字符串");
            v.0.Anonymous.Anonymous.vt = VT_BSTR;
            v.0.Anonymous.Anonymous.Anonymous.bstrVal = b;
        }
        Ok(v)
    }
    pub fn number(&self) -> Result<i32> {
        unsafe {
            let v = self.0.Anonymous.Anonymous;
            Ok(match v.vt {
                VT_I4 | VT_INT => v.Anonymous.lVal,
                VT_UI4 | VT_UINT => v.Anonymous.ulVal as i32,
                VT_I2 => v.Anonymous.iVal as i32,
                VT_BOOL => v.Anonymous.boolVal as i32,
                _ => anyhow::bail!("COM 返回类型不是整数：{}", v.vt),
            })
        }
    }
    pub fn object(mut self) -> Result<Dispatch> {
        unsafe {
            ensure!(
                self.0.Anonymous.Anonymous.vt == VT_DISPATCH,
                "COM 未返回对象"
            );
            let raw = self.0.Anonymous.Anonymous.Anonymous.pdispVal;
            ensure!(!raw.is_null(), "COM 返回空对象");
            self.0.Anonymous.Anonymous.vt = VT_EMPTY;
            Ok(Dispatch(raw, PhantomData))
        }
    }
}
impl Drop for Value {
    fn drop(&mut self) {
        unsafe {
            VariantClear(&mut self.0);
        }
    }
}
pub struct Dispatch(*mut c_void, PhantomData<Rc<()>>);
impl Dispatch {
    pub fn create(prog_id: &str, local: bool) -> Result<Self> {
        unsafe {
            let mut clsid = IID_NULL;
            let hr = CLSIDFromProgIDEx(wide(prog_id).as_ptr(), &mut clsid);
            ensure!(hr >= 0, "未安装或未注册 {}", prog_id);
            let mut raw = ptr::null_mut();
            let hr = CoCreateInstance(
                &clsid,
                ptr::null_mut(),
                if local {
                    CLSCTX_LOCAL_SERVER
                } else {
                    CLSCTX_INPROC_SERVER
                },
                &IID_DISPATCH,
                &mut raw,
            );
            ensure!(
                hr >= 0 && !raw.is_null(),
                "无法启动 {}：0x{:08X}",
                prog_id,
                hr as u32
            );
            Ok(Self(raw, PhantomData))
        }
    }
    pub fn registered(prog_id: &str) -> bool {
        unsafe {
            let mut clsid = IID_NULL;
            CLSIDFromProgIDEx(wide(prog_id).as_ptr(), &mut clsid) >= 0
        }
    }
    fn call(&self, name: &str, flags: u16, mut args: Vec<Value>) -> Result<Value> {
        unsafe {
            let table = &**(self.0 as *const *const Vtable);
            let string = wide(name);
            let pointer = string.as_ptr();
            let mut id = 0;
            let hr = (table.ids)(self.0, &IID_NULL, &pointer, 1, 0x400, &mut id);
            ensure!(
                hr >= 0,
                "PowerPoint 接口不支持 {}：0x{:08X}",
                name,
                hr as u32
            );
            args.reverse();
            let mut raw: Vec<VARIANT> = args.iter().map(|v| v.0).collect();
            let mut named = -3;
            let put = flags == 4;
            let mut parameters = DISPPARAMS {
                rgvarg: raw.as_mut_ptr(),
                rgdispidNamedArgs: if put { &mut named } else { ptr::null_mut() },
                cArgs: raw.len() as u32,
                cNamedArgs: if put { 1 } else { 0 },
            };
            let mut output = Value::empty();
            let mut exception: EXCEPINFO = std::mem::zeroed();
            let mut argument_error = 0;
            let hr = (table.invoke)(
                self.0,
                id,
                &IID_NULL,
                0x400,
                flags,
                &mut parameters,
                &mut output.0,
                &mut exception,
                &mut argument_error,
            );
            if hr < 0 {
                if let Some(fill) = exception.pfnDeferredFillIn {
                    fill(&mut exception);
                }
            }
            let description = if exception.bstrDescription.is_null() {
                String::new()
            } else {
                String::from_utf16_lossy(std::slice::from_raw_parts(
                    exception.bstrDescription,
                    SysStringLen(exception.bstrDescription) as usize,
                ))
            };
            for b in [
                exception.bstrSource,
                exception.bstrDescription,
                exception.bstrHelpFile,
            ] {
                if !b.is_null() {
                    SysFreeString(b);
                }
            }
            ensure!(
                hr >= 0,
                "PowerPoint {} 失败：0x{:08X} {}",
                name,
                hr as u32,
                description
            );
            Ok(output)
        }
    }
    pub fn get(&self, name: &str) -> Result<Value> {
        self.call(name, 2, vec![])
    }
    pub fn set(&self, name: &str, value: Value) -> Result<()> {
        self.call(name, 4, vec![value])?;
        Ok(())
    }
    pub fn method(&self, name: &str, args: Vec<Value>) -> Result<Value> {
        self.call(name, 1, args)
    }
}
impl Drop for Dispatch {
    fn drop(&mut self) {
        unsafe {
            let table = &**(self.0 as *const *const Vtable);
            (table.release)(self.0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_windows_dispatch_arguments_variants_and_errors() -> Result<()> {
        let _apartment = Apartment::new()?;
        let dictionary = Dispatch::create("Scripting.Dictionary", false)?;
        dictionary.method("Add", vec![Value::string("中文 key")?, Value::integer(42)])?;
        assert_eq!(dictionary.get("Count")?.number()?, 1);
        assert_ne!(
            dictionary
                .method("Exists", vec![Value::string("中文 key")?])?
                .number()?,
            0
        );
        assert_eq!(
            dictionary
                .call("Item", 2, vec![Value::string("中文 key")?])?
                .number()?,
            42
        );
        dictionary.method("Add", vec![Value::string("bool")?, Value::boolean(true)])?;
        assert_eq!(
            dictionary
                .call("Item", 2, vec![Value::string("bool")?])?
                .number()?,
            -1
        );
        assert!(dictionary
            .method("Add", vec![Value::string("中文 key")?, Value::integer(3)])
            .is_err());
        assert!(dictionary.get("NoSuchProperty").is_err());
        Ok(())
    }
}
