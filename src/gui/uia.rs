//! UI Automation で、窓ではないコントロール（WPF）を読み、操作する（D-055）。
//!
//! WPF のボタンやテキストボックスは Win32 の窓ではなく、窓（`HwndWrapper[...]`）の中に WPF が描いているだけなので、窓を
//! たどっても見えない。WPF は UI Automation に答える（AutomationPeer）ので、それで読む。`AutomationId` は、指定が無ければ
//! `FrameworkElement.Name`（XAML の `x:Name`）になる。
//!
//! - バインディングは `idl/uiautomation.idl` から `tools/idlgen` が生成した `crate::uiautomation`。
//! - 一覧は、窓 1 つにつき 1 回の問い合わせで、木全体と要るプロパティをまとめて取る（CacheRequest）。要素ごとに問い合わせると、
//!   対象のプロセスとの行き来が要素の数 × プロパティの数になる。
//! - 問い合わせは、対象の UI のスレッドが答える。止まっている対象には呼ばない（呼ぶ側が、前に読んだ一覧を使う）。
//!   操作のハンドラーがブレークポイントで止まると答えが返らないので、待つ時間を短くしてある（`TRANSACTION_TIMEOUT_MS`）。
//! - 要素の番号（`Element::id`）は、UI Automation の RuntimeId から作る。要素が生きている間は変わらない。

use super::ffi::{self, CoCreateInstance, CLSCTX_INPROC_SERVER, RECT};
use super::Element;
use crate::com::{ComPtr, Interface, GUID};
use crate::uiautomation::*;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

/// CUIAutomation8（Windows 8 以降。IUIAutomation2 の時間切れの設定が使える）。
const CLSID_CUIAUTOMATION8: GUID = GUID { data1: 0xe22ad333, data2: 0xb25f, data3: 0x460c, data4: [0x83, 0xd0, 0x05, 0x81, 0x10, 0x73, 0x95, 0xc9] };
/// 対象につながるまで待つ時間。
const CONNECTION_TIMEOUT_MS: u32 = 2000;
/// 1 回の問い合わせ・操作の答えを待つ時間。操作のハンドラーがブレークポイントで止まったときに、ここで長く固まらない。
const TRANSACTION_TIMEOUT_MS: u32 = 2000;
/// UIA_E_TIMEOUT: 答えが時間内に返らなかった。
const UIA_E_TIMEOUT: i32 = 0x8013_1505u32 as i32;

const VT_I4: u16 = 3;
const VT_BSTR: u16 = 8;
const VT_BOOL: u16 = 11;
const VT_ARRAY: u16 = 0x2000;

/// 要素の COM の参照。1 回の処理（一覧を読んで、その中の要素を操作する）の間だけ持つ。
pub type Handles = HashMap<usize, ComPtr<IUIAutomationElement>>;

/// UI Automation で読む窓か（WPF の窓）。
pub fn is_automation_window(class: &str) -> bool {
    class.starts_with("HwndWrapper[")
}

/// 一覧に読むプロパティ。
const CACHED_PROPERTIES: &[PROPERTYID] = &[
    UIA_RuntimeIdPropertyId,
    UIA_ControlTypePropertyId,
    UIA_NamePropertyId,
    UIA_AutomationIdPropertyId,
    UIA_ClassNamePropertyId,
    UIA_BoundingRectanglePropertyId,
    UIA_IsEnabledPropertyId,
    UIA_IsOffscreenPropertyId,
    UIA_IsPasswordPropertyId,
    UIA_ValueValuePropertyId,
    UIA_ToggleToggleStatePropertyId,
    UIA_SelectionItemIsSelectedPropertyId,
];

pub struct Automation {
    client: ComPtr<IUIAutomation2>,
}

