// このファイルは tools/idlgen が idl/uiautomation.idl から生成した。手で編集しない。
//     cargo run -p idlgen -- idl/uiautomation.idl src/uiautomation.rs
//
// メソッドは `extern "system"`: x86 では stdcall、x64 では通常の規約になる。
// インターフェイスへのポインターは `*mut c_void` で受け渡しし、呼び出し側が `ComPtr` に包む。
// 中身を知らずポインターとしてだけ扱っている型: IAccessible, IUIAutomationEventHandler, IUIAutomationFocusChangedEventHandler, IUIAutomationPropertyChangedEventHandler, IUIAutomationProxyFactory, IUIAutomationProxyFactoryEntry, IUIAutomationProxyFactoryMapping, IUIAutomationStructureChangedEventHandler, IUIAutomationTreeWalker, OrientationType, SAFEARRAY
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]

use crate::com::{ComPtr, IUnknownVtbl, Interface, GUID, HRESULT};
use std::ffi::c_void;

pub type BSTR = *mut u16;
pub type PROPERTYID = i32;
pub type PATTERNID = i32;
pub type EVENTID = i32;
pub type CONTROLTYPEID = i32;
pub type UIA_HWND = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VARIANT {
    pub vt: u16,
    pub wReserved1: u16,
    pub wReserved2: u16,
    pub wReserved3: u16,
    pub data: [usize; 2],
}

pub type ToggleState = u32;
pub const ToggleState_Off: ToggleState = 0x0;
pub const ToggleState_On: ToggleState = 0x1;
pub const ToggleState_Indeterminate: ToggleState = 0x2;

pub type ExpandCollapseState = u32;
pub const ExpandCollapseState_Collapsed: ExpandCollapseState = 0x0;
pub const ExpandCollapseState_Expanded: ExpandCollapseState = 0x1;
pub const ExpandCollapseState_PartiallyExpanded: ExpandCollapseState = 0x2;
pub const ExpandCollapseState_LeafNode: ExpandCollapseState = 0x3;

pub type TreeScope = u32;
pub const TreeScope_None: TreeScope = 0x0;
pub const TreeScope_Element: TreeScope = 0x1;
pub const TreeScope_Children: TreeScope = 0x2;
pub const TreeScope_Descendants: TreeScope = 0x4;
pub const TreeScope_Parent: TreeScope = 0x8;
pub const TreeScope_Ancestors: TreeScope = 0x10;
pub const TreeScope_Subtree: TreeScope = 0x7;

pub type PropertyConditionFlags = u32;
pub const PropertyConditionFlags_None: PropertyConditionFlags = 0x0;
pub const PropertyConditionFlags_IgnoreCase: PropertyConditionFlags = 0x1;
pub const PropertyConditionFlags_MatchSubstring: PropertyConditionFlags = 0x2;

