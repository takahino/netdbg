//! 拡張（VSCodedotnetX86Debugger）の独自の要求: オブジェクト ID、スレッドの凍結、ヒープとハンドルの統計（段階 5）。
//! 要求の形と応答は、拡張が netcoredbg に当てたパッチ（0008 / 0010 / 0011 / 0012）と同じ。
//! - `netdbg.makeObjectId`（`variablesReference`）: その値のオブジェクトに ID を付け、`{ id }` を返す。ID は弱い GC
//!   ハンドルで持ち（Visual Studio と同じく、オブジェクトを生かし続けない。回収されたら null）、続行しても残る。式の中では
//!   `__NETDBG_OBJID_<N>` と書く（拡張が `$N` から書き換えて送ってくる）。変数の欄の値には ` {$N}` を足す。
//! - `netdbg.freezeThread`（`threadId`、`frozen`）: スレッドを止めたままにする（続行しても、評価のあとも動かさない）。
//! - `netdbg.heapStats`: GC ヒープの、型ごとの数と大きさ（`{ types: [{ name, count, size }] }`、大きい順）。
//! - `netdbg.handleStats`: ネイティブのハンドルを持つ型ごとの数と、Dispose されていない数（`handle_statistics`）。

use super::*;
use std::collections::HashMap;

/// 式の中で、オブジェクト ID を指す名前の頭（`__NETDBG_OBJID_1`）。
pub(super) const OBJECT_ID_PREFIX: &str = "__NETDBG_OBJID_";

/// オブジェクト ID。弱い GC ハンドルと、その番号。
pub(super) struct ObjectId {
    pub id: i64,
    pub handle: ComPtr<ICorDebugHandleValue>,
}

/// 値が指すオブジェクトの番地（null と値型は 0）。
pub(super) unsafe fn object_address(value: &ComPtr<ICorDebugValue>) -> u64 {
    if let Ok(reference) = value.cast::<ICorDebugReferenceValue>() {
        let mut null = 0i32;
        let mut address = 0u64;
        if !reference.IsNull(&mut null).is_ok() || null != 0 || !reference.GetValue(&mut address).is_ok() {
            return 0;
        }
        return address;
    }
    if value.cast::<ICorDebugHeapValue>().is_ok() {
        let mut address = 0u64;
        if value.GetAddress(&mut address).is_ok() {
            return address;
        }
    }
    0
}

/// `netdbg.handleStats` で、型の番号ごとに数えるもの。
struct HandleCount {
    /// ハンドルのフィールド（オブジェクトの中の位置と `型.名前`）。無ければ、その型は数えない。
    field: Option<(u32, String)>,
    /// オブジェクトの数。
    count: u64,
    /// そのうち、ハンドルが 0 でない（Dispose されていない）数。
    live: u64,
}

impl Session {
    /// `netdbg.makeObjectId`
    pub fn make_object_id(&mut self, arguments: &Value) -> Result<Value, String> {
        let reference = arguments.get("variablesReference").and_then(Value::as_u64).ok_or_else(|| tr!("netdbg.makeObjectId has no variablesReference."))? as usize;
        let handle = reference.checked_sub(1).and_then(|index| self.handles.get(index)).cloned().ok_or_else(|| tr!("That variablesReference is not from the current stop."))?;
        let (Handle::Children { value, .. } | Handle::Raw { value, .. }) = handle else { return Err(tr!("An object ID can be made only for an object value.")) };
        let address = unsafe { object_address(&value) };
        if address == 0 {
            return Err(tr!("An object ID cannot be made for null or a value type."));
        }
        // すでに ID があれば、それを返す。
        if let Some(existing) = self.object_ids.iter().find(|entry| entry.handle.cast::<ICorDebugValue>().is_ok_and(|handle| unsafe { object_address(&handle) } == address)) {
            return Ok(json!({ "id": existing.id }));
        }
        let object = unsafe {
            match value.cast::<ICorDebugReferenceValue>() {
                Ok(reference) => take::<ICorDebugValue>("ICorDebugReferenceValue::Dereference", |out| reference.Dereference(out))?,
                Err(_) => value.clone(),
            }
        };
        let heap: ComPtr<ICorDebugHeapValue2> = object.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugHeapValue2"), hr))?;
        let handle = unsafe { take::<ICorDebugHandleValue>("ICorDebugHeapValue2::CreateHandle", |out| heap.CreateHandle(HANDLE_WEAK_TRACK_RESURRECTION, out))? };
        self.last_object_id += 1;
        let id = self.last_object_id;
        self.object_ids.push(ObjectId { id, handle });
        Ok(json!({ "id": id }))
    }

