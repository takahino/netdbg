//! 窓の見た目を撮る。
//!
//! デバッガーで止まった窓は、前に出ると Windows が「応答なし」の代わりの窓（白っぽい写し）に差し替える。そこで、
//! 拡張の `resources/windowCapture.ps1` と同じく、DWM のサムネイル（もとの窓の最後の描画）を、自前の最前面の窓に
//! 映してから、画面を写す。動いている窓も同じやり方で撮る（ほかの窓に隠れていても写る）。

use super::ffi::*;
use super::png;
use crate::win32::HANDLE;
use std::ptr::null_mut;

/// 撮った画像。`origin` は、画像の左上の画面の座標（物理ピクセル）。
pub struct Shot {
    pub png: Vec<u8>,
    pub origin: (i32, i32),
    pub width: i32,
    pub height: i32,
    /// 上の行からの BGRA（成績書で、操作した箇所を塗るのに使う）。
    pub pixels: Vec<u8>,
}

/// 画像に描く、番号つきの枠（Set-of-Mark）。座標は画面の座標。
pub struct Mark {
    pub rect: RECT,
    pub label: String,
    /// 前にある窓（ここには描かない。後ろの窓のコントロールの枠が、前のダイアログの上に出ないように）。
    pub hidden_by: Vec<RECT>,
}

/// DWM がサムネイルを描き終えるのを待つ回数（DwmFlush 1 回が 1 フレーム）。
const SETTLE_FRAMES: usize = 3;

/// 窓の見えている範囲（影や、目に見えない枠を除く）。
pub fn visible_rect(window: usize) -> RECT {
    let mut rect = RECT::default();
    let ok = unsafe { DwmGetWindowAttribute(window as HWND, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32) };
    if ok != 0 {
        unsafe { GetWindowRect(window as HWND, &mut rect) };
    }
    rect
}

pub fn union(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

pub fn intersect(a: RECT, b: RECT) -> Option<RECT> {
    let rect = RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) };
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}

/// `windows`（トップレベルの窓。前にあるものから順）を撮る。`crop` があれば、その範囲（画面の座標）だけ。
/// `windows` が空なら、画面の `crop` をそのまま写す。
/// `scale` は画面の DPI の倍率（印の文字の大きさに使う）。
pub fn capture(windows: &[usize], crop: Option<RECT>, marks: &[Mark], scale: f64) -> Result<Shot, String> {
    let shown: Vec<usize> = windows.iter().copied().filter(|&w| unsafe { IsWindowVisible(w as HWND) != 0 && IsIconic(w as HWND) == 0 }).collect();
    if !windows.is_empty() && shown.is_empty() {
        return Err(tr!("The window is not visible (it is minimized or hidden)."));
    }
    let bounds = shown.iter().map(|&w| visible_rect(w)).reduce(union);
    let area = match (bounds, crop) {
        (Some(bounds), Some(crop)) => intersect(bounds, crop).ok_or_else(|| tr!("The given area is outside the window."))?,
        (Some(bounds), None) => bounds,
        (None, Some(crop)) => crop,
        (None, None) => return Err(tr!("There is no area to capture.")),
    };
    let overlay = match bounds {
        Some(bounds) => Some(Overlay::show(bounds, &shown)?),
        None => None,
    };
    let pixels = copy_screen(area, marks, scale);
    drop(overlay);
    let pixels = pixels?;
    let (width, height) = (area.right - area.left, area.bottom - area.top);
    Ok(Shot { png: png::encode(width as usize, height as usize, &pixels), origin: (area.left, area.top), width, height, pixels })
}

/// もとの窓の描画を映す、最前面の窓（入力は素通し、前に出ない）。
struct Overlay {
    window: HWND,
    thumbnails: Vec<isize>,
}