pub type AutomationElementMode = u32;
pub const AutomationElementMode_None: AutomationElementMode = 0x0;
pub const AutomationElementMode_Full: AutomationElementMode = 0x1;
pub const UIA_InvokePatternId: i32 = 10000;
pub const UIA_SelectionPatternId: i32 = 10001;
pub const UIA_ValuePatternId: i32 = 10002;
pub const UIA_RangeValuePatternId: i32 = 10003;
pub const UIA_ScrollPatternId: i32 = 10004;
pub const UIA_ExpandCollapsePatternId: i32 = 10005;
pub const UIA_GridPatternId: i32 = 10006;
pub const UIA_GridItemPatternId: i32 = 10007;
pub const UIA_MultipleViewPatternId: i32 = 10008;
pub const UIA_WindowPatternId: i32 = 10009;
pub const UIA_SelectionItemPatternId: i32 = 10010;
pub const UIA_DockPatternId: i32 = 10011;
pub const UIA_TablePatternId: i32 = 10012;
pub const UIA_TableItemPatternId: i32 = 10013;
pub const UIA_TextPatternId: i32 = 10014;
pub const UIA_TogglePatternId: i32 = 10015;
pub const UIA_TransformPatternId: i32 = 10016;
pub const UIA_ScrollItemPatternId: i32 = 10017;
pub const UIA_LegacyIAccessiblePatternId: i32 = 10018;
pub const UIA_ItemContainerPatternId: i32 = 10019;
pub const UIA_VirtualizedItemPatternId: i32 = 10020;
pub const UIA_SynchronizedInputPatternId: i32 = 10021;
pub const UIA_ObjectModelPatternId: i32 = 10022;
pub const UIA_AnnotationPatternId: i32 = 10023;
pub const UIA_TextPattern2Id: i32 = 10024;
pub const UIA_StylesPatternId: i32 = 10025;
pub const UIA_SpreadsheetPatternId: i32 = 10026;
pub const UIA_SpreadsheetItemPatternId: i32 = 10027;
pub const UIA_TransformPattern2Id: i32 = 10028;
pub const UIA_TextChildPatternId: i32 = 10029;
pub const UIA_DragPatternId: i32 = 10030;
pub const UIA_DropTargetPatternId: i32 = 10031;
pub const UIA_TextEditPatternId: i32 = 10032;
pub const UIA_CustomNavigationPatternId: i32 = 10033;
pub const UIA_SelectionPattern2Id: i32 = 10034;
pub const UIA_RuntimeIdPropertyId: i32 = 30000;
pub const UIA_BoundingRectanglePropertyId: i32 = 30001;
pub const UIA_ProcessIdPropertyId: i32 = 30002;
pub const UIA_ControlTypePropertyId: i32 = 30003;
pub const UIA_LocalizedControlTypePropertyId: i32 = 30004;
pub const UIA_NamePropertyId: i32 = 30005;
pub const UIA_AcceleratorKeyPropertyId: i32 = 30006;
pub const UIA_AccessKeyPropertyId: i32 = 30007;
pub const UIA_HasKeyboardFocusPropertyId: i32 = 30008;
pub const UIA_IsKeyboardFocusablePropertyId: i32 = 30009;
pub const UIA_IsEnabledPropertyId: i32 = 30010;
pub const UIA_AutomationIdPropertyId: i32 = 30011;
pub const UIA_ClassNamePropertyId: i32 = 30012;
pub const UIA_HelpTextPropertyId: i32 = 30013;
pub const UIA_ClickablePointPropertyId: i32 = 30014;
pub const UIA_CulturePropertyId: i32 = 30015;
pub const UIA_IsControlElementPropertyId: i32 = 30016;
pub const UIA_IsContentElementPropertyId: i32 = 30017;
pub const UIA_LabeledByPropertyId: i32 = 30018;
pub const UIA_IsPasswordPropertyId: i32 = 30019;
pub const UIA_NativeWindowHandlePropertyId: i32 = 30020;
pub const UIA_ItemTypePropertyId: i32 = 30021;
pub const UIA_IsOffscreenPropertyId: i32 = 30022;
pub const UIA_OrientationPropertyId: i32 = 30023;
pub const UIA_FrameworkIdPropertyId: i32 = 30024;
pub const UIA_IsRequiredForFormPropertyId: i32 = 30025;
pub const UIA_ItemStatusPropertyId: i32 = 30026;
pub const UIA_IsDockPatternAvailablePropertyId: i32 = 30027;
pub const UIA_IsExpandCollapsePatternAvailablePropertyId: i32 = 30028;
pub const UIA_IsGridItemPatternAvailablePropertyId: i32 = 30029;
pub const UIA_IsGridPatternAvailablePropertyId: i32 = 30030;
pub const UIA_IsInvokePatternAvailablePropertyId: i32 = 30031;
pub const UIA_IsMultipleViewPatternAvailablePropertyId: i32 = 30032;
pub const UIA_IsRangeValuePatternAvailablePropertyId: i32 = 30033;
pub const UIA_IsScrollPatternAvailablePropertyId: i32 = 30034;
pub const UIA_IsScrollItemPatternAvailablePropertyId: i32 = 30035;
pub const UIA_IsSelectionItemPatternAvailablePropertyId: i32 = 30036;
pub const UIA_IsSelectionPatternAvailablePropertyId: i32 = 30037;
pub const UIA_IsTablePatternAvailablePropertyId: i32 = 30038;
pub const UIA_IsTableItemPatternAvailablePropertyId: i32 = 30039;
pub const UIA_IsTextPatternAvailablePropertyId: i32 = 30040;
pub const UIA_IsTogglePatternAvailablePropertyId: i32 = 30041;
pub const UIA_IsTransformPatternAvailablePropertyId: i32 = 30042;
pub const UIA_IsValuePatternAvailablePropertyId: i32 = 30043;
pub const UIA_IsWindowPatternAvailablePropertyId: i32 = 30044;
pub const UIA_ValueValuePropertyId: i32 = 30045;
pub const UIA_ValueIsReadOnlyPropertyId: i32 = 30046;
pub const UIA_RangeValueValuePropertyId: i32 = 30047;
pub const UIA_RangeValueIsReadOnlyPropertyId: i32 = 30048;
pub const UIA_RangeValueMinimumPropertyId: i32 = 30049;
pub const UIA_RangeValueMaximumPropertyId: i32 = 30050;
pub const UIA_RangeValueLargeChangePropertyId: i32 = 30051;
pub const UIA_RangeValueSmallChangePropertyId: i32 = 30052;
pub const UIA_ScrollHorizontalScrollPercentPropertyId: i32 = 30053;
pub const UIA_ScrollHorizontalViewSizePropertyId: i32 = 30054;
pub const UIA_ScrollVerticalScrollPercentPropertyId: i32 = 30055;
pub const UIA_ScrollVerticalViewSizePropertyId: i32 = 30056;
pub const UIA_ScrollHorizontallyScrollablePropertyId: i32 = 30057;
pub const UIA_ScrollVerticallyScrollablePropertyId: i32 = 30058;
pub const UIA_SelectionSelectionPropertyId: i32 = 30059;
pub const UIA_SelectionCanSelectMultiplePropertyId: i32 = 30060;
pub const UIA_SelectionIsSelectionRequiredPropertyId: i32 = 30061;
pub const UIA_GridRowCountPropertyId: i32 = 30062;
pub const UIA_GridColumnCountPropertyId: i32 = 30063;
pub const UIA_GridItemRowPropertyId: i32 = 30064;
pub const UIA_GridItemColumnPropertyId: i32 = 30065;
pub const UIA_GridItemRowSpanPropertyId: i32 = 30066;
pub const UIA_GridItemColumnSpanPropertyId: i32 = 30067;
pub const UIA_GridItemContainingGridPropertyId: i32 = 30068;
pub const UIA_DockDockPositionPropertyId: i32 = 30069;
pub const UIA_ExpandCollapseExpandCollapseStatePropertyId: i32 = 30070;
pub const UIA_MultipleViewCurrentViewPropertyId: i32 = 30071;
pub const UIA_MultipleViewSupportedViewsPropertyId: i32 = 30072;
pub const UIA_WindowCanMaximizePropertyId: i32 = 30073;
pub const UIA_WindowCanMinimizePropertyId: i32 = 30074;
pub const UIA_WindowWindowVisualStatePropertyId: i32 = 30075;
pub const UIA_WindowWindowInteractionStatePropertyId: i32 = 30076;
pub const UIA_WindowIsModalPropertyId: i32 = 30077;
pub const UIA_WindowIsTopmostPropertyId: i32 = 30078;
pub const UIA_SelectionItemIsSelectedPropertyId: i32 = 30079;
pub const UIA_SelectionItemSelectionContainerPropertyId: i32 = 30080;
pub const UIA_TableRowHeadersPropertyId: i32 = 30081;
pub const UIA_TableColumnHeadersPropertyId: i32 = 30082;
pub const UIA_TableRowOrColumnMajorPropertyId: i32 = 30083;
pub const UIA_TableItemRowHeaderItemsPropertyId: i32 = 30084;
pub const UIA_TableItemColumnHeaderItemsPropertyId: i32 = 30085;
pub const UIA_ToggleToggleStatePropertyId: i32 = 30086;
pub const UIA_TransformCanMovePropertyId: i32 = 30087;
pub const UIA_TransformCanResizePropertyId: i32 = 30088;
pub const UIA_TransformCanRotatePropertyId: i32 = 30089;
pub const UIA_IsLegacyIAccessiblePatternAvailablePropertyId: i32 = 30090;
pub const UIA_LegacyIAccessibleChildIdPropertyId: i32 = 30091;
pub const UIA_LegacyIAccessibleNamePropertyId: i32 = 30092;
pub const UIA_LegacyIAccessibleValuePropertyId: i32 = 30093;
pub const UIA_LegacyIAccessibleDescriptionPropertyId: i32 = 30094;
pub const UIA_LegacyIAccessibleRolePropertyId: i32 = 30095;
pub const UIA_LegacyIAccessibleStatePropertyId: i32 = 30096;
pub const UIA_LegacyIAccessibleHelpPropertyId: i32 = 30097;
pub const UIA_LegacyIAccessibleKeyboardShortcutPropertyId: i32 = 30098;
pub const UIA_LegacyIAccessibleSelectionPropertyId: i32 = 30099;
pub const UIA_LegacyIAccessibleDefaultActionPropertyId: i32 = 30100;
pub const UIA_AriaRolePropertyId: i32 = 30101;
pub const UIA_AriaPropertiesPropertyId: i32 = 30102;
pub const UIA_IsDataValidForFormPropertyId: i32 = 30103;
pub const UIA_ControllerForPropertyId: i32 = 30104;
pub const UIA_DescribedByPropertyId: i32 = 30105;
pub const UIA_FlowsToPropertyId: i32 = 30106;
pub const UIA_ProviderDescriptionPropertyId: i32 = 30107;
pub const UIA_IsItemContainerPatternAvailablePropertyId: i32 = 30108;
pub const UIA_IsVirtualizedItemPatternAvailablePropertyId: i32 = 30109;
pub const UIA_IsSynchronizedInputPatternAvailablePropertyId: i32 = 30110;
pub const UIA_OptimizeForVisualContentPropertyId: i32 = 30111;
pub const UIA_IsObjectModelPatternAvailablePropertyId: i32 = 30112;
pub const UIA_AnnotationAnnotationTypeIdPropertyId: i32 = 30113;
pub const UIA_AnnotationAnnotationTypeNamePropertyId: i32 = 30114;
pub const UIA_AnnotationAuthorPropertyId: i32 = 30115;
pub const UIA_AnnotationDateTimePropertyId: i32 = 30116;
pub const UIA_AnnotationTargetPropertyId: i32 = 30117;
pub const UIA_IsAnnotationPatternAvailablePropertyId: i32 = 30118;
pub const UIA_IsTextPattern2AvailablePropertyId: i32 = 30119;
pub const UIA_StylesStyleIdPropertyId: i32 = 30120;
pub const UIA_StylesStyleNamePropertyId: i32 = 30121;
pub const UIA_StylesFillColorPropertyId: i32 = 30122;
pub const UIA_StylesFillPatternStylePropertyId: i32 = 30123;
pub const UIA_StylesShapePropertyId: i32 = 30124;
pub const UIA_StylesFillPatternColorPropertyId: i32 = 30125;
pub const UIA_StylesExtendedPropertiesPropertyId: i32 = 30126;
pub const UIA_IsStylesPatternAvailablePropertyId: i32 = 30127;
pub const UIA_IsSpreadsheetPatternAvailablePropertyId: i32 = 30128;
pub const UIA_SpreadsheetItemFormulaPropertyId: i32 = 30129;
pub const UIA_SpreadsheetItemAnnotationObjectsPropertyId: i32 = 30130;
pub const UIA_SpreadsheetItemAnnotationTypesPropertyId: i32 = 30131;
pub const UIA_IsSpreadsheetItemPatternAvailablePropertyId: i32 = 30132;
pub const UIA_Transform2CanZoomPropertyId: i32 = 30133;
pub const UIA_IsTransformPattern2AvailablePropertyId: i32 = 30134;
pub const UIA_LiveSettingPropertyId: i32 = 30135;
pub const UIA_IsTextChildPatternAvailablePropertyId: i32 = 30136;
pub const UIA_IsDragPatternAvailablePropertyId: i32 = 30137;
pub const UIA_DragIsGrabbedPropertyId: i32 = 30138;
pub const UIA_DragDropEffectPropertyId: i32 = 30139;
pub const UIA_DragDropEffectsPropertyId: i32 = 30140;
pub const UIA_IsDropTargetPatternAvailablePropertyId: i32 = 30141;
pub const UIA_DropTargetDropTargetEffectPropertyId: i32 = 30142;
pub const UIA_DropTargetDropTargetEffectsPropertyId: i32 = 30143;
pub const UIA_DragGrabbedItemsPropertyId: i32 = 30144;
pub const UIA_Transform2ZoomLevelPropertyId: i32 = 30145;
pub const UIA_Transform2ZoomMinimumPropertyId: i32 = 30146;
pub const UIA_Transform2ZoomMaximumPropertyId: i32 = 30147;
pub const UIA_FlowsFromPropertyId: i32 = 30148;
pub const UIA_IsTextEditPatternAvailablePropertyId: i32 = 30149;
pub const UIA_IsPeripheralPropertyId: i32 = 30150;
pub const UIA_IsCustomNavigationPatternAvailablePropertyId: i32 = 30151;
pub const UIA_PositionInSetPropertyId: i32 = 30152;
pub const UIA_SizeOfSetPropertyId: i32 = 30153;
pub const UIA_LevelPropertyId: i32 = 30154;
pub const UIA_AnnotationTypesPropertyId: i32 = 30155;
pub const UIA_AnnotationObjectsPropertyId: i32 = 30156;
pub const UIA_LandmarkTypePropertyId: i32 = 30157;
pub const UIA_LocalizedLandmarkTypePropertyId: i32 = 30158;
pub const UIA_FullDescriptionPropertyId: i32 = 30159;
pub const UIA_FillColorPropertyId: i32 = 30160;
pub const UIA_OutlineColorPropertyId: i32 = 30161;
pub const UIA_FillTypePropertyId: i32 = 30162;
pub const UIA_VisualEffectsPropertyId: i32 = 30163;
pub const UIA_OutlineThicknessPropertyId: i32 = 30164;
pub const UIA_CenterPointPropertyId: i32 = 30165;
pub const UIA_RotationPropertyId: i32 = 30166;
pub const UIA_SizePropertyId: i32 = 30167;
pub const UIA_IsSelectionPattern2AvailablePropertyId: i32 = 30168;
pub const UIA_Selection2FirstSelectedItemPropertyId: i32 = 30169;
pub const UIA_Selection2LastSelectedItemPropertyId: i32 = 30170;
pub const UIA_Selection2CurrentSelectedItemPropertyId: i32 = 30171;
pub const UIA_Selection2ItemCountPropertyId: i32 = 30172;
pub const UIA_HeadingLevelPropertyId: i32 = 30173;
pub const UIA_IsDialogPropertyId: i32 = 30174;
pub const UIA_ButtonControlTypeId: i32 = 50000;
pub const UIA_CalendarControlTypeId: i32 = 50001;
pub const UIA_CheckBoxControlTypeId: i32 = 50002;
pub const UIA_ComboBoxControlTypeId: i32 = 50003;
pub const UIA_EditControlTypeId: i32 = 50004;
pub const UIA_HyperlinkControlTypeId: i32 = 50005;
pub const UIA_ImageControlTypeId: i32 = 50006;
pub const UIA_ListItemControlTypeId: i32 = 50007;
pub const UIA_ListControlTypeId: i32 = 50008;
pub const UIA_MenuControlTypeId: i32 = 50009;
pub const UIA_MenuBarControlTypeId: i32 = 50010;
pub const UIA_MenuItemControlTypeId: i32 = 50011;
pub const UIA_ProgressBarControlTypeId: i32 = 50012;
pub const UIA_RadioButtonControlTypeId: i32 = 50013;
pub const UIA_ScrollBarControlTypeId: i32 = 50014;
pub const UIA_SliderControlTypeId: i32 = 50015;
pub const UIA_SpinnerControlTypeId: i32 = 50016;
pub const UIA_StatusBarControlTypeId: i32 = 50017;
pub const UIA_TabControlTypeId: i32 = 50018;
pub const UIA_TabItemControlTypeId: i32 = 50019;
pub const UIA_TextControlTypeId: i32 = 50020;
pub const UIA_ToolBarControlTypeId: i32 = 50021;
pub const UIA_ToolTipControlTypeId: i32 = 50022;
pub const UIA_TreeControlTypeId: i32 = 50023;
pub const UIA_TreeItemControlTypeId: i32 = 50024;
pub const UIA_CustomControlTypeId: i32 = 50025;
pub const UIA_GroupControlTypeId: i32 = 50026;
pub const UIA_ThumbControlTypeId: i32 = 50027;
pub const UIA_DataGridControlTypeId: i32 = 50028;
pub const UIA_DataItemControlTypeId: i32 = 50029;
pub const UIA_DocumentControlTypeId: i32 = 50030;
pub const UIA_SplitButtonControlTypeId: i32 = 50031;
pub const UIA_WindowControlTypeId: i32 = 50032;
pub const UIA_PaneControlTypeId: i32 = 50033;
pub const UIA_HeaderControlTypeId: i32 = 50034;
pub const UIA_HeaderItemControlTypeId: i32 = 50035;
pub const UIA_TableControlTypeId: i32 = 50036;
pub const UIA_TitleBarControlTypeId: i32 = 50037;
pub const UIA_SeparatorControlTypeId: i32 = 50038;
pub const UIA_SemanticZoomControlTypeId: i32 = 50039;
pub const UIA_AppBarControlTypeId: i32 = 50040;