    /// 式の中の `__NETDBG_OBJID_<N>` の値。その名前でなければ `None`。
    pub(super) fn object_id_value(&self, name: &str) -> Option<Result<ComPtr<ICorDebugValue>, String>> {
        let number = name.strip_prefix(OBJECT_ID_PREFIX)?;
        let id: i64 = match number.parse() {
            Ok(id) => id,
            Err(_) => return Some(Err(tr!("The object ID ${number} is not a valid number.", number = number))),
        };
        Some(match self.object_ids.iter().find(|entry| entry.id == id) {
            Some(entry) => entry.handle.cast::<ICorDebugValue>().map_err(|hr| failed(tr_str!("Getting the object ID's value"), hr)),
            None => Err(tr!("There is no object ID ${id}.", id = id)),
        })
    }

    /// 値が ID の付いたオブジェクトを指していれば、` {$N}`（変数の欄の値のあとに付ける）。
    pub(super) fn object_id_suffix(&self, value: &ComPtr<ICorDebugValue>) -> String {
        if self.object_ids.is_empty() {
            return String::new();
        }
        let address = unsafe { object_address(value) };
        if address == 0 {
            return String::new();
        }
        self.object_ids
            .iter()
            .find(|entry| entry.handle.cast::<ICorDebugValue>().is_ok_and(|handle| unsafe { object_address(&handle) } == address))
            .map_or_else(String::new, |entry| format!(" {{${}}}", entry.id))
    }