impl Automation {
    /// COM を初期化したスレッド（`gui::spawn`）で作る。
    pub fn new() -> Result<Automation, String> {
        let mut raw = null_mut();
        let hr = unsafe { CoCreateInstance(&CLSID_CUIAUTOMATION8, null_mut(), CLSCTX_INPROC_SERVER, &IUIAutomation2::IID, &mut raw) };
        if hr < 0 {
            return Err(tr!("UI Automation is not available ({code}).", code = format!("0x{hr:08X}")));
        }
        let client: ComPtr<IUIAutomation2> = unsafe { ComPtr::from_raw(raw) }.ok_or_else(|| tr!("UI Automation is not available."))?;
        unsafe {
            client.put_ConnectionTimeout(CONNECTION_TIMEOUT_MS);
            client.put_TransactionTimeout(TRANSACTION_TIMEOUT_MS);
        }
        Ok(Automation { client })
    }

    fn cache_request(&self, properties: &[PROPERTYID], scope: TreeScope) -> Result<ComPtr<IUIAutomationCacheRequest>, String> {
        let mut raw = null_mut();
        unsafe { self.client.CreateCacheRequest(&mut raw) }.ok().map_err(|e| tr!("Cannot create a UI Automation query ({error}).", error = e))?;
        let request: ComPtr<IUIAutomationCacheRequest> = unsafe { ComPtr::from_raw(raw) }.ok_or_else(|| tr!("Cannot create a UI Automation query."))?;
        for &property in properties {
            unsafe { request.AddProperty(property) };
        }
        unsafe { request.put_TreeScope(scope) };
        Ok(request)
    }

