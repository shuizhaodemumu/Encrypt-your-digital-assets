use windows::core::VARIANT;
// Discards only the ink prompt owned by our exact slideshow window.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use windows::Win32::{
    System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
    UI::Accessibility::*,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
pub struct InkGuard {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    pub dismissed: Arc<AtomicU32>,
}
impl InkGuard {
    pub fn start(hwnd: isize, pid: u32) -> anyhow::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let dismissed = Arc::new(AtomicU32::new(0));
        let flag = stop.clone();
        let count = dismissed.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let apartment = crate::automation::Apartment::new();
            if let Err(e) = apartment {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
            let _apartment = apartment.unwrap();
            let automation: windows::core::Result<IUIAutomation> =
                unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
            let automation = match automation {
                Ok(a) => a,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            };
            let _ = tx.send(Ok(()));
            while !flag.load(Ordering::SeqCst) {
                struct Search {
                    owner: isize,
                    pid: u32,
                    windows: Vec<isize>,
                }
                unsafe extern "system" fn visit(h: isize, l: isize) -> i32 {
                    let s = &mut *(l as *mut Search);
                    let mut pid = 0;
                    GetWindowThreadProcessId(h, &mut pid);
                    if pid == s.pid && GetWindow(h, GW_OWNER) == s.owner && IsWindowVisible(h) != 0
                    {
                        let mut cls = [0u16; 64];
                        let n = GetClassNameW(h, cls.as_mut_ptr(), 64);
                        if String::from_utf16_lossy(&cls[..n.max(0) as usize]) == "NUIDialog" {
                            s.windows.push(h);
                        }
                    }
                    1
                }
                let mut search = Search {
                    owner: hwnd,
                    pid,
                    windows: vec![],
                };
                unsafe {
                    EnumWindows(Some(visit), (&mut search as *mut Search) as isize);
                }
                for h in search.windows {
                    if unsafe { discard_ink(&automation, h) }.is_ok() {
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                }
                thread::sleep(Duration::from_millis(30));
            }
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
                dismissed,
            }),
            other => {
                stop.store(true, Ordering::SeqCst);
                anyhow::bail!("PowerPoint 批注退出保护初始化失败：{other:?}");
            }
        }
    }
}
unsafe fn discard_ink(a: &IUIAutomation, h: isize) -> anyhow::Result<()> {
    let root = a.ElementFromHandle(windows::Win32::Foundation::HWND(h as _))?;
    let condition =
        a.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("4001"))?;
    let text = root
        .FindFirst(TreeScope_Descendants, &condition)?
        .CurrentName()?
        .to_string();
    anyhow::ensure!(
        text.contains("墨迹") || text.to_ascii_lowercase().contains("ink annotations"),
        "不是批注提示"
    );
    let condition = a.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("102"))?;
    let button = root.FindFirst(TreeScope_Descendants, &condition)?;
    let name = button.CurrentName()?.to_string();
    anyhow::ensure!(
        name.contains("放弃") || name.contains("丢弃") || name.contains("Discard"),
        "不是丢弃按钮"
    );
    let invoke: IUIAutomationInvokePattern = button.GetCurrentPatternAs(UIA_InvokePatternId)?;
    invoke.Invoke()?;
    Ok(())
}
impl Drop for InkGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