// ---- IUIAutomationCondition : IUnknown
#[repr(C)]
pub struct IUIAutomationConditionVtbl {
    pub base: IUnknownVtbl,
}
pub enum IUIAutomationCondition {}
unsafe impl Interface for IUIAutomationCondition {
    const IID: GUID = GUID { data1: 0x352ffba8, data2: 0x0973, data3: 0x437c, data4: [0xa6, 0x1f, 0xf6, 0x4c, 0xaf, 0xd8, 0x1d, 0xf9] };
    type Vtbl = IUIAutomationConditionVtbl;
}
impl ComPtr<IUIAutomationCondition> {
}

// ---- IUIAutomationCacheRequest : IUnknown
#[repr(C)]
pub struct IUIAutomationCacheRequestVtbl {
    pub base: IUnknownVtbl,
    pub AddProperty: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID) -> HRESULT,
    pub AddPattern: unsafe extern "system" fn(this: *mut c_void, patternId: PATTERNID) -> HRESULT,
    pub Clone: unsafe extern "system" fn(this: *mut c_void, clonedRequest: *mut *mut c_void) -> HRESULT,
    pub get_TreeScope: unsafe extern "system" fn(this: *mut c_void, scope: *mut TreeScope) -> HRESULT,
    pub put_TreeScope: unsafe extern "system" fn(this: *mut c_void, scope: TreeScope) -> HRESULT,
    pub get_TreeFilter: unsafe extern "system" fn(this: *mut c_void, filter: *mut *mut c_void) -> HRESULT,
    pub put_TreeFilter: unsafe extern "system" fn(this: *mut c_void, filter: *mut c_void) -> HRESULT,
    pub get_AutomationElementMode: unsafe extern "system" fn(this: *mut c_void, mode: *mut AutomationElementMode) -> HRESULT,
    pub put_AutomationElementMode: unsafe extern "system" fn(this: *mut c_void, mode: AutomationElementMode) -> HRESULT,
}
pub enum IUIAutomationCacheRequest {}
unsafe impl Interface for IUIAutomationCacheRequest {
    const IID: GUID = GUID { data1: 0xb32a92b5, data2: 0xbc25, data3: 0x4078, data4: [0x9c, 0x08, 0xd7, 0xee, 0x95, 0xc4, 0x8e, 0x03] };
    type Vtbl = IUIAutomationCacheRequestVtbl;
}
impl ComPtr<IUIAutomationCacheRequest> {
    #[inline]
    pub unsafe fn AddProperty(&self, propertyId: PROPERTYID) -> HRESULT {
        (self.vtbl().AddProperty)(self.as_raw(), propertyId)
    }
    #[inline]
    pub unsafe fn AddPattern(&self, patternId: PATTERNID) -> HRESULT {
        (self.vtbl().AddPattern)(self.as_raw(), patternId)
    }
    #[inline]
    pub unsafe fn Clone(&self, clonedRequest: *mut *mut c_void) -> HRESULT {
        (self.vtbl().Clone)(self.as_raw(), clonedRequest)
    }
    #[inline]
    pub unsafe fn get_TreeScope(&self, scope: *mut TreeScope) -> HRESULT {
        (self.vtbl().get_TreeScope)(self.as_raw(), scope)
    }
    #[inline]
    pub unsafe fn put_TreeScope(&self, scope: TreeScope) -> HRESULT {
        (self.vtbl().put_TreeScope)(self.as_raw(), scope)
    }
    #[inline]
    pub unsafe fn get_TreeFilter(&self, filter: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_TreeFilter)(self.as_raw(), filter)
    }
    #[inline]
    pub unsafe fn put_TreeFilter(&self, filter: *mut c_void) -> HRESULT {
        (self.vtbl().put_TreeFilter)(self.as_raw(), filter)
    }
    #[inline]
    pub unsafe fn get_AutomationElementMode(&self, mode: *mut AutomationElementMode) -> HRESULT {
        (self.vtbl().get_AutomationElementMode)(self.as_raw(), mode)
    }
    #[inline]
    pub unsafe fn put_AutomationElementMode(&self, mode: AutomationElementMode) -> HRESULT {
        (self.vtbl().put_AutomationElementMode)(self.as_raw(), mode)
    }
}

// ---- IUIAutomationInvokePattern : IUnknown
#[repr(C)]
pub struct IUIAutomationInvokePatternVtbl {
    pub base: IUnknownVtbl,
    pub Invoke: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
}
pub enum IUIAutomationInvokePattern {}
unsafe impl Interface for IUIAutomationInvokePattern {
    const IID: GUID = GUID { data1: 0xfb377fbe, data2: 0x8ea6, data3: 0x46d5, data4: [0x9c, 0x73, 0x64, 0x99, 0x64, 0x2d, 0x30, 0x59] };
    type Vtbl = IUIAutomationInvokePatternVtbl;
}
impl ComPtr<IUIAutomationInvokePattern> {
    #[inline]
    pub unsafe fn Invoke(&self) -> HRESULT {
        (self.vtbl().Invoke)(self.as_raw())
    }
}

// ---- IUIAutomationExpandCollapsePattern : IUnknown
#[repr(C)]
pub struct IUIAutomationExpandCollapsePatternVtbl {
    pub base: IUnknownVtbl,
    pub Expand: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub Collapse: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub get_CurrentExpandCollapseState: unsafe extern "system" fn(this: *mut c_void, retVal: *mut ExpandCollapseState) -> HRESULT,
    pub get_CachedExpandCollapseState: unsafe extern "system" fn(this: *mut c_void, retVal: *mut ExpandCollapseState) -> HRESULT,
}
pub enum IUIAutomationExpandCollapsePattern {}
unsafe impl Interface for IUIAutomationExpandCollapsePattern {
    const IID: GUID = GUID { data1: 0x619be086, data2: 0x1f4e, data3: 0x4ee4, data4: [0xba, 0xfa, 0x21, 0x01, 0x28, 0x73, 0x87, 0x30] };
    type Vtbl = IUIAutomationExpandCollapsePatternVtbl;
}
impl ComPtr<IUIAutomationExpandCollapsePattern> {
    #[inline]
    pub unsafe fn Expand(&self) -> HRESULT {
        (self.vtbl().Expand)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Collapse(&self) -> HRESULT {
        (self.vtbl().Collapse)(self.as_raw())
    }
    #[inline]
    pub unsafe fn get_CurrentExpandCollapseState(&self, retVal: *mut ExpandCollapseState) -> HRESULT {
        (self.vtbl().get_CurrentExpandCollapseState)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedExpandCollapseState(&self, retVal: *mut ExpandCollapseState) -> HRESULT {
        (self.vtbl().get_CachedExpandCollapseState)(self.as_raw(), retVal)
    }
}

// ---- IUIAutomationSelectionPattern : IUnknown
#[repr(C)]
pub struct IUIAutomationSelectionPatternVtbl {
    pub base: IUnknownVtbl,
    pub GetCurrentSelection: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CurrentCanSelectMultiple: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentIsSelectionRequired: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub GetCachedSelection: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CachedCanSelectMultiple: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedIsSelectionRequired: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
}
pub enum IUIAutomationSelectionPattern {}
unsafe impl Interface for IUIAutomationSelectionPattern {
    const IID: GUID = GUID { data1: 0x5ed5202e, data2: 0xb2ac, data3: 0x47a6, data4: [0xb6, 0x38, 0x4b, 0x0b, 0xf1, 0x40, 0xd7, 0x8e] };
    type Vtbl = IUIAutomationSelectionPatternVtbl;
}
impl ComPtr<IUIAutomationSelectionPattern> {
    #[inline]
    pub unsafe fn GetCurrentSelection(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCurrentSelection)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentCanSelectMultiple(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentCanSelectMultiple)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsSelectionRequired(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsSelectionRequired)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn GetCachedSelection(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedSelection)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedCanSelectMultiple(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedCanSelectMultiple)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsSelectionRequired(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsSelectionRequired)(self.as_raw(), retVal)
    }
}

// ---- IUIAutomationSelectionItemPattern : IUnknown
#[repr(C)]
pub struct IUIAutomationSelectionItemPatternVtbl {
    pub base: IUnknownVtbl,
    pub Select: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub AddToSelection: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub RemoveFromSelection: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub get_CurrentIsSelected: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentSelectionContainer: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CachedIsSelected: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedSelectionContainer: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
}
pub enum IUIAutomationSelectionItemPattern {}
unsafe impl Interface for IUIAutomationSelectionItemPattern {
    const IID: GUID = GUID { data1: 0xa8efa66a, data2: 0x0fda, data3: 0x421a, data4: [0x91, 0x94, 0x38, 0x02, 0x1f, 0x35, 0x78, 0xea] };
    type Vtbl = IUIAutomationSelectionItemPatternVtbl;
}
impl ComPtr<IUIAutomationSelectionItemPattern> {
    #[inline]
    pub unsafe fn Select(&self) -> HRESULT {
        (self.vtbl().Select)(self.as_raw())
    }
    #[inline]
    pub unsafe fn AddToSelection(&self) -> HRESULT {
        (self.vtbl().AddToSelection)(self.as_raw())
    }
    #[inline]
    pub unsafe fn RemoveFromSelection(&self) -> HRESULT {
        (self.vtbl().RemoveFromSelection)(self.as_raw())
    }
    #[inline]
    pub unsafe fn get_CurrentIsSelected(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsSelected)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentSelectionContainer(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CurrentSelectionContainer)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsSelected(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsSelected)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedSelectionContainer(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CachedSelectionContainer)(self.as_raw(), retVal)
    }
}