impl Overlay {
    fn show(bounds: RECT, windows: &[usize]) -> Result<Overlay, String> {
        let class = crate::win32::wide("NetdbgCapture");
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let description = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: DefWindowProcW,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) } as HANDLE,
            lpszMenuName: null_mut(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        // 2 回目からは「もうある」で失敗するが、そのまま使える。
        unsafe { RegisterClassExW(&description) };
        let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
        let window = unsafe { CreateWindowExW(ex_style, class.as_ptr(), class.as_ptr(), WS_POPUP, bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top, null_mut(), null_mut(), instance, null_mut()) };
        if window.is_null() {
            return Err(tr!("Could not create the window for capturing ({error}).", error = std::io::Error::last_os_error()));
        }
        let mut overlay = Overlay { window, thumbnails: Vec::new() };
        // 後ろの窓から登録する（後から登録したサムネイルが上に描かれる）。
        for &source in windows.iter().rev() {
            let mut thumbnail = 0isize;
            let hr = unsafe { DwmRegisterThumbnail(window, source as HWND, &mut thumbnail) };
            if hr != 0 {
                return Err(tr!("DwmRegisterThumbnail failed ({code}).", code = format!("0x{hr:08X}")));
            }
            overlay.thumbnails.push(thumbnail);
            let (frame, visible) = (super::window_rect(source), visible_rect(source));
            let properties = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_OPACITY | DWM_TNP_VISIBLE | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination: RECT { left: visible.left - bounds.left, top: visible.top - bounds.top, right: visible.right - bounds.left, bottom: visible.bottom - bounds.top },
                rcSource: RECT { left: visible.left - frame.left, top: visible.top - frame.top, right: visible.right - frame.left, bottom: visible.bottom - frame.top },
                opacity: 255,
                fVisible: 1,
                fSourceClientAreaOnly: 0,
            };
            unsafe { DwmUpdateThumbnailProperties(thumbnail, &properties) };
        }
        unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
        for _ in 0..SETTLE_FRAMES {
            pump_messages();
            unsafe { DwmFlush() };
        }
        pump_messages();
        Ok(overlay)
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        for &thumbnail in &self.thumbnails {
            unsafe { DwmUnregisterThumbnail(thumbnail) };
        }
        unsafe { DestroyWindow(self.window) };
        pump_messages();
    }
}

fn pump_messages() {
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// 画面の `area` を写し、印を描いて、上の行からの BGRA で返す。
fn copy_screen(area: RECT, marks: &[Mark], scale: f64) -> Result<Vec<u8>, String> {
    let (width, height) = (area.right - area.left, area.bottom - area.top);
    unsafe {
        let screen = GetDC(null_mut());
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(memory, bitmap);
        let copied = BitBlt(memory, 0, 0, width, height, screen, area.left, area.top, SRCCOPY);
        draw_marks(memory, area, marks, scale);
        SelectObject(memory, old);
        let mut header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // 負: 上の行から
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(memory, bitmap, 0, height as u32, pixels.as_mut_ptr() as *mut _, &mut header, DIB_RGB_COLORS);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(null_mut(), screen);
        if copied == 0 || lines != height {
            return Err(tr!("Could not capture the screen ({error}). Is the screen locked?", error = std::io::Error::last_os_error()));
        }
        Ok(pixels)
    }
}

/// 枠（赤）と、左上に番号（赤の地に白の太字）。大きさは DPI の倍率に合わせる。
unsafe fn draw_marks(dc: HDC, area: RECT, marks: &[Mark], scale: f64) {
    if marks.is_empty() {
        return;
    }
    let to_image = |r: RECT| RECT { left: r.left - area.left, top: r.top - area.top, right: r.right - area.left, bottom: r.bottom - area.top };
    let red = CreateSolidBrush(0x0000_00E0);
    let font = CreateFontW(-((13.0 * scale).round() as i32), 0, 0, 0, FW_BOLD, 0, 0, 0, 1, 0, 0, ANTIALIASED_QUALITY, 0, crate::win32::wide("Segoe UI").as_ptr());
    let old_font = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, 0x00FF_FFFF);
    let thickness = (scale.round() as i32).max(1) + 1;
    let padding = (2.0 * scale).round() as i32;
    for mark in marks {
        let saved = SaveDC(dc);
        for front in &mark.hidden_by {
            let f = to_image(*front);
            ExcludeClipRect(dc, f.left, f.top, f.right, f.bottom);
        }
        let r = to_image(mark.rect);
        for i in 0..thickness {
            FrameRect(dc, &RECT { left: r.left + i, top: r.top + i, right: r.right - i, bottom: r.bottom - i }, red);
        }
        let label = crate::win32::wide(&mark.label);
        let characters = label.len() as i32 - 1;
        let mut size = [0i32; 2];
        GetTextExtentPoint32W(dc, label.as_ptr(), characters, &mut size);
        let tag = RECT { left: r.left, top: r.top, right: r.left + size[0] + 2 * padding, bottom: r.top + size[1] };
        FillRect(dc, &tag, red);
        TextOutW(dc, tag.left + padding, tag.top, label.as_ptr(), characters);
        RestoreDC(dc, saved);
    }
    SelectObject(dc, old_font);
    DeleteObject(font);
    DeleteObject(red);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_join_and_overlap() {
        let a = RECT { left: 0, top: 0, right: 10, bottom: 10 };
        let b = RECT { left: 5, top: 5, right: 20, bottom: 8 };
        assert_eq!(union(a, b), RECT { left: 0, top: 0, right: 20, bottom: 10 });
        assert_eq!(intersect(a, b), Some(RECT { left: 5, top: 5, right: 10, bottom: 8 }));
        assert_eq!(intersect(a, RECT { left: 10, top: 0, right: 12, bottom: 5 }), None);
    }
}