    /// 窓の中のコントロールを、木の順（見た目の上から）で `list` に足す。窓そのもの（深さ 0）は、呼ぶ側が Win32 で足す。
    pub fn read_window(&self, window: usize, max: usize, list: &mut Vec<Element>, handles: &mut Handles) -> Result<(), String> {
        let request = self.cache_request(CACHED_PROPERTIES, TreeScope_Subtree)?;
        let mut raw = null_mut();
        let hr = unsafe { self.client.ElementFromHandleBuildCache(window as UIA_HWND, request.as_raw(), &mut raw) };
        let root: ComPtr<IUIAutomationElement> = match unsafe { ComPtr::from_raw(raw) } {
            Some(root) if hr.is_ok() => root,
            _ => return Err(timeout_or(hr.0, &tr!("Could not read the controls in the window"))),
        };
        let mut stack: Vec<(ComPtr<IUIAutomationElement>, usize)> = cached_children(&root).into_iter().rev().map(|c| (c, 1)).collect();
        while let Some((element, depth)) = stack.pop() {
            if list.len() >= max {
                break;
            }
            let read = describe(&element, window, depth);
            handles.insert(read.id, element.clone());
            list.push(read);
            for child in cached_children(&element).into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
        Ok(())
    }

    /// 要素の子孫のうち、項目（ListItem）の名前と要素。
    fn items(&self, element: &ComPtr<IUIAutomationElement>) -> Result<Vec<(String, ComPtr<IUIAutomationElement>)>, String> {
        let request = self.cache_request(&[UIA_NamePropertyId], TreeScope_Element)?;
        let mut condition = null_mut();
        unsafe { self.client.CreatePropertyCondition(UIA_ControlTypePropertyId, i4(UIA_ListItemControlTypeId), &mut condition) }.ok().map_err(|e| tr!("Cannot search the items ({error}).", error = e))?;
        let condition: ComPtr<IUIAutomationCondition> = unsafe { ComPtr::from_raw(condition) }.ok_or_else(|| tr!("Cannot search the items."))?;
        let mut raw = null_mut();
        let hr = unsafe { element.FindAllBuildCache(TreeScope_Descendants, condition.as_raw(), request.as_raw(), &mut raw) };
        if !hr.is_ok() {
            return Err(timeout_or(hr.0, &tr!("Could not read the items")));
        }
        Ok(array(raw).into_iter().map(|item| (bstr(|out| unsafe { item.get_CachedName(out) }).unwrap_or_default(), item)).collect())
    }

    /// コンボボックス・リストの項目を選ぶ。コンボボックスは、項目を作らせるために一度開いてから選び、閉じる。
    pub fn select(&self, element: &ComPtr<IUIAutomationElement>, item: Option<&str>, index: Option<usize>) -> Result<(), String> {
        let expand = pattern::<IUIAutomationExpandCollapsePattern>(element, UIA_ExpandCollapsePatternId);
        if let Some(expand) = &expand {
            unsafe { expand.Expand() };
        }
        let result = (|| {
            // WPF は、開いたあとの描画で項目の入れ物を作る。少し待ちながら読み直す。
            let mut items = Vec::new();
            for _ in 0..10 {
                items = self.items(element)?;
                if !items.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let names: Vec<&str> = items.iter().map(|(n, _)| n.as_str()).collect();
            let chosen = match (item, index) {
                (_, Some(index)) if index < items.len() => index,
                (Some(item), _) => names.iter().position(|n| *n == item).ok_or_else(|| tr!("There is no item '{item}'. Items: {items}", item = item, items = names.join(", ")))?,
                _ => return Err(tr!("Specify item (the item's text) or index. Items: {items}", items = names.join(", "))),
            };
            let select = pattern::<IUIAutomationSelectionItemPattern>(&items[chosen].1, UIA_SelectionItemPatternId).ok_or_else(|| tr!("Cannot select the item (it does not answer the select action)."))?;
            acted(unsafe { select.Select() }.0, &tr!("Could not select the item"))
        })();
        if let Some(expand) = &expand {
            unsafe { expand.Collapse() };
        }
        result
    }
}

/// 押す。押すボタンなら Invoke、チェックボックスなら Toggle、ラジオボタンなら選ぶ。
pub fn invoke(element: &ComPtr<IUIAutomationElement>) -> Result<(), String> {
    if let Some(invoke) = pattern::<IUIAutomationInvokePattern>(element, UIA_InvokePatternId) {
        return acted(unsafe { invoke.Invoke() }.0, &tr!("Could not press it"));
    }
    if let Some(toggle) = pattern::<IUIAutomationTogglePattern>(element, UIA_TogglePatternId) {
        return acted(unsafe { toggle.Toggle() }.0, &tr!("Could not press it"));
    }
    if let Some(select) = pattern::<IUIAutomationSelectionItemPattern>(element, UIA_SelectionItemPatternId) {
        return acted(unsafe { select.Select() }.0, &tr!("Could not press it"));
    }
    Err(tr!("The control does not answer the press action. Click it with ui_mouse."))
}

/// チェックを切り替える（チェックボックスは Toggle、ラジオボタンは選ぶ）。
pub fn toggle(element: &ComPtr<IUIAutomationElement>) -> Result<(), String> {
    if let Some(toggle) = pattern::<IUIAutomationTogglePattern>(element, UIA_TogglePatternId) {
        return acted(unsafe { toggle.Toggle() }.0, &tr!("Could not toggle it"));
    }
    if let Some(select) = pattern::<IUIAutomationSelectionItemPattern>(element, UIA_SelectionItemPatternId) {
        return acted(unsafe { select.Select() }.0, &tr!("Could not toggle it"));
    }
    Err(tr!("The control does not answer the check action."))
}

/// 文字を入れる（Value の操作）。
pub fn set_text(element: &ComPtr<IUIAutomationElement>, text: &str) -> Result<(), String> {
    let value = pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId).ok_or_else(|| tr!("The control does not answer the set-text action. Type with ui_keys."))?;
    let text = crate::win32::wide(text);
    let bstr = unsafe { ffi::SysAllocString(text.as_ptr()) };
    let hr = unsafe { value.SetValue(bstr) }.0;
    unsafe { ffi::SysFreeString(bstr) };
    acted(hr, &tr!("Could not set the text"))
}

/// キーの入力先にする。
pub fn focus(element: &ComPtr<IUIAutomationElement>) -> Result<(), String> {
    acted(unsafe { element.SetFocus() }.0, &tr!("Could not move the focus to the control"))
}

/// 操作の結果。時間切れは、ハンドラーがブレークポイントで止まった（など）として、操作は届いたものとみなす（止まったかは呼ぶ側が見る）。
fn acted(hr: i32, what: &str) -> Result<(), String> {
    if hr >= 0 || hr == UIA_E_TIMEOUT {
        return Ok(());
    }
    Err(tr!("{what} ({code}).", what = what, code = format!("0x{hr:08X}")))
}

fn timeout_or(hr: i32, what: &str) -> String {
    if hr == UIA_E_TIMEOUT {
        return tr!("The target's windows do not answer (they seem stopped or busy).");
    }
    tr!("{what} ({code}).", what = what, code = format!("0x{hr:08X}"))
}

fn pattern<P: Interface>(element: &ComPtr<IUIAutomationElement>, id: PATTERNID) -> Option<ComPtr<P>> {
    let mut raw = null_mut();
    let hr = unsafe { element.GetCurrentPatternAs(id, &P::IID, &mut raw) };
    // 対応していないパターンは、成功で null が返る。
    hr.is_ok().then(|| unsafe { ComPtr::from_raw(raw) }).flatten()
}

fn cached_children(element: &ComPtr<IUIAutomationElement>) -> Vec<ComPtr<IUIAutomationElement>> {
    let mut raw = null_mut();
    if !unsafe { element.GetCachedChildren(&mut raw) }.is_ok() {
        return Vec::new();
    }
    array(raw)
}

/// IUIAutomationElementArray（子が無ければ null）を、要素の列にする。
fn array(raw: *mut c_void) -> Vec<ComPtr<IUIAutomationElement>> {
    let Some(array): Option<ComPtr<IUIAutomationElementArray>> = (unsafe { ComPtr::from_raw(raw) }) else { return Vec::new() };
    let mut length = 0;
    unsafe { array.get_Length(&mut length) };
    (0..length)
        .filter_map(|i| {
            let mut element = null_mut();
            unsafe { array.GetElement(i, &mut element) };
            unsafe { ComPtr::from_raw(element) }
        })
        .collect()
}

/// BSTR を返すメソッドの答え（読んで解放する）。
fn bstr(call: impl FnOnce(*mut BSTR) -> crate::com::HRESULT) -> Option<String> {
    let mut raw: BSTR = null_mut();
    if !call(&mut raw).is_ok() || raw.is_null() {
        return None;
    }
    let text = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(raw, ffi::SysStringLen(raw) as usize)) };
    unsafe { ffi::SysFreeString(raw) };
    Some(text)
}