    /// `netdbg.freezeThread`
    pub fn freeze_thread(&mut self, arguments: &Value) -> Result<Value, String> {
        let thread_id = arguments.get("threadId").and_then(Value::as_u64).ok_or_else(|| tr!("netdbg.freezeThread has no threadId."))? as u32;
        let frozen = arguments.get("frozen").and_then(Value::as_bool).unwrap_or(true);
        let process = self.process.clone().ok_or_else(|| tr!("There is no target process."))?;
        unsafe {
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread_id, out))?;
            thread.SetDebugState(if frozen { THREAD_SUSPEND } else { THREAD_RUN }).ok().map_err(|hr| failed("ICorDebugThread::SetDebugState", hr))?;
        }
        if frozen {
            self.frozen_threads.insert(thread_id);
        } else {
            self.frozen_threads.remove(&thread_id);
        }
        Ok(Value::Null)
    }

    /// 止まっている対象の GC ヒープのオブジェクトを、一つずつ `visit` に渡す。
    fn walk_heap(&self, mut visit: impl FnMut(&COR_HEAPOBJECT)) -> Result<ComPtr<ICorDebugProcess5>, String> {
        if self.state != State::Stopped {
            return Err(tr!("The heap can be counted only while the target is stopped."));
        }
        let process = self.process.clone().ok_or_else(|| tr!("There is no target process."))?;
        let process5: ComPtr<ICorDebugProcess5> = process.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugProcess5"), hr))?;
        let mut information: COR_HEAPINFO = unsafe { std::mem::zeroed() };
        unsafe { process5.GetGCHeapInformation(&mut information).ok().map_err(|hr| failed("ICorDebugProcess5::GetGCHeapInformation", hr))? };
        // ヒープをたどれるのは、ランタイムがヒープを整った状態にしているときだけ（GC の途中などは、たどれない）。
        if information.areGCStructuresValid == 0 {
            return Err(tr!("The heap cannot be walked now (a GC may be in progress). Run a little and try again."));
        }
        unsafe {
            let heap = take::<ICorDebugHeapEnum>("ICorDebugProcess5::EnumerateHeap", |out| process5.EnumerateHeap(out))?;
            let mut objects: Vec<COR_HEAPOBJECT> = vec![std::mem::zeroed(); 512];
            loop {
                let mut fetched = 0u32;
                let result = heap.Next(objects.len() as u32, objects.as_mut_ptr(), &mut fetched);
                if !result.is_ok() || fetched == 0 {
                    break;
                }
                objects[..fetched as usize].iter().for_each(&mut visit);
            }
        }
        Ok(process5)
    }

    /// 型の番号の、表示用の名前（引けなければ「?」）。
    pub(super) fn heap_type_name(&self, process5: &ComPtr<ICorDebugProcess5>, id: (u64, u64)) -> String {
        let ty = unsafe { take::<ICorDebugType>("ICorDebugProcess5::GetTypeForTypeID", |out| process5.GetTypeForTypeID(COR_TYPEID { token1: id.0, token2: id.1 }, out)) };
        ty.ok().map(|ty| unsafe { value::type_name(&ty, &self.modules) }).filter(|name| !name.is_empty()).unwrap_or_else(|| "?".into())
    }

    /// 型の番号が COM オブジェクト（RCW）の型か: `System.__ComObject` か、COM から取り込んだ型（TypeDef の Import の印）。
    fn is_com_object_type(&self, process5: &ComPtr<ICorDebugProcess5>, id: (u64, u64)) -> bool {
        const TYPE_DEF: usize = 0x02;
        const TD_IMPORT: u32 = 0x1000;
        unsafe {
            let Ok(ty) = take::<ICorDebugType>("ICorDebugProcess5::GetTypeForTypeID", |out| process5.GetTypeForTypeID(COR_TYPEID { token1: id.0, token2: id.1 }, out)) else { return false };
            if value::type_name(&ty, &self.modules) == "System.__ComObject" {
                return true;
            }
            let Ok(class) = take::<ICorDebugClass>("ICorDebugType::GetClass", |out| ty.GetClass(out)) else { return false };
            let mut token = 0u32;
            class.GetToken(&mut token);
            let Ok(module) = take::<ICorDebugModule>("ICorDebugClass::GetModule", |out| class.GetModule(out)) else { return false };
            let path = module_path(&module);
            let Some(metadata) = self.modules.iter().find(|m| m.path == path).and_then(|m| m.metadata()) else { return false };
            token >> 24 == TYPE_DEF as u32 && metadata.cell(TYPE_DEF, token & 0x00FF_FFFF, 0) & TD_IMPORT != 0
        }
    }
    /// `netdbg.heapStats`
    pub fn heap_statistics(&mut self) -> Result<Value, String> {
        // 型の番号ごとに数と大きさを数え、名前は、番号ごとに一度だけ引く。
        let mut by_id: HashMap<(u64, u64), (u64, u64)> = HashMap::new();
        let process5 = self.walk_heap(|object| {
            let entry = by_id.entry((object.type_.token1, object.type_.token2)).or_default();
            entry.0 += 1;
            entry.1 += object.size;
        })?;
        // 型引数の違う型が同じ名前になることがあるので、名前ごとに足す。型の無いもの（オブジェクトの間の空き）は「?」。
        let mut by_name: HashMap<String, (u64, u64)> = HashMap::new();
        for (&id, &(count, size)) in &by_id {
            let total = by_name.entry(self.heap_type_name(&process5, id)).or_default();
            total.0 += count;
            total.1 += size;
        }
        let mut types: Vec<(String, u64, u64)> = by_name.into_iter().map(|(name, (count, size))| (name, count, size)).collect();
        types.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        // COM オブジェクト（RCW）: System.__ComObject と、COM から取り込んだ型（Import の印）のオブジェクト。前の統計からの
        // 増分も付ける（解放されない COM オブジェクトの漏れ。D-092）。
        let com_objects: u64 = by_id.iter().filter(|(id, _)| self.is_com_object_type(&process5, **id)).map(|(_, (count, _))| count).sum();
        let previous = self.last_com_objects.replace(com_objects);
        let mut com = json!({ "count": com_objects });
        if let Some(previous) = previous {
            com["previous"] = json!(previous);
            com["delta"] = json!(com_objects as i64 - previous as i64);
        }
        Ok(json!({ "types": types.into_iter().map(|(name, count, size)| json!({ "name": name, "count": count, "size": size })).collect::<Vec<_>>(), "comObjects": com }))
    }

    /// `netdbg.handleStats`（`fields: [{ type, field }]`）: 指定したフィールド（宣言した型の `Namespace.Name` と
    /// フィールド名）にネイティブのハンドルを持つ型ごとに、オブジェクトの数と、ハンドルが 0 でない（Dispose されていない）
    /// 数を返す（`{ types: [{ name, count, live, field }] }`、名前の順）。拡張のパッチ 0012 と同じ。
    pub fn handle_statistics(&mut self, arguments: &Value) -> Result<Value, String> {
        let wanted: Vec<(String, String)> = arguments
            .get("fields")
            .and_then(Value::as_array)
            .ok_or_else(|| tr!("netdbg.handleStats has no fields."))?
            .iter()
            .filter_map(|field| Some((field.get("type")?.as_str()?.to_string(), field.get("field")?.as_str()?.to_string())))
            .collect();
        let mut by_id: HashMap<(u64, u64), HandleCount> = HashMap::new();
        let mut objects: Vec<(u64, (u64, u64))> = Vec::new();
        let process5 = self.walk_heap(|object| {
            // 型の無いもの（オブジェクトの間の空き）は数えない。
            if object.type_.token1 != 0 || object.type_.token2 != 0 {
                objects.push((object.address, (object.type_.token1, object.type_.token2)));
            }
        })?;
        let process = self.process.clone().ok_or_else(|| tr!("There is no target process."))?;
        for (address, id) in objects {
            let entry = match by_id.entry(id) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => entry.insert(HandleCount { field: self.handle_field(&process5, id, &wanted), count: 0, live: 0 }),
            };
            let Some((offset, _)) = &entry.field else { continue };
            entry.count += 1;
            // ハンドルはポインターの大きさ。エンジンと対象のビット数は同じ。Dispose すると 0 になる。
            let mut handle = [0u8; std::mem::size_of::<usize>()];
            let mut read = 0usize;
            let ok = unsafe { process.ReadMemory(address + u64::from(*offset), handle.len() as u32, handle.as_mut_ptr(), &mut read).is_ok() };
            if ok && read == handle.len() && usize::from_ne_bytes(handle) != 0 {
                entry.live += 1;
            }
        }
        // 型引数の違う型が同じ名前になることがあるので、名前ごとに足す。
        let mut by_name: std::collections::BTreeMap<String, (u64, u64, String)> = std::collections::BTreeMap::new();
        for (id, HandleCount { field, count, live }) in by_id {
            let Some((_, field)) = field else { continue };
            let total = by_name.entry(self.heap_type_name(&process5, id)).or_default();
            total.0 += count;
            total.1 += live;
            total.2 = field;
        }
        Ok(json!({ "types": by_name.into_iter().map(|(name, (count, live, field))| json!({ "name": name, "count": count, "live": live, "field": field })).collect::<Vec<_>>() }))
    }

    /// 型（とその基底の型）のインスタンス フィールドのうち、`wanted` に挙がったものを探し、オブジェクトの中の位置と
    /// "宣言した型.名前" を返す。フィールドは基底の型が宣言していることがある（`Bitmap` は `Image._nativeImage`）。
    fn handle_field(&self, process5: &ComPtr<ICorDebugProcess5>, id: (u64, u64), wanted: &[(String, String)]) -> Option<(u32, String)> {
        let mut current = COR_TYPEID { token1: id.0, token2: id.1 };
        // 基底の型をたどる。壊れた情報で輪になっても止まるよう、深さを限る。
        for _ in 0..64 {
            if current.token1 == 0 && current.token2 == 0 {
                return None;
            }
            let mut layout: COR_TYPE_LAYOUT = unsafe { std::mem::zeroed() };
            if !unsafe { process5.GetTypeLayout(current, &mut layout) }.is_ok() {
                return None;
            }
            if let Some(found) = self.handle_field_of(process5, current, layout.numFields, wanted) {
                return Some(found);
            }
            current = layout.parentID;
        }
        None
    }

    /// `handle_field` の、型 1 つ分（その型が宣言したフィールドだけを見る）。
    fn handle_field_of(&self, process5: &ComPtr<ICorDebugProcess5>, id: COR_TYPEID, count: u32, wanted: &[(String, String)]) -> Option<(u32, String)> {
        if count == 0 {
            return None;
        }
        let mut fields: Vec<COR_FIELD> = vec![unsafe { std::mem::zeroed() }; count as usize];
        let mut fetched = 0u32;
        if !unsafe { process5.GetTypeFields(id, count, fields.as_mut_ptr(), &mut fetched) }.is_ok() {
            return None;
        }
        let ty = unsafe { take::<ICorDebugType>("ICorDebugProcess5::GetTypeForTypeID", |out| process5.GetTypeForTypeID(id, out)) }.ok()?;
        let class = unsafe { take::<ICorDebugClass>("ICorDebugType::GetClass", |out| ty.GetClass(out)) }.ok()?;
        let module = unsafe { take::<ICorDebugModule>("ICorDebugClass::GetModule", |out| class.GetModule(out)) }.ok()?;
        let metadata = value::Modules::metadata(&self.modules, &module)?;
        fields.iter().take(fetched as usize).find_map(|field| {
            let (owner, declared) = metadata.field_declaration(field.token & 0x00FF_FFFF)?;
            if declared.is_static {
                return None;
            }
            let owner = metadata.raw_type_name(owner);
            wanted.iter().any(|(ty, name)| *ty == owner && name == declared.name).then(|| (field.offset, format!("{owner}.{}", declared.name)))
        })
    }
}