// ---- IUIAutomationTogglePattern : IUnknown
#[repr(C)]
pub struct IUIAutomationTogglePatternVtbl {
    pub base: IUnknownVtbl,
    pub Toggle: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub get_CurrentToggleState: unsafe extern "system" fn(this: *mut c_void, retVal: *mut ToggleState) -> HRESULT,
    pub get_CachedToggleState: unsafe extern "system" fn(this: *mut c_void, retVal: *mut ToggleState) -> HRESULT,
}
pub enum IUIAutomationTogglePattern {}
unsafe impl Interface for IUIAutomationTogglePattern {
    const IID: GUID = GUID { data1: 0x94cf8058, data2: 0x9b8d, data3: 0x4ab9, data4: [0x8b, 0xfd, 0x4c, 0xd0, 0xa3, 0x3c, 0x8c, 0x70] };
    type Vtbl = IUIAutomationTogglePatternVtbl;
}
impl ComPtr<IUIAutomationTogglePattern> {
    #[inline]
    pub unsafe fn Toggle(&self) -> HRESULT {
        (self.vtbl().Toggle)(self.as_raw())
    }
    #[inline]
    pub unsafe fn get_CurrentToggleState(&self, retVal: *mut ToggleState) -> HRESULT {
        (self.vtbl().get_CurrentToggleState)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedToggleState(&self, retVal: *mut ToggleState) -> HRESULT {
        (self.vtbl().get_CachedToggleState)(self.as_raw(), retVal)
    }
}

// ---- IUIAutomationValuePattern : IUnknown
#[repr(C)]
pub struct IUIAutomationValuePatternVtbl {
    pub base: IUnknownVtbl,
    pub SetValue: unsafe extern "system" fn(this: *mut c_void, val: BSTR) -> HRESULT,
    pub get_CurrentValue: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentIsReadOnly: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedValue: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedIsReadOnly: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
}
pub enum IUIAutomationValuePattern {}
unsafe impl Interface for IUIAutomationValuePattern {
    const IID: GUID = GUID { data1: 0xa94cd8b1, data2: 0x0844, data3: 0x4cd6, data4: [0x9d, 0x2d, 0x64, 0x05, 0x37, 0xab, 0x39, 0xe9] };
    type Vtbl = IUIAutomationValuePatternVtbl;
}
impl ComPtr<IUIAutomationValuePattern> {
    #[inline]
    pub unsafe fn SetValue(&self, val: BSTR) -> HRESULT {
        (self.vtbl().SetValue)(self.as_raw(), val)
    }
    #[inline]
    pub unsafe fn get_CurrentValue(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentValue)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsReadOnly(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsReadOnly)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedValue(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedValue)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsReadOnly(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsReadOnly)(self.as_raw(), retVal)
    }
}