fn i4(value: i32) -> VARIANT {
    VARIANT { vt: VT_I4, wReserved1: 0, wReserved2: 0, wReserved3: 0, data: [value as u32 as usize, 0] }
}

/// 要素の、読んでおいたプロパティの値。
enum Property {
    Int(i32),
    Bool(bool),
    Text(String),
    Ints(Vec<i32>),
    Empty,
}

fn cached_property(element: &ComPtr<IUIAutomationElement>, id: PROPERTYID) -> Property {
    let mut value = VARIANT { vt: 0, wReserved1: 0, wReserved2: 0, wReserved3: 0, data: [0, 0] };
    if !unsafe { element.GetCachedPropertyValue(id, &mut value) }.is_ok() {
        return Property::Empty;
    }
    let read = match value.vt {
        VT_I4 => Property::Int(value.data[0] as u32 as i32),
        // VARIANT_BOOL は、真が -1（0xFFFF）。
        VT_BOOL => Property::Bool(value.data[0] as u16 != 0),
        VT_BSTR if value.data[0] != 0 => {
            let raw = value.data[0] as *const u16;
            Property::Text(unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(raw, ffi::SysStringLen(raw) as usize)) })
        }
        vt if vt == VT_ARRAY | VT_I4 => Property::Ints(safe_array_ints(value.data[0] as *mut c_void)),
        _ => Property::Empty,
    };
    unsafe { ffi::VariantClear(&mut value as *mut VARIANT as *mut c_void) };
    read
}