// ---- IUIAutomationElement : IUnknown
#[repr(C)]
pub struct IUIAutomationElementVtbl {
    pub base: IUnknownVtbl,
    pub SetFocus: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub GetRuntimeId: unsafe extern "system" fn(this: *mut c_void, runtimeId: *mut *mut c_void) -> HRESULT,
    pub FindFirst: unsafe extern "system" fn(this: *mut c_void, scope: TreeScope, condition: *mut c_void, found: *mut *mut c_void) -> HRESULT,
    pub FindAll: unsafe extern "system" fn(this: *mut c_void, scope: TreeScope, condition: *mut c_void, found: *mut *mut c_void) -> HRESULT,
    pub FindFirstBuildCache: unsafe extern "system" fn(this: *mut c_void, scope: TreeScope, condition: *mut c_void, cacheRequest: *mut c_void, found: *mut *mut c_void) -> HRESULT,
    pub FindAllBuildCache: unsafe extern "system" fn(this: *mut c_void, scope: TreeScope, condition: *mut c_void, cacheRequest: *mut c_void, found: *mut *mut c_void) -> HRESULT,
    pub BuildUpdatedCache: unsafe extern "system" fn(this: *mut c_void, cacheRequest: *mut c_void, updatedElement: *mut *mut c_void) -> HRESULT,
    pub GetCurrentPropertyValue: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID, retVal: *mut VARIANT) -> HRESULT,
    pub GetCurrentPropertyValueEx: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID, ignoreDefaultValue: i32, retVal: *mut VARIANT) -> HRESULT,
    pub GetCachedPropertyValue: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID, retVal: *mut VARIANT) -> HRESULT,
    pub GetCachedPropertyValueEx: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID, ignoreDefaultValue: i32, retVal: *mut VARIANT) -> HRESULT,
    pub GetCurrentPatternAs: unsafe extern "system" fn(this: *mut c_void, patternId: PATTERNID, riid: *const GUID, patternObject: *mut *mut c_void) -> HRESULT,
    pub GetCachedPatternAs: unsafe extern "system" fn(this: *mut c_void, patternId: PATTERNID, riid: *const GUID, patternObject: *mut *mut c_void) -> HRESULT,
    pub GetCurrentPattern: unsafe extern "system" fn(this: *mut c_void, patternId: PATTERNID, patternObject: *mut *mut c_void) -> HRESULT,
    pub GetCachedPattern: unsafe extern "system" fn(this: *mut c_void, patternId: PATTERNID, patternObject: *mut *mut c_void) -> HRESULT,
    pub GetCachedParent: unsafe extern "system" fn(this: *mut c_void, parent: *mut *mut c_void) -> HRESULT,
    pub GetCachedChildren: unsafe extern "system" fn(this: *mut c_void, children: *mut *mut c_void) -> HRESULT,
    pub get_CurrentProcessId: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentControlType: unsafe extern "system" fn(this: *mut c_void, retVal: *mut CONTROLTYPEID) -> HRESULT,
    pub get_CurrentLocalizedControlType: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentName: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentAcceleratorKey: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentAccessKey: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentHasKeyboardFocus: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentIsKeyboardFocusable: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentIsEnabled: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentAutomationId: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentClassName: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentHelpText: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentCulture: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentIsControlElement: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentIsContentElement: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentIsPassword: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentNativeWindowHandle: unsafe extern "system" fn(this: *mut c_void, retVal: *mut UIA_HWND) -> HRESULT,
    pub get_CurrentItemType: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentIsOffscreen: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentOrientation: unsafe extern "system" fn(this: *mut c_void, retVal: *mut c_void) -> HRESULT,
    pub get_CurrentFrameworkId: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentIsRequiredForForm: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentItemStatus: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentBoundingRectangle: unsafe extern "system" fn(this: *mut c_void, retVal: *mut RECT) -> HRESULT,
    pub get_CurrentLabeledBy: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CurrentAriaRole: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentAriaProperties: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CurrentIsDataValidForForm: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CurrentControllerFor: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CurrentDescribedBy: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CurrentFlowsTo: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CurrentProviderDescription: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedProcessId: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedControlType: unsafe extern "system" fn(this: *mut c_void, retVal: *mut CONTROLTYPEID) -> HRESULT,
    pub get_CachedLocalizedControlType: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedName: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedAcceleratorKey: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedAccessKey: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedHasKeyboardFocus: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedIsKeyboardFocusable: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedIsEnabled: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedAutomationId: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedClassName: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedHelpText: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedCulture: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedIsControlElement: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedIsContentElement: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedIsPassword: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedNativeWindowHandle: unsafe extern "system" fn(this: *mut c_void, retVal: *mut UIA_HWND) -> HRESULT,
    pub get_CachedItemType: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedIsOffscreen: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedOrientation: unsafe extern "system" fn(this: *mut c_void, retVal: *mut c_void) -> HRESULT,
    pub get_CachedFrameworkId: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedIsRequiredForForm: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedItemStatus: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedBoundingRectangle: unsafe extern "system" fn(this: *mut c_void, retVal: *mut RECT) -> HRESULT,
    pub get_CachedLabeledBy: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CachedAriaRole: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedAriaProperties: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub get_CachedIsDataValidForForm: unsafe extern "system" fn(this: *mut c_void, retVal: *mut i32) -> HRESULT,
    pub get_CachedControllerFor: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CachedDescribedBy: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CachedFlowsTo: unsafe extern "system" fn(this: *mut c_void, retVal: *mut *mut c_void) -> HRESULT,
    pub get_CachedProviderDescription: unsafe extern "system" fn(this: *mut c_void, retVal: *mut BSTR) -> HRESULT,
    pub GetClickablePoint: unsafe extern "system" fn(this: *mut c_void, clickable: *mut POINT, gotClickable: *mut i32) -> HRESULT,
}
pub enum IUIAutomationElement {}
unsafe impl Interface for IUIAutomationElement {
    const IID: GUID = GUID { data1: 0xd22108aa, data2: 0x8ac5, data3: 0x49a5, data4: [0x83, 0x7b, 0x37, 0xbb, 0xb3, 0xd7, 0x59, 0x1e] };
    type Vtbl = IUIAutomationElementVtbl;
}
impl ComPtr<IUIAutomationElement> {
    #[inline]
    pub unsafe fn SetFocus(&self) -> HRESULT {
        (self.vtbl().SetFocus)(self.as_raw())
    }
    #[inline]
    pub unsafe fn GetRuntimeId(&self, runtimeId: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRuntimeId)(self.as_raw(), runtimeId)
    }
    #[inline]
    pub unsafe fn FindFirst(&self, scope: TreeScope, condition: *mut c_void, found: *mut *mut c_void) -> HRESULT {
        (self.vtbl().FindFirst)(self.as_raw(), scope, condition, found)
    }
    #[inline]
    pub unsafe fn FindAll(&self, scope: TreeScope, condition: *mut c_void, found: *mut *mut c_void) -> HRESULT {
        (self.vtbl().FindAll)(self.as_raw(), scope, condition, found)
    }
    #[inline]
    pub unsafe fn FindFirstBuildCache(&self, scope: TreeScope, condition: *mut c_void, cacheRequest: *mut c_void, found: *mut *mut c_void) -> HRESULT {
        (self.vtbl().FindFirstBuildCache)(self.as_raw(), scope, condition, cacheRequest, found)
    }
    #[inline]
    pub unsafe fn FindAllBuildCache(&self, scope: TreeScope, condition: *mut c_void, cacheRequest: *mut c_void, found: *mut *mut c_void) -> HRESULT {
        (self.vtbl().FindAllBuildCache)(self.as_raw(), scope, condition, cacheRequest, found)
    }
    #[inline]
    pub unsafe fn BuildUpdatedCache(&self, cacheRequest: *mut c_void, updatedElement: *mut *mut c_void) -> HRESULT {
        (self.vtbl().BuildUpdatedCache)(self.as_raw(), cacheRequest, updatedElement)
    }
    #[inline]
    pub unsafe fn GetCurrentPropertyValue(&self, propertyId: PROPERTYID, retVal: *mut VARIANT) -> HRESULT {
        (self.vtbl().GetCurrentPropertyValue)(self.as_raw(), propertyId, retVal)
    }
    #[inline]
    pub unsafe fn GetCurrentPropertyValueEx(&self, propertyId: PROPERTYID, ignoreDefaultValue: i32, retVal: *mut VARIANT) -> HRESULT {
        (self.vtbl().GetCurrentPropertyValueEx)(self.as_raw(), propertyId, ignoreDefaultValue, retVal)
    }
    #[inline]
    pub unsafe fn GetCachedPropertyValue(&self, propertyId: PROPERTYID, retVal: *mut VARIANT) -> HRESULT {
        (self.vtbl().GetCachedPropertyValue)(self.as_raw(), propertyId, retVal)
    }
    #[inline]
    pub unsafe fn GetCachedPropertyValueEx(&self, propertyId: PROPERTYID, ignoreDefaultValue: i32, retVal: *mut VARIANT) -> HRESULT {
        (self.vtbl().GetCachedPropertyValueEx)(self.as_raw(), propertyId, ignoreDefaultValue, retVal)
    }
    #[inline]
    pub unsafe fn GetCurrentPatternAs(&self, patternId: PATTERNID, riid: *const GUID, patternObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCurrentPatternAs)(self.as_raw(), patternId, riid, patternObject)
    }
    #[inline]
    pub unsafe fn GetCachedPatternAs(&self, patternId: PATTERNID, riid: *const GUID, patternObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedPatternAs)(self.as_raw(), patternId, riid, patternObject)
    }
    #[inline]
    pub unsafe fn GetCurrentPattern(&self, patternId: PATTERNID, patternObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCurrentPattern)(self.as_raw(), patternId, patternObject)
    }
    #[inline]
    pub unsafe fn GetCachedPattern(&self, patternId: PATTERNID, patternObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedPattern)(self.as_raw(), patternId, patternObject)
    }
    #[inline]
    pub unsafe fn GetCachedParent(&self, parent: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedParent)(self.as_raw(), parent)
    }
    #[inline]
    pub unsafe fn GetCachedChildren(&self, children: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedChildren)(self.as_raw(), children)
    }
    #[inline]
    pub unsafe fn get_CurrentProcessId(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentProcessId)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentControlType(&self, retVal: *mut CONTROLTYPEID) -> HRESULT {
        (self.vtbl().get_CurrentControlType)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentLocalizedControlType(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentLocalizedControlType)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentName(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentName)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentAcceleratorKey(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentAcceleratorKey)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentAccessKey(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentAccessKey)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentHasKeyboardFocus(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentHasKeyboardFocus)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsKeyboardFocusable(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsKeyboardFocusable)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsEnabled(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsEnabled)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentAutomationId(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentAutomationId)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentClassName(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentClassName)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentHelpText(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentHelpText)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentCulture(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentCulture)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsControlElement(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsControlElement)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsContentElement(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsContentElement)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsPassword(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsPassword)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentNativeWindowHandle(&self, retVal: *mut UIA_HWND) -> HRESULT {
        (self.vtbl().get_CurrentNativeWindowHandle)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentItemType(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentItemType)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsOffscreen(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsOffscreen)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentOrientation(&self, retVal: *mut c_void) -> HRESULT {
        (self.vtbl().get_CurrentOrientation)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentFrameworkId(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentFrameworkId)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsRequiredForForm(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsRequiredForForm)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentItemStatus(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentItemStatus)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentBoundingRectangle(&self, retVal: *mut RECT) -> HRESULT {
        (self.vtbl().get_CurrentBoundingRectangle)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentLabeledBy(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CurrentLabeledBy)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentAriaRole(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentAriaRole)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentAriaProperties(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentAriaProperties)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentIsDataValidForForm(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CurrentIsDataValidForForm)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentControllerFor(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CurrentControllerFor)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentDescribedBy(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CurrentDescribedBy)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentFlowsTo(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CurrentFlowsTo)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CurrentProviderDescription(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CurrentProviderDescription)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedProcessId(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedProcessId)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedControlType(&self, retVal: *mut CONTROLTYPEID) -> HRESULT {
        (self.vtbl().get_CachedControlType)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedLocalizedControlType(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedLocalizedControlType)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedName(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedName)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedAcceleratorKey(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedAcceleratorKey)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedAccessKey(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedAccessKey)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedHasKeyboardFocus(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedHasKeyboardFocus)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsKeyboardFocusable(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsKeyboardFocusable)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsEnabled(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsEnabled)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedAutomationId(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedAutomationId)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedClassName(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedClassName)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedHelpText(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedHelpText)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedCulture(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedCulture)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsControlElement(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsControlElement)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsContentElement(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsContentElement)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsPassword(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsPassword)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedNativeWindowHandle(&self, retVal: *mut UIA_HWND) -> HRESULT {
        (self.vtbl().get_CachedNativeWindowHandle)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedItemType(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedItemType)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsOffscreen(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsOffscreen)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedOrientation(&self, retVal: *mut c_void) -> HRESULT {
        (self.vtbl().get_CachedOrientation)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedFrameworkId(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedFrameworkId)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsRequiredForForm(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsRequiredForForm)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedItemStatus(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedItemStatus)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedBoundingRectangle(&self, retVal: *mut RECT) -> HRESULT {
        (self.vtbl().get_CachedBoundingRectangle)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedLabeledBy(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CachedLabeledBy)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedAriaRole(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedAriaRole)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedAriaProperties(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedAriaProperties)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedIsDataValidForForm(&self, retVal: *mut i32) -> HRESULT {
        (self.vtbl().get_CachedIsDataValidForForm)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedControllerFor(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CachedControllerFor)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedDescribedBy(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CachedDescribedBy)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedFlowsTo(&self, retVal: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_CachedFlowsTo)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn get_CachedProviderDescription(&self, retVal: *mut BSTR) -> HRESULT {
        (self.vtbl().get_CachedProviderDescription)(self.as_raw(), retVal)
    }
    #[inline]
    pub unsafe fn GetClickablePoint(&self, clickable: *mut POINT, gotClickable: *mut i32) -> HRESULT {
        (self.vtbl().GetClickablePoint)(self.as_raw(), clickable, gotClickable)
    }
}

// ---- IUIAutomationElementArray : IUnknown
#[repr(C)]
pub struct IUIAutomationElementArrayVtbl {
    pub base: IUnknownVtbl,
    pub get_Length: unsafe extern "system" fn(this: *mut c_void, length: *mut i32) -> HRESULT,
    pub GetElement: unsafe extern "system" fn(this: *mut c_void, index: i32, element: *mut *mut c_void) -> HRESULT,
}
pub enum IUIAutomationElementArray {}
unsafe impl Interface for IUIAutomationElementArray {
    const IID: GUID = GUID { data1: 0x14314595, data2: 0xb4bc, data3: 0x4055, data4: [0x95, 0xf2, 0x58, 0xf2, 0xe4, 0x2c, 0x98, 0x55] };
    type Vtbl = IUIAutomationElementArrayVtbl;
}
impl ComPtr<IUIAutomationElementArray> {
    #[inline]
    pub unsafe fn get_Length(&self, length: *mut i32) -> HRESULT {
        (self.vtbl().get_Length)(self.as_raw(), length)
    }
    #[inline]
    pub unsafe fn GetElement(&self, index: i32, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetElement)(self.as_raw(), index, element)
    }
}

// ---- IUIAutomation : IUnknown
#[repr(C)]
pub struct IUIAutomationVtbl {
    pub base: IUnknownVtbl,
    pub CompareElements: unsafe extern "system" fn(this: *mut c_void, el1: *mut c_void, el2: *mut c_void, areSame: *mut i32) -> HRESULT,
    pub CompareRuntimeIds: unsafe extern "system" fn(this: *mut c_void, runtimeId1: *mut c_void, runtimeId2: *mut c_void, areSame: *mut i32) -> HRESULT,
    pub GetRootElement: unsafe extern "system" fn(this: *mut c_void, root: *mut *mut c_void) -> HRESULT,
    pub ElementFromHandle: unsafe extern "system" fn(this: *mut c_void, hwnd: UIA_HWND, element: *mut *mut c_void) -> HRESULT,
    pub ElementFromPoint: unsafe extern "system" fn(this: *mut c_void, pt: POINT, element: *mut *mut c_void) -> HRESULT,
    pub GetFocusedElement: unsafe extern "system" fn(this: *mut c_void, element: *mut *mut c_void) -> HRESULT,
    pub GetRootElementBuildCache: unsafe extern "system" fn(this: *mut c_void, cacheRequest: *mut c_void, root: *mut *mut c_void) -> HRESULT,
    pub ElementFromHandleBuildCache: unsafe extern "system" fn(this: *mut c_void, hwnd: UIA_HWND, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT,
    pub ElementFromPointBuildCache: unsafe extern "system" fn(this: *mut c_void, pt: POINT, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT,
    pub GetFocusedElementBuildCache: unsafe extern "system" fn(this: *mut c_void, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT,
    pub CreateTreeWalker: unsafe extern "system" fn(this: *mut c_void, pCondition: *mut c_void, walker: *mut *mut c_void) -> HRESULT,
    pub get_ControlViewWalker: unsafe extern "system" fn(this: *mut c_void, walker: *mut *mut c_void) -> HRESULT,
    pub get_ContentViewWalker: unsafe extern "system" fn(this: *mut c_void, walker: *mut *mut c_void) -> HRESULT,
    pub get_RawViewWalker: unsafe extern "system" fn(this: *mut c_void, walker: *mut *mut c_void) -> HRESULT,
    pub get_RawViewCondition: unsafe extern "system" fn(this: *mut c_void, condition: *mut *mut c_void) -> HRESULT,
    pub get_ControlViewCondition: unsafe extern "system" fn(this: *mut c_void, condition: *mut *mut c_void) -> HRESULT,
    pub get_ContentViewCondition: unsafe extern "system" fn(this: *mut c_void, condition: *mut *mut c_void) -> HRESULT,
    pub CreateCacheRequest: unsafe extern "system" fn(this: *mut c_void, cacheRequest: *mut *mut c_void) -> HRESULT,
    pub CreateTrueCondition: unsafe extern "system" fn(this: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateFalseCondition: unsafe extern "system" fn(this: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreatePropertyCondition: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID, value: VARIANT, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreatePropertyConditionEx: unsafe extern "system" fn(this: *mut c_void, propertyId: PROPERTYID, value: VARIANT, flags: PropertyConditionFlags, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateAndCondition: unsafe extern "system" fn(this: *mut c_void, condition1: *mut c_void, condition2: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateAndConditionFromArray: unsafe extern "system" fn(this: *mut c_void, conditions: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateAndConditionFromNativeArray: unsafe extern "system" fn(this: *mut c_void, conditions: *mut *mut c_void, conditionCount: i32, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateOrCondition: unsafe extern "system" fn(this: *mut c_void, condition1: *mut c_void, condition2: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateOrConditionFromArray: unsafe extern "system" fn(this: *mut c_void, conditions: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateOrConditionFromNativeArray: unsafe extern "system" fn(this: *mut c_void, conditions: *mut *mut c_void, conditionCount: i32, newCondition: *mut *mut c_void) -> HRESULT,
    pub CreateNotCondition: unsafe extern "system" fn(this: *mut c_void, condition: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT,
    pub AddAutomationEventHandler: unsafe extern "system" fn(this: *mut c_void, eventId: EVENTID, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub RemoveAutomationEventHandler: unsafe extern "system" fn(this: *mut c_void, eventId: EVENTID, element: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub AddPropertyChangedEventHandlerNativeArray: unsafe extern "system" fn(this: *mut c_void, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void, propertyArray: *mut PROPERTYID, propertyCount: i32) -> HRESULT,
    pub AddPropertyChangedEventHandler: unsafe extern "system" fn(this: *mut c_void, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void, propertyArray: *mut c_void) -> HRESULT,
    pub RemovePropertyChangedEventHandler: unsafe extern "system" fn(this: *mut c_void, element: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub AddStructureChangedEventHandler: unsafe extern "system" fn(this: *mut c_void, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub RemoveStructureChangedEventHandler: unsafe extern "system" fn(this: *mut c_void, element: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub AddFocusChangedEventHandler: unsafe extern "system" fn(this: *mut c_void, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub RemoveFocusChangedEventHandler: unsafe extern "system" fn(this: *mut c_void, handler: *mut c_void) -> HRESULT,
    pub RemoveAllEventHandlers: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub IntNativeArrayToSafeArray: unsafe extern "system" fn(this: *mut c_void, array: *mut i32, arrayCount: i32, safeArray: *mut *mut c_void) -> HRESULT,
    pub IntSafeArrayToNativeArray: unsafe extern "system" fn(this: *mut c_void, intArray: *mut c_void, array: *mut *mut i32, arrayCount: *mut i32) -> HRESULT,
    pub RectToVariant: unsafe extern "system" fn(this: *mut c_void, rc: RECT, var: *mut VARIANT) -> HRESULT,
    pub VariantToRect: unsafe extern "system" fn(this: *mut c_void, var: VARIANT, rc: *mut RECT) -> HRESULT,
    pub SafeArrayToRectNativeArray: unsafe extern "system" fn(this: *mut c_void, rects: *mut c_void, rectArray: *mut *mut RECT, rectArrayCount: *mut i32) -> HRESULT,
    pub CreateProxyFactoryEntry: unsafe extern "system" fn(this: *mut c_void, factory: *mut c_void, factoryEntry: *mut *mut c_void) -> HRESULT,
    pub get_ProxyFactoryMapping: unsafe extern "system" fn(this: *mut c_void, factoryMapping: *mut *mut c_void) -> HRESULT,
    pub GetPropertyProgrammaticName: unsafe extern "system" fn(this: *mut c_void, property: PROPERTYID, name: *mut BSTR) -> HRESULT,
    pub GetPatternProgrammaticName: unsafe extern "system" fn(this: *mut c_void, pattern: PATTERNID, name: *mut BSTR) -> HRESULT,
    pub PollForPotentialSupportedPatterns: unsafe extern "system" fn(this: *mut c_void, pElement: *mut c_void, patternIds: *mut *mut c_void, patternNames: *mut *mut c_void) -> HRESULT,
    pub PollForPotentialSupportedProperties: unsafe extern "system" fn(this: *mut c_void, pElement: *mut c_void, propertyIds: *mut *mut c_void, propertyNames: *mut *mut c_void) -> HRESULT,
    pub CheckNotSupported: unsafe extern "system" fn(this: *mut c_void, value: VARIANT, isNotSupported: *mut i32) -> HRESULT,
    pub get_ReservedNotSupportedValue: unsafe extern "system" fn(this: *mut c_void, notSupportedValue: *mut *mut c_void) -> HRESULT,
    pub get_ReservedMixedAttributeValue: unsafe extern "system" fn(this: *mut c_void, mixedAttributeValue: *mut *mut c_void) -> HRESULT,
    pub ElementFromIAccessible: unsafe extern "system" fn(this: *mut c_void, accessible: *mut c_void, childId: i32, element: *mut *mut c_void) -> HRESULT,
    pub ElementFromIAccessibleBuildCache: unsafe extern "system" fn(this: *mut c_void, accessible: *mut c_void, childId: i32, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT,
}
pub enum IUIAutomation {}
unsafe impl Interface for IUIAutomation {
    const IID: GUID = GUID { data1: 0x30cbe57d, data2: 0xd9d0, data3: 0x452a, data4: [0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee] };
    type Vtbl = IUIAutomationVtbl;
}
impl ComPtr<IUIAutomation> {
    #[inline]
    pub unsafe fn CompareElements(&self, el1: *mut c_void, el2: *mut c_void, areSame: *mut i32) -> HRESULT {
        (self.vtbl().CompareElements)(self.as_raw(), el1, el2, areSame)
    }
    #[inline]
    pub unsafe fn CompareRuntimeIds(&self, runtimeId1: *mut c_void, runtimeId2: *mut c_void, areSame: *mut i32) -> HRESULT {
        (self.vtbl().CompareRuntimeIds)(self.as_raw(), runtimeId1, runtimeId2, areSame)
    }
    #[inline]
    pub unsafe fn GetRootElement(&self, root: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRootElement)(self.as_raw(), root)
    }
    #[inline]
    pub unsafe fn ElementFromHandle(&self, hwnd: UIA_HWND, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ElementFromHandle)(self.as_raw(), hwnd, element)
    }
    #[inline]
    pub unsafe fn ElementFromPoint(&self, pt: POINT, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ElementFromPoint)(self.as_raw(), pt, element)
    }
    #[inline]
    pub unsafe fn GetFocusedElement(&self, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFocusedElement)(self.as_raw(), element)
    }
    #[inline]
    pub unsafe fn GetRootElementBuildCache(&self, cacheRequest: *mut c_void, root: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRootElementBuildCache)(self.as_raw(), cacheRequest, root)
    }
    #[inline]
    pub unsafe fn ElementFromHandleBuildCache(&self, hwnd: UIA_HWND, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ElementFromHandleBuildCache)(self.as_raw(), hwnd, cacheRequest, element)
    }
    #[inline]
    pub unsafe fn ElementFromPointBuildCache(&self, pt: POINT, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ElementFromPointBuildCache)(self.as_raw(), pt, cacheRequest, element)
    }
    #[inline]
    pub unsafe fn GetFocusedElementBuildCache(&self, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFocusedElementBuildCache)(self.as_raw(), cacheRequest, element)
    }
    #[inline]
    pub unsafe fn CreateTreeWalker(&self, pCondition: *mut c_void, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateTreeWalker)(self.as_raw(), pCondition, walker)
    }
    #[inline]
    pub unsafe fn get_ControlViewWalker(&self, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ControlViewWalker)(self.as_raw(), walker)
    }
    #[inline]
    pub unsafe fn get_ContentViewWalker(&self, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ContentViewWalker)(self.as_raw(), walker)
    }
    #[inline]
    pub unsafe fn get_RawViewWalker(&self, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_RawViewWalker)(self.as_raw(), walker)
    }
    #[inline]
    pub unsafe fn get_RawViewCondition(&self, condition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_RawViewCondition)(self.as_raw(), condition)
    }
    #[inline]
    pub unsafe fn get_ControlViewCondition(&self, condition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ControlViewCondition)(self.as_raw(), condition)
    }
    #[inline]
    pub unsafe fn get_ContentViewCondition(&self, condition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ContentViewCondition)(self.as_raw(), condition)
    }
    #[inline]
    pub unsafe fn CreateCacheRequest(&self, cacheRequest: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateCacheRequest)(self.as_raw(), cacheRequest)
    }
    #[inline]
    pub unsafe fn CreateTrueCondition(&self, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateTrueCondition)(self.as_raw(), newCondition)
    }
    #[inline]
    pub unsafe fn CreateFalseCondition(&self, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateFalseCondition)(self.as_raw(), newCondition)
    }
    #[inline]
    pub unsafe fn CreatePropertyCondition(&self, propertyId: PROPERTYID, value: VARIANT, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreatePropertyCondition)(self.as_raw(), propertyId, value, newCondition)
    }
    #[inline]
    pub unsafe fn CreatePropertyConditionEx(&self, propertyId: PROPERTYID, value: VARIANT, flags: PropertyConditionFlags, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreatePropertyConditionEx)(self.as_raw(), propertyId, value, flags, newCondition)
    }
    #[inline]
    pub unsafe fn CreateAndCondition(&self, condition1: *mut c_void, condition2: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateAndCondition)(self.as_raw(), condition1, condition2, newCondition)
    }
    #[inline]
    pub unsafe fn CreateAndConditionFromArray(&self, conditions: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateAndConditionFromArray)(self.as_raw(), conditions, newCondition)
    }
    #[inline]
    pub unsafe fn CreateAndConditionFromNativeArray(&self, conditions: *mut *mut c_void, conditionCount: i32, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateAndConditionFromNativeArray)(self.as_raw(), conditions, conditionCount, newCondition)
    }
    #[inline]
    pub unsafe fn CreateOrCondition(&self, condition1: *mut c_void, condition2: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateOrCondition)(self.as_raw(), condition1, condition2, newCondition)
    }
    #[inline]
    pub unsafe fn CreateOrConditionFromArray(&self, conditions: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateOrConditionFromArray)(self.as_raw(), conditions, newCondition)
    }
    #[inline]
    pub unsafe fn CreateOrConditionFromNativeArray(&self, conditions: *mut *mut c_void, conditionCount: i32, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateOrConditionFromNativeArray)(self.as_raw(), conditions, conditionCount, newCondition)
    }
    #[inline]
    pub unsafe fn CreateNotCondition(&self, condition: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateNotCondition)(self.as_raw(), condition, newCondition)
    }
    #[inline]
    pub unsafe fn AddAutomationEventHandler(&self, eventId: EVENTID, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().AddAutomationEventHandler)(self.as_raw(), eventId, element, scope, cacheRequest, handler)
    }
    #[inline]
    pub unsafe fn RemoveAutomationEventHandler(&self, eventId: EVENTID, element: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().RemoveAutomationEventHandler)(self.as_raw(), eventId, element, handler)
    }
    #[inline]
    pub unsafe fn AddPropertyChangedEventHandlerNativeArray(&self, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void, propertyArray: *mut PROPERTYID, propertyCount: i32) -> HRESULT {
        (self.vtbl().AddPropertyChangedEventHandlerNativeArray)(self.as_raw(), element, scope, cacheRequest, handler, propertyArray, propertyCount)
    }
    #[inline]
    pub unsafe fn AddPropertyChangedEventHandler(&self, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void, propertyArray: *mut c_void) -> HRESULT {
        (self.vtbl().AddPropertyChangedEventHandler)(self.as_raw(), element, scope, cacheRequest, handler, propertyArray)
    }
    #[inline]
    pub unsafe fn RemovePropertyChangedEventHandler(&self, element: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().RemovePropertyChangedEventHandler)(self.as_raw(), element, handler)
    }
    #[inline]
    pub unsafe fn AddStructureChangedEventHandler(&self, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().AddStructureChangedEventHandler)(self.as_raw(), element, scope, cacheRequest, handler)
    }
    #[inline]
    pub unsafe fn RemoveStructureChangedEventHandler(&self, element: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().RemoveStructureChangedEventHandler)(self.as_raw(), element, handler)
    }
    #[inline]
    pub unsafe fn AddFocusChangedEventHandler(&self, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().AddFocusChangedEventHandler)(self.as_raw(), cacheRequest, handler)
    }
    #[inline]
    pub unsafe fn RemoveFocusChangedEventHandler(&self, handler: *mut c_void) -> HRESULT {
        (self.vtbl().RemoveFocusChangedEventHandler)(self.as_raw(), handler)
    }
    #[inline]
    pub unsafe fn RemoveAllEventHandlers(&self) -> HRESULT {
        (self.vtbl().RemoveAllEventHandlers)(self.as_raw())
    }
    #[inline]
    pub unsafe fn IntNativeArrayToSafeArray(&self, array: *mut i32, arrayCount: i32, safeArray: *mut *mut c_void) -> HRESULT {
        (self.vtbl().IntNativeArrayToSafeArray)(self.as_raw(), array, arrayCount, safeArray)
    }
    #[inline]
    pub unsafe fn IntSafeArrayToNativeArray(&self, intArray: *mut c_void, array: *mut *mut i32, arrayCount: *mut i32) -> HRESULT {
        (self.vtbl().IntSafeArrayToNativeArray)(self.as_raw(), intArray, array, arrayCount)
    }
    #[inline]
    pub unsafe fn RectToVariant(&self, rc: RECT, var: *mut VARIANT) -> HRESULT {
        (self.vtbl().RectToVariant)(self.as_raw(), rc, var)
    }
    #[inline]
    pub unsafe fn VariantToRect(&self, var: VARIANT, rc: *mut RECT) -> HRESULT {
        (self.vtbl().VariantToRect)(self.as_raw(), var, rc)
    }
    #[inline]
    pub unsafe fn SafeArrayToRectNativeArray(&self, rects: *mut c_void, rectArray: *mut *mut RECT, rectArrayCount: *mut i32) -> HRESULT {
        (self.vtbl().SafeArrayToRectNativeArray)(self.as_raw(), rects, rectArray, rectArrayCount)
    }
    #[inline]
    pub unsafe fn CreateProxyFactoryEntry(&self, factory: *mut c_void, factoryEntry: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateProxyFactoryEntry)(self.as_raw(), factory, factoryEntry)
    }
    #[inline]
    pub unsafe fn get_ProxyFactoryMapping(&self, factoryMapping: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ProxyFactoryMapping)(self.as_raw(), factoryMapping)
    }
    #[inline]
    pub unsafe fn GetPropertyProgrammaticName(&self, property: PROPERTYID, name: *mut BSTR) -> HRESULT {
        (self.vtbl().GetPropertyProgrammaticName)(self.as_raw(), property, name)
    }
    #[inline]
    pub unsafe fn GetPatternProgrammaticName(&self, pattern: PATTERNID, name: *mut BSTR) -> HRESULT {
        (self.vtbl().GetPatternProgrammaticName)(self.as_raw(), pattern, name)
    }
    #[inline]
    pub unsafe fn PollForPotentialSupportedPatterns(&self, pElement: *mut c_void, patternIds: *mut *mut c_void, patternNames: *mut *mut c_void) -> HRESULT {
        (self.vtbl().PollForPotentialSupportedPatterns)(self.as_raw(), pElement, patternIds, patternNames)
    }
    #[inline]
    pub unsafe fn PollForPotentialSupportedProperties(&self, pElement: *mut c_void, propertyIds: *mut *mut c_void, propertyNames: *mut *mut c_void) -> HRESULT {
        (self.vtbl().PollForPotentialSupportedProperties)(self.as_raw(), pElement, propertyIds, propertyNames)
    }
    #[inline]
    pub unsafe fn CheckNotSupported(&self, value: VARIANT, isNotSupported: *mut i32) -> HRESULT {
        (self.vtbl().CheckNotSupported)(self.as_raw(), value, isNotSupported)
    }
    #[inline]
    pub unsafe fn get_ReservedNotSupportedValue(&self, notSupportedValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ReservedNotSupportedValue)(self.as_raw(), notSupportedValue)
    }
    #[inline]
    pub unsafe fn get_ReservedMixedAttributeValue(&self, mixedAttributeValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_ReservedMixedAttributeValue)(self.as_raw(), mixedAttributeValue)
    }
    #[inline]
    pub unsafe fn ElementFromIAccessible(&self, accessible: *mut c_void, childId: i32, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ElementFromIAccessible)(self.as_raw(), accessible, childId, element)
    }
    #[inline]
    pub unsafe fn ElementFromIAccessibleBuildCache(&self, accessible: *mut c_void, childId: i32, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ElementFromIAccessibleBuildCache)(self.as_raw(), accessible, childId, cacheRequest, element)
    }
}