/// 1 次元の SAFEARRAY(int) の中身。
fn safe_array_ints(array: *mut c_void) -> Vec<i32> {
    if array.is_null() {
        return Vec::new();
    }
    let (mut low, mut high) = (0i32, -1i32);
    let mut data = null_mut();
    unsafe {
        if ffi::SafeArrayGetLBound(array, 1, &mut low) < 0 || ffi::SafeArrayGetUBound(array, 1, &mut high) < 0 || high < low || ffi::SafeArrayAccessData(array, &mut data) < 0 {
            return Vec::new();
        }
        let ints = std::slice::from_raw_parts(data as *const i32, (high - low + 1) as usize).to_vec();
        ffi::SafeArrayUnaccessData(array);
        ints
    }
}

/// 要素の番号。RuntimeId（整数の列）の FNV-1a を 31 ビットにし、32 ビット目を印にする。窓のハンドルは 32 ビット目が
/// 立たない（x64 でも、窓のハンドルの値は 32 ビットに収まる）ので重ならない。x86 と x64 で同じ値になる。
pub fn element_id(runtime_id: &[i32]) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for value in runtime_id {
        for byte in value.to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    (hash & 0x7FFF_FFFF | ELEMENT_ID_MARK) as usize
}

const ELEMENT_ID_MARK: u32 = 0x8000_0000;


/// 種類（WinForms の側の名前にそろえる）。
#[allow(non_upper_case_globals)] // 生成した定数の名前は SDK のまま
pub fn kind_of(control_type: CONTROLTYPEID) -> &'static str {
    match control_type {
        UIA_ButtonControlTypeId => "Button",
        UIA_CalendarControlTypeId => "Calendar",
        UIA_CheckBoxControlTypeId => "CheckBox",
        UIA_ComboBoxControlTypeId => "ComboBox",
        UIA_EditControlTypeId => "TextBox",
        UIA_HyperlinkControlTypeId => "Hyperlink",
        UIA_ImageControlTypeId => "Image",
        UIA_ListItemControlTypeId => "ListItem",
        UIA_ListControlTypeId => "ListBox",
        UIA_MenuControlTypeId => "Menu",
        UIA_MenuBarControlTypeId => "MenuBar",
        UIA_MenuItemControlTypeId => "MenuItem",
        UIA_ProgressBarControlTypeId => "ProgressBar",
        UIA_RadioButtonControlTypeId => "RadioButton",
        UIA_ScrollBarControlTypeId => "ScrollBar",
        UIA_SliderControlTypeId => "Slider",
        UIA_SpinnerControlTypeId => "Spinner",
        UIA_StatusBarControlTypeId => "StatusBar",
        UIA_TabControlTypeId => "TabControl",
        UIA_TabItemControlTypeId => "TabItem",
        UIA_TextControlTypeId => "Label",
        UIA_ToolBarControlTypeId => "ToolBar",
        UIA_ToolTipControlTypeId => "ToolTip",
        UIA_TreeControlTypeId => "TreeView",
        UIA_TreeItemControlTypeId => "TreeItem",
        UIA_GroupControlTypeId => "GroupBox",
        UIA_ThumbControlTypeId => "Thumb",
        UIA_DataGridControlTypeId => "DataGrid",
        UIA_DataItemControlTypeId => "DataItem",
        UIA_DocumentControlTypeId => "Document",
        UIA_SplitButtonControlTypeId => "SplitButton",
        UIA_WindowControlTypeId => "Window",
        UIA_PaneControlTypeId => "Pane",
        UIA_HeaderControlTypeId => "Header",
        UIA_HeaderItemControlTypeId => "HeaderItem",
        UIA_TableControlTypeId => "Table",
        UIA_TitleBarControlTypeId => "TitleBar",
        UIA_SeparatorControlTypeId => "Separator",
        _ => "Control",
    }
}

fn describe(element: &ComPtr<IUIAutomationElement>, window: usize, depth: usize) -> Element {
    let text = |id| match cached_property(element, id) {
        Property::Text(text) => Some(text),
        _ => None,
    };
    let flag = |id| matches!(cached_property(element, id), Property::Bool(true));
    let mut control_type = 0;
    unsafe { element.get_CachedControlType(&mut control_type) };
    let kind = kind_of(control_type);
    let mut rect = crate::uiautomation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { element.get_CachedBoundingRectangle(&mut rect) };
    let rect = RECT { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
    let runtime_id = match cached_property(element, UIA_RuntimeIdPropertyId) {
        Property::Ints(ids) => ids,
        _ => Vec::new(),
    };
    // 文字: 入力欄は値（パスワードは読まない）。ほかは名前（ボタンの表示、ラベルの文字）。
    let value = if flag(UIA_IsPasswordPropertyId) { None } else { text(UIA_ValueValuePropertyId) };
    let shown = text(UIA_NamePropertyId);
    let checked = match kind {
        "CheckBox" => match cached_property(element, UIA_ToggleToggleStatePropertyId) {
            Property::Int(state) => Some(state == ToggleState_On as i32),
            _ => None,
        },
        "RadioButton" | "ListItem" | "TabItem" => match cached_property(element, UIA_SelectionItemIsSelectedPropertyId) {
            Property::Bool(selected) => Some(selected),
            _ => None,
        },
        _ => None,
    };
    let non_empty = |s: Option<String>| s.filter(|s| !s.is_empty());
    Element {
        id: element_id(&runtime_id),
        window,
        depth,
        kind,
        type_name: non_empty(text(UIA_ClassNamePropertyId)),
        name: non_empty(text(UIA_AutomationIdPropertyId)),
        text: match kind {
            "TextBox" => value.or(shown),
            // 選ぶだけのコンボボックスとリストは、値を持たず、閉じている間は項目も子に見せない。選ばれている項目の名前にする。
            "ComboBox" | "ListBox" => non_empty(value).or_else(|| selected_names(element)).or(shown),
            _ => shown,
        },
        visible: !flag(UIA_IsOffscreenPropertyId) && rect.right > rect.left && rect.bottom > rect.top,
        enabled: flag(UIA_IsEnabledPropertyId),
        checked,
        rect,
    }
}

/// 選ばれている項目の名前（複数なら `, ` でつなぐ。無ければ `None`）。対象への問い合わせになる（一覧の中の、コンボボックスと
/// リストの数だけ）。
fn selected_names(element: &ComPtr<IUIAutomationElement>) -> Option<String> {
    let selection = pattern::<IUIAutomationSelectionPattern>(element, UIA_SelectionPatternId)?;
    let mut raw = null_mut();
    if !unsafe { selection.GetCurrentSelection(&mut raw) }.is_ok() {
        return None;
    }
    let names: Vec<String> = array(raw).iter().filter_map(|item| bstr(|out| unsafe { item.get_CurrentName(out) })).collect();
    (!names.is_empty()).then(|| names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_ids_do_not_collide_with_window_handles() {
        let id = element_id(&[42, 1234, 7]);
        assert_eq!(id, element_id(&[42, 1234, 7]));
        assert_ne!(id, element_id(&[42, 1234, 8]));
        // 窓のハンドルは 32 ビット目が立たないので、印のある要素の番号とは重ならない。
        assert_eq!(id as u64 & 0xFFFF_FFFF_8000_0000, 0x8000_0000);
    }

    #[test]
    fn control_types_are_named_like_winforms() {
        assert_eq!(kind_of(UIA_EditControlTypeId), "TextBox");
        assert_eq!(kind_of(UIA_TextControlTypeId), "Label");
        assert_eq!(kind_of(UIA_ListControlTypeId), "ListBox");
        assert_eq!(kind_of(12345), "Control");
    }
}