// ---- IUIAutomation2 : IUIAutomation
#[repr(C)]
pub struct IUIAutomation2Vtbl {
    pub base: IUIAutomationVtbl,
    pub get_AutoSetFocus: unsafe extern "system" fn(this: *mut c_void, autoSetFocus: *mut i32) -> HRESULT,
    pub put_AutoSetFocus: unsafe extern "system" fn(this: *mut c_void, autoSetFocus: i32) -> HRESULT,
    pub get_ConnectionTimeout: unsafe extern "system" fn(this: *mut c_void, timeout: *mut u32) -> HRESULT,
    pub put_ConnectionTimeout: unsafe extern "system" fn(this: *mut c_void, timeout: u32) -> HRESULT,
    pub get_TransactionTimeout: unsafe extern "system" fn(this: *mut c_void, timeout: *mut u32) -> HRESULT,
    pub put_TransactionTimeout: unsafe extern "system" fn(this: *mut c_void, timeout: u32) -> HRESULT,
}
pub enum IUIAutomation2 {}
unsafe impl Interface for IUIAutomation2 {
    const IID: GUID = GUID { data1: 0x34723aff, data2: 0x0c9d, data3: 0x49d0, data4: [0x98, 0x96, 0x7a, 0xb5, 0x2d, 0xf8, 0xcd, 0x8a] };
    type Vtbl = IUIAutomation2Vtbl;
}
impl ComPtr<IUIAutomation2> {
    #[inline]
    pub unsafe fn get_AutoSetFocus(&self, autoSetFocus: *mut i32) -> HRESULT {
        (self.vtbl().get_AutoSetFocus)(self.as_raw(), autoSetFocus)
    }
    #[inline]
    pub unsafe fn put_AutoSetFocus(&self, autoSetFocus: i32) -> HRESULT {
        (self.vtbl().put_AutoSetFocus)(self.as_raw(), autoSetFocus)
    }
    #[inline]
    pub unsafe fn get_ConnectionTimeout(&self, timeout: *mut u32) -> HRESULT {
        (self.vtbl().get_ConnectionTimeout)(self.as_raw(), timeout)
    }
    #[inline]
    pub unsafe fn put_ConnectionTimeout(&self, timeout: u32) -> HRESULT {
        (self.vtbl().put_ConnectionTimeout)(self.as_raw(), timeout)
    }
    #[inline]
    pub unsafe fn get_TransactionTimeout(&self, timeout: *mut u32) -> HRESULT {
        (self.vtbl().get_TransactionTimeout)(self.as_raw(), timeout)
    }
    #[inline]
    pub unsafe fn put_TransactionTimeout(&self, timeout: u32) -> HRESULT {
        (self.vtbl().put_TransactionTimeout)(self.as_raw(), timeout)
    }
    #[inline]
    pub unsafe fn CompareElements(&self, el1: *mut c_void, el2: *mut c_void, areSame: *mut i32) -> HRESULT {
        (self.vtbl().base.CompareElements)(self.as_raw(), el1, el2, areSame)
    }
    #[inline]
    pub unsafe fn CompareRuntimeIds(&self, runtimeId1: *mut c_void, runtimeId2: *mut c_void, areSame: *mut i32) -> HRESULT {
        (self.vtbl().base.CompareRuntimeIds)(self.as_raw(), runtimeId1, runtimeId2, areSame)
    }
    #[inline]
    pub unsafe fn GetRootElement(&self, root: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetRootElement)(self.as_raw(), root)
    }
    #[inline]
    pub unsafe fn ElementFromHandle(&self, hwnd: UIA_HWND, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.ElementFromHandle)(self.as_raw(), hwnd, element)
    }
    #[inline]
    pub unsafe fn ElementFromPoint(&self, pt: POINT, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.ElementFromPoint)(self.as_raw(), pt, element)
    }
    #[inline]
    pub unsafe fn GetFocusedElement(&self, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFocusedElement)(self.as_raw(), element)
    }
    #[inline]
    pub unsafe fn GetRootElementBuildCache(&self, cacheRequest: *mut c_void, root: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetRootElementBuildCache)(self.as_raw(), cacheRequest, root)
    }
    #[inline]
    pub unsafe fn ElementFromHandleBuildCache(&self, hwnd: UIA_HWND, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.ElementFromHandleBuildCache)(self.as_raw(), hwnd, cacheRequest, element)
    }
    #[inline]
    pub unsafe fn ElementFromPointBuildCache(&self, pt: POINT, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.ElementFromPointBuildCache)(self.as_raw(), pt, cacheRequest, element)
    }
    #[inline]
    pub unsafe fn GetFocusedElementBuildCache(&self, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFocusedElementBuildCache)(self.as_raw(), cacheRequest, element)
    }
    #[inline]
    pub unsafe fn CreateTreeWalker(&self, pCondition: *mut c_void, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateTreeWalker)(self.as_raw(), pCondition, walker)
    }
    #[inline]
    pub unsafe fn get_ControlViewWalker(&self, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ControlViewWalker)(self.as_raw(), walker)
    }
    #[inline]
    pub unsafe fn get_ContentViewWalker(&self, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ContentViewWalker)(self.as_raw(), walker)
    }
    #[inline]
    pub unsafe fn get_RawViewWalker(&self, walker: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_RawViewWalker)(self.as_raw(), walker)
    }
    #[inline]
    pub unsafe fn get_RawViewCondition(&self, condition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_RawViewCondition)(self.as_raw(), condition)
    }
    #[inline]
    pub unsafe fn get_ControlViewCondition(&self, condition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ControlViewCondition)(self.as_raw(), condition)
    }
    #[inline]
    pub unsafe fn get_ContentViewCondition(&self, condition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ContentViewCondition)(self.as_raw(), condition)
    }
    #[inline]
    pub unsafe fn CreateCacheRequest(&self, cacheRequest: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateCacheRequest)(self.as_raw(), cacheRequest)
    }
    #[inline]
    pub unsafe fn CreateTrueCondition(&self, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateTrueCondition)(self.as_raw(), newCondition)
    }
    #[inline]
    pub unsafe fn CreateFalseCondition(&self, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateFalseCondition)(self.as_raw(), newCondition)
    }
    #[inline]
    pub unsafe fn CreatePropertyCondition(&self, propertyId: PROPERTYID, value: VARIANT, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreatePropertyCondition)(self.as_raw(), propertyId, value, newCondition)
    }
    #[inline]
    pub unsafe fn CreatePropertyConditionEx(&self, propertyId: PROPERTYID, value: VARIANT, flags: PropertyConditionFlags, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreatePropertyConditionEx)(self.as_raw(), propertyId, value, flags, newCondition)
    }
    #[inline]
    pub unsafe fn CreateAndCondition(&self, condition1: *mut c_void, condition2: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateAndCondition)(self.as_raw(), condition1, condition2, newCondition)
    }
    #[inline]
    pub unsafe fn CreateAndConditionFromArray(&self, conditions: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateAndConditionFromArray)(self.as_raw(), conditions, newCondition)
    }
    #[inline]
    pub unsafe fn CreateAndConditionFromNativeArray(&self, conditions: *mut *mut c_void, conditionCount: i32, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateAndConditionFromNativeArray)(self.as_raw(), conditions, conditionCount, newCondition)
    }
    #[inline]
    pub unsafe fn CreateOrCondition(&self, condition1: *mut c_void, condition2: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateOrCondition)(self.as_raw(), condition1, condition2, newCondition)
    }
    #[inline]
    pub unsafe fn CreateOrConditionFromArray(&self, conditions: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateOrConditionFromArray)(self.as_raw(), conditions, newCondition)
    }
    #[inline]
    pub unsafe fn CreateOrConditionFromNativeArray(&self, conditions: *mut *mut c_void, conditionCount: i32, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateOrConditionFromNativeArray)(self.as_raw(), conditions, conditionCount, newCondition)
    }
    #[inline]
    pub unsafe fn CreateNotCondition(&self, condition: *mut c_void, newCondition: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateNotCondition)(self.as_raw(), condition, newCondition)
    }
    #[inline]
    pub unsafe fn AddAutomationEventHandler(&self, eventId: EVENTID, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.AddAutomationEventHandler)(self.as_raw(), eventId, element, scope, cacheRequest, handler)
    }
    #[inline]
    pub unsafe fn RemoveAutomationEventHandler(&self, eventId: EVENTID, element: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.RemoveAutomationEventHandler)(self.as_raw(), eventId, element, handler)
    }
    #[inline]
    pub unsafe fn AddPropertyChangedEventHandlerNativeArray(&self, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void, propertyArray: *mut PROPERTYID, propertyCount: i32) -> HRESULT {
        (self.vtbl().base.AddPropertyChangedEventHandlerNativeArray)(self.as_raw(), element, scope, cacheRequest, handler, propertyArray, propertyCount)
    }
    #[inline]
    pub unsafe fn AddPropertyChangedEventHandler(&self, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void, propertyArray: *mut c_void) -> HRESULT {
        (self.vtbl().base.AddPropertyChangedEventHandler)(self.as_raw(), element, scope, cacheRequest, handler, propertyArray)
    }
    #[inline]
    pub unsafe fn RemovePropertyChangedEventHandler(&self, element: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.RemovePropertyChangedEventHandler)(self.as_raw(), element, handler)
    }
    #[inline]
    pub unsafe fn AddStructureChangedEventHandler(&self, element: *mut c_void, scope: TreeScope, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.AddStructureChangedEventHandler)(self.as_raw(), element, scope, cacheRequest, handler)
    }
    #[inline]
    pub unsafe fn RemoveStructureChangedEventHandler(&self, element: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.RemoveStructureChangedEventHandler)(self.as_raw(), element, handler)
    }
    #[inline]
    pub unsafe fn AddFocusChangedEventHandler(&self, cacheRequest: *mut c_void, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.AddFocusChangedEventHandler)(self.as_raw(), cacheRequest, handler)
    }
    #[inline]
    pub unsafe fn RemoveFocusChangedEventHandler(&self, handler: *mut c_void) -> HRESULT {
        (self.vtbl().base.RemoveFocusChangedEventHandler)(self.as_raw(), handler)
    }
    #[inline]
    pub unsafe fn RemoveAllEventHandlers(&self) -> HRESULT {
        (self.vtbl().base.RemoveAllEventHandlers)(self.as_raw())
    }
    #[inline]
    pub unsafe fn IntNativeArrayToSafeArray(&self, array: *mut i32, arrayCount: i32, safeArray: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.IntNativeArrayToSafeArray)(self.as_raw(), array, arrayCount, safeArray)
    }
    #[inline]
    pub unsafe fn IntSafeArrayToNativeArray(&self, intArray: *mut c_void, array: *mut *mut i32, arrayCount: *mut i32) -> HRESULT {
        (self.vtbl().base.IntSafeArrayToNativeArray)(self.as_raw(), intArray, array, arrayCount)
    }
    #[inline]
    pub unsafe fn RectToVariant(&self, rc: RECT, var: *mut VARIANT) -> HRESULT {
        (self.vtbl().base.RectToVariant)(self.as_raw(), rc, var)
    }
    #[inline]
    pub unsafe fn VariantToRect(&self, var: VARIANT, rc: *mut RECT) -> HRESULT {
        (self.vtbl().base.VariantToRect)(self.as_raw(), var, rc)
    }
    #[inline]
    pub unsafe fn SafeArrayToRectNativeArray(&self, rects: *mut c_void, rectArray: *mut *mut RECT, rectArrayCount: *mut i32) -> HRESULT {
        (self.vtbl().base.SafeArrayToRectNativeArray)(self.as_raw(), rects, rectArray, rectArrayCount)
    }
    #[inline]
    pub unsafe fn CreateProxyFactoryEntry(&self, factory: *mut c_void, factoryEntry: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateProxyFactoryEntry)(self.as_raw(), factory, factoryEntry)
    }
    #[inline]
    pub unsafe fn get_ProxyFactoryMapping(&self, factoryMapping: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ProxyFactoryMapping)(self.as_raw(), factoryMapping)
    }
    #[inline]
    pub unsafe fn GetPropertyProgrammaticName(&self, property: PROPERTYID, name: *mut BSTR) -> HRESULT {
        (self.vtbl().base.GetPropertyProgrammaticName)(self.as_raw(), property, name)
    }
    #[inline]
    pub unsafe fn GetPatternProgrammaticName(&self, pattern: PATTERNID, name: *mut BSTR) -> HRESULT {
        (self.vtbl().base.GetPatternProgrammaticName)(self.as_raw(), pattern, name)
    }
    #[inline]
    pub unsafe fn PollForPotentialSupportedPatterns(&self, pElement: *mut c_void, patternIds: *mut *mut c_void, patternNames: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.PollForPotentialSupportedPatterns)(self.as_raw(), pElement, patternIds, patternNames)
    }
    #[inline]
    pub unsafe fn PollForPotentialSupportedProperties(&self, pElement: *mut c_void, propertyIds: *mut *mut c_void, propertyNames: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.PollForPotentialSupportedProperties)(self.as_raw(), pElement, propertyIds, propertyNames)
    }
    #[inline]
    pub unsafe fn CheckNotSupported(&self, value: VARIANT, isNotSupported: *mut i32) -> HRESULT {
        (self.vtbl().base.CheckNotSupported)(self.as_raw(), value, isNotSupported)
    }
    #[inline]
    pub unsafe fn get_ReservedNotSupportedValue(&self, notSupportedValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ReservedNotSupportedValue)(self.as_raw(), notSupportedValue)
    }
    #[inline]
    pub unsafe fn get_ReservedMixedAttributeValue(&self, mixedAttributeValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.get_ReservedMixedAttributeValue)(self.as_raw(), mixedAttributeValue)
    }
    #[inline]
    pub unsafe fn ElementFromIAccessible(&self, accessible: *mut c_void, childId: i32, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.ElementFromIAccessible)(self.as_raw(), accessible, childId, element)
    }
    #[inline]
    pub unsafe fn ElementFromIAccessibleBuildCache(&self, accessible: *mut c_void, childId: i32, cacheRequest: *mut c_void, element: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.ElementFromIAccessibleBuildCache)(self.as_raw(), accessible, childId, cacheRequest, element)
    }
}
