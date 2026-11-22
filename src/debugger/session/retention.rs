//! オブジェクトを保持している経路（段階 12）: 指定したオブジェクトが、どの GC のルートから、どの参照をたどって生きて
//! いるか。メモリーリークの「なぜ回収されないか」に答える。全体のプロファイラにはしない。
//!
//! たどり方: GC のルート（`EnumerateGCReferences`。スタック、強いハンドル、ファイナライザーの列。弱い参照は除く）から、
//! 参照を幅優先でたどり、指定したオブジェクトに着いたら、通った道を返す（いちばん短い経路）。深さと、たどるオブジェクトの
//! 数に上限を付ける。オブジェクトの中の参照は、型ごとに一度だけ求めて覚える（`GetTypeLayout` / `GetTypeFields`。基底の
//! 型と、値型の欄の中の参照も含める。配列は要素）。中身は対象のメモリーから読む。止まっているときだけ。

use super::inspect::object_address;
use super::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

/// 経路の長さ（参照の数）の既定の上限と、上限の上限。
const DEFAULT_DEPTH: usize = 20;
const MAX_DEPTH: usize = 100;
/// たどるオブジェクトの数の上限（32bit のエンジンで、覚える表が大きくなりすぎないように）。
const MAX_VISITED: usize = 2_000_000;
/// 配列の要素を読む数の上限（巨大な配列の先頭だけを見る）。
const MAX_ELEMENTS: usize = 1 << 20;
/// ルートがスタックのとき、ローカル変数の名前を探すフレームの数（スレッドごと）。
const ROOT_FRAMES: usize = 30;

// CorElementType のうち、参照と値型。
const ELEMENT_TYPE_STRING: u32 = 0x0E;
const ELEMENT_TYPE_VALUETYPE: u32 = 0x11;
const ELEMENT_TYPE_CLASS: u32 = 0x12;
const ELEMENT_TYPE_VAR: u32 = 0x13;
const ELEMENT_TYPE_ARRAY: u32 = 0x14;
const ELEMENT_TYPE_GENERICINST: u32 = 0x15;
const ELEMENT_TYPE_OBJECT: u32 = 0x1C;
const ELEMENT_TYPE_SZARRAY: u32 = 0x1D;
const ELEMENT_TYPE_MVAR: u32 = 0x1E;

/// オブジェクトの中の、参照を持つ欄。
struct Slot {
    offset: u32,
    /// フィールドの名前（値型の中なら `outer.inner`）。
    name: Rc<str>,
}

/// 型ごとの、参照のありか。
enum Layout {
    NoReferences,
    Object(Vec<Slot>),
    /// 配列: 要素の頭、要素の大きさ、要素の数の位置。要素が参照なら `slots` は空で `references` が真。
    Array { first: u32, element_size: u32, count_offset: u32, references: bool, slots: Vec<Slot> },
}

/// 親から子への参照のしかた。
#[derive(Clone)]
enum Via {
    Field(Rc<str>),
    Element(usize, Option<Rc<str>>),
}

impl Via {
    fn to_json(&self) -> Value {
        match self {
            Via::Field(name) => json!(format!(".{name}")),
            Via::Element(index, None) => json!(format!("[{index}]")),
            Via::Element(index, Some(name)) => json!(format!("[{index}].{name}")),
        }
    }
}

/// たどった表の 1 つ: 親（ルートなら `None` とルートの番号）と、親からの参照。
enum Parent {
    Root(usize),
    Object(u64, Via),
}

#[derive(Clone)]
struct Root {
    address: u64,
    kind: u32,
}

/// ヒープをたどる道具（型ごとの参照のありかを覚える）。
struct Walker<'a> {
    session: &'a Session,
    process: ComPtr<ICorDebugProcess>,
    process5: ComPtr<ICorDebugProcess5>,
    layouts: HashMap<(u64, u64), Rc<Layout>>,
}

impl Session {
    /// `netdbg.retentionPath { variablesReference | expression + frameId, maxDepth? }`: オブジェクトを保持している
    /// 経路（ルートから順に、型、番地、親からの参照）。見つからなければ `found: false` と理由。
    pub fn retention_path(&mut self, arguments: &Value) -> Result<Value, String> {
        if self.state != State::Stopped {
            return Err(tr!("The retention path can be searched only while the target is stopped."));
        }
        let target = self.retention_target(arguments)?;
        let max_depth = arguments.get("maxDepth").and_then(Value::as_u64).map_or(DEFAULT_DEPTH, |d| (d as usize).clamp(1, MAX_DEPTH));
        let started = std::time::Instant::now();
        let process = self.process.clone().ok_or_else(|| tr!("There is no target process."))?;
        let process5: ComPtr<ICorDebugProcess5> = process.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugProcess5"), hr))?;
        let mut information: COR_HEAPINFO = unsafe { std::mem::zeroed() };
        unsafe { process5.GetGCHeapInformation(&mut information).ok().map_err(|hr| failed("ICorDebugProcess5::GetGCHeapInformation", hr))? };
        if information.areGCStructuresValid == 0 {
            return Err(tr!("The heap cannot be walked now (a GC may be in progress). Run a little and try again."));
        }
        // スタックのルートのうち、名前のあるローカル変数・引数のものと、名前の無い一時の置き場所（Debug の JIT が式の途中の
        // 値を残すなど）を分ける。一時の置き場所は、まず除いて探す（ほかに本当の持ち主がいれば、そちらが知りたいこと）。
        let all_roots = unsafe { gc_roots(&process5)? };
        let named = self.named_local_objects();
        let is_temporary = |root: &Root| root.kind == CorReferenceStack && !named.contains(&root.address);
        let temporary_holds_target = all_roots.iter().any(|r| r.address == target && is_temporary(r));
        let lasting: Vec<Root> = all_roots.iter().filter(|r| !is_temporary(r)).cloned().collect();
        let mut walker = Walker { session: self, process, process5: process5.clone(), layouts: HashMap::new() };
        let (mut found, mut visited) = walker.search(&lasting, target, max_depth);
        let mut roots = lasting;
        if found.is_none() {
            (found, visited) = walker.search(&all_roots, target, max_depth);
            roots = all_roots;
        }
        crate::log!("retention path: {} roots, {} objects visited in {:.1} ms", roots.len(), visited.len(), started.elapsed().as_secs_f64() * 1000.0);
        let target_type = self.heap_object_type(&process5, target);
        let Some(root_index) = found else {
            let reason = if visited.len() >= MAX_VISITED {
                format!("No path found before the limit of {MAX_VISITED} objects.")
            } else {
                format!("No path of {max_depth} references or fewer from a GC root (the object may be kept only by weak references, or be garbage the next GC collects).")
            };
            return Ok(json!({ "found": false, "target": { "address": hex(target), "type": target_type }, "visited": visited.len(), "reason": reason }));
        };
        // 着いたオブジェクトから、親をたどってルートまで戻る。各段は「オブジェクトと、次の段への参照（対象は無い）」。
        let mut chain: Vec<(u64, Option<Via>)> = vec![(target, None)];
        let mut at = target;
        while let Some(Parent::Object(parent, how)) = visited.get(&at) {
            chain.push((*parent, Some(how.clone())));
            at = *parent;
        }
        chain.reverse();
        let path: Vec<Value> = chain
            .iter()
            .map(|(address, next)| {
                let mut step = json!({ "address": hex(*address), "type": self.heap_object_type(&process5, *address) });
                if let Some(next) = next {
                    step["next"] = next.to_json();
                }
                step
            })
            .collect();
        let root = &roots[root_index];
        let mut root_json = json!({ "kind": root_kind(root.kind) });
        if root.kind == CorReferenceStack {
            if let Some((name, function, thread)) = self.local_holding(root.address) {
                root_json["local"] = json!({ "name": name, "function": function, "threadId": thread });
            }
        }
        let mut answer = json!({ "found": true, "target": { "address": hex(target), "type": target_type }, "root": root_json, "path": path, "length": path.len() - 1, "visited": visited.len() });
        if temporary_holds_target && !(root.kind == CorReferenceStack && path.len() == 1) {
            answer["alsoHeldBy"] = json!(["a temporary on the stack (a hidden slot of a running method; gone when the method moves on)"]);
        }
        Ok(answer)
    }

    /// 調べるオブジェクトの番地（`variablesReference` か、`frameId` の上で評価する `expression`）。
    fn retention_target(&mut self, arguments: &Value) -> Result<u64, String> {
        let value = if let Some(reference) = arguments.get("variablesReference").and_then(Value::as_u64) {
            let handle = (reference as usize).checked_sub(1).and_then(|index| self.handles.get(index)).cloned().ok_or_else(|| tr!("That variablesReference is not from the current stop."))?;
            let (Handle::Children { value, .. } | Handle::Raw { value, .. }) = handle else { return Err(tr!("A path can be searched only for an object value.")) };
            value
        } else {
            let expression = arguments.get("expression").and_then(Value::as_str).ok_or_else(|| tr!("netdbg.retentionPath has no variablesReference or expression."))?;
            let (thread, depth) = self.frame_of(arguments)?;
            match self.evaluate_expression(thread, depth, expression)? {
                eval::Evaluated::Value(value) | eval::Evaluated::Nullable(value) => value,
                _ => return Err(tr!("The value of the expression is not an object.")),
            }
        };
        let address = unsafe { object_address(&value) };
        if address == 0 {
            return Err(tr!("null and value-type values have no retention path."));
        }
        Ok(address)
    }

    fn heap_object_type(&self, process5: &ComPtr<ICorDebugProcess5>, address: u64) -> String {
        let mut id = COR_TYPEID { token1: 0, token2: 0 };
        if unsafe { process5.GetTypeID(address, &mut id) }.is_ok() {
            self.heap_type_name(process5, (id.token1, id.token2))
        } else {
            "?".into()
        }
    }

    /// 名前のあるローカル変数と引数が持っているオブジェクト（スレッドごとに上のフレームだけ）。
    fn named_local_objects(&self) -> HashSet<u64> {
        let mut held = HashSet::new();
        for &thread in &self.threads {
            let Ok(frames) = self.walk_frames(thread) else { continue };
            for depth in 0..frames.len().min(ROOT_FRAMES) {
                for (_, value) in self.frame_variables(thread, depth).unwrap_or_default() {
                    held.insert(unsafe { object_address(&value) });
                }
            }
        }
        held.remove(&0);
        held
    }

    /// スタックのルートのオブジェクトを持っているローカル変数（名前、フレームのメソッド、スレッド）。
    fn local_holding(&self, address: u64) -> Option<(String, String, u32)> {
        for &thread in &self.threads {
            let Ok(frames) = self.walk_frames(thread) else { continue };
            for (depth, frame) in frames.iter().enumerate().take(ROOT_FRAMES) {
                let function = unsafe { self.describe_frame(0, frame) }["name"].as_str().unwrap_or("").to_string();
                for (name, value) in self.frame_variables(thread, depth).unwrap_or_default() {
                    if unsafe { object_address(&value) } == address {
                        return Some((name, function, thread));
                    }
                }
            }
        }
        None
    }
}

impl Walker<'_> {
    /// ルートから幅優先でたどる。着いたら、そのルートの番号と、たどった表。
    fn search(&mut self, roots: &[Root], target: u64, max_depth: usize) -> (Option<usize>, HashMap<u64, Parent>) {
        let mut visited: HashMap<u64, Parent> = HashMap::new();
        let mut queue: VecDeque<(u64, usize)> = VecDeque::new();
        for (index, root) in roots.iter().enumerate() {
            if root.address == 0 || visited.contains_key(&root.address) {
                continue;
            }
            // 対象を直接指す強いハンドルは、対象を求めた評価の結果をデバッガーが持っているもの（続行するまで生きている）。
            // 利用者の `GCHandle.Alloc` も同じ形なので見分けられないが、評価の結果の方がずっと多いので、除いてほかの経路を探す。
            if root.address == target && root.kind == CorHandleStrong {
                continue;
            }
            visited.insert(root.address, Parent::Root(index));
            if root.address == target {
                return (Some(index), visited);
            }
            queue.push_back((root.address, 0));
        }
        while let Some((address, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for (child, via) in self.references(address) {
                if child == 0 || visited.contains_key(&child) {
                    continue;
                }
                visited.insert(child, Parent::Object(address, via));
                if child == target {
                    return (Some(root_of(&visited, child)), visited);
                }
                if visited.len() >= MAX_VISITED {
                    return (None, visited);
                }
                queue.push_back((child, depth + 1));
            }
        }
        (None, visited)
    }

    /// オブジェクトが参照しているオブジェクトと、その参照のしかた。
    fn references(&mut self, address: u64) -> Vec<(u64, Via)> {
        let mut id = COR_TYPEID { token1: 0, token2: 0 };
        if !unsafe { self.process5.GetTypeID(address, &mut id) }.is_ok() {
            return Vec::new();
        }
        let layout = self.layout(id);
        match &*layout {
            Layout::NoReferences => Vec::new(),
            Layout::Object(slots) => {
                let end = slots.iter().map(|s| s.offset as usize + POINTER).max().unwrap_or(0);
                let Some(bytes) = self.read(address, end) else { return Vec::new() };
                slots.iter().map(|s| (pointer_at(&bytes, s.offset as usize), Via::Field(s.name.clone()))).collect()
            }
            Layout::Array { first, element_size, count_offset, references, slots } => {
                let Some(count) = self.read(address + u64::from(*count_offset), 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize) else { return Vec::new() };
                let count = count.min(MAX_ELEMENTS);
                let size = *element_size as usize;
                let Some(bytes) = self.read(address + u64::from(*first), count * size) else { return Vec::new() };
                let mut out = Vec::new();
                for index in 0..count {
                    if *references {
                        out.push((pointer_at(&bytes, index * size), Via::Element(index, None)));
                    } else {
                        for slot in slots {
                            out.push((pointer_at(&bytes, index * size + slot.offset as usize), Via::Element(index, Some(slot.name.clone()))));
                        }
                    }
                }
                out
            }
        }
    }

    fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
        if length == 0 {
            return Some(Vec::new());
        }
        let mut buffer = vec![0u8; length];
        let mut got = 0usize;
        (unsafe { self.process.ReadMemory(address, length as u32, buffer.as_mut_ptr(), &mut got) }.is_ok() && got == length).then_some(buffer)
    }

    /// 型の参照のありか（覚えておく）。
    fn layout(&mut self, id: COR_TYPEID) -> Rc<Layout> {
        if let Some(layout) = self.layouts.get(&(id.token1, id.token2)) {
            return layout.clone();
        }
        let layout = Rc::new(self.build_layout(id));
        self.layouts.insert((id.token1, id.token2), layout.clone());
        layout
    }

    fn build_layout(&self, id: COR_TYPEID) -> Layout {
        let mut layout: COR_TYPE_LAYOUT = unsafe { std::mem::zeroed() };
        if !unsafe { self.process5.GetTypeLayout(id, &mut layout) }.is_ok() {
            return Layout::NoReferences;
        }
        if matches!(layout.type_, ELEMENT_TYPE_SZARRAY | ELEMENT_TYPE_ARRAY) {
            let mut array: COR_ARRAY_LAYOUT = unsafe { std::mem::zeroed() };
            if !unsafe { self.process5.GetArrayLayout(id, &mut array) }.is_ok() {
                return Layout::NoReferences;
            }
            let (references, slots) = match self.kind_of(array.componentType, array.componentID) {
                Kind::Reference => (true, Vec::new()),
                Kind::Value => {
                    let mut slots = Vec::new();
                    self.value_slots(array.componentID, 0, "", &mut slots, 0);
                    (false, slots)
                }
                Kind::Other => (false, Vec::new()),
            };
            if !references && slots.is_empty() {
                return Layout::NoReferences;
            }
            return Layout::Array { first: array.firstElementOffset, element_size: array.elementSize, count_offset: array.countOffset, references, slots };
        }
        let mut slots = Vec::new();
        self.object_slots(id, 0, "", &mut slots, 0);
        if slots.is_empty() {
            Layout::NoReferences
        } else {
            Layout::Object(slots)
        }
    }

    /// 型（と基底の型）のインスタンス フィールドのうち、参照のもの。`shift` を足した位置にする（値型を埋め込んだ所）。
    fn object_slots(&self, id: COR_TYPEID, shift: i64, prefix: &str, slots: &mut Vec<Slot>, nesting: usize) {
        let mut current = id;
        // 基底の型をたどる。壊れた情報で輪になっても止まるよう、深さを限る。
        for _ in 0..64 {
            if (current.token1 == 0 && current.token2 == 0) || nesting > 8 {
                return;
            }
            let mut layout: COR_TYPE_LAYOUT = unsafe { std::mem::zeroed() };
            if !unsafe { self.process5.GetTypeLayout(current, &mut layout) }.is_ok() {
                return;
            }
            if layout.numFields > 0 {
                let mut fields: Vec<COR_FIELD> = vec![unsafe { std::mem::zeroed() }; layout.numFields as usize];
                let mut fetched = 0u32;
                if unsafe { self.process5.GetTypeFields(current, layout.numFields, fields.as_mut_ptr(), &mut fetched) }.is_ok() {
                    let names = self.field_names(current, &fields[..fetched as usize]);
                    for (field, name) in fields.iter().take(fetched as usize).zip(names) {
                        let name = if prefix.is_empty() { name } else { format!("{prefix}.{name}") };
                        let offset = i64::from(field.offset) + shift;
                        match self.kind_of(field.fieldType, field.id) {
                            Kind::Reference if offset >= 0 => slots.push(Slot { offset: offset as u32, name: name.into() }),
                            Kind::Value => self.value_slots(field.id, offset, &name, slots, nesting + 1),
                            _ => {}
                        }
                    }
                }
            }
            current = layout.parentID;
        }
    }

    /// 値型の中の参照。値型のフィールドの位置（`GetTypeFields`）は、箱に入れた形ではなく、値そのものの頭から数える
    /// （参照型の位置はオブジェクトの頭から）。埋め込んだ所の位置 `at` を足すだけ。
    fn value_slots(&self, id: COR_TYPEID, at: i64, prefix: &str, slots: &mut Vec<Slot>, nesting: usize) {
        self.object_slots(id, at, prefix, slots, nesting);
    }

    /// 欄の型が、参照か、値型か、どちらでもない（数など）か。
    fn kind_of(&self, element: u32, id: COR_TYPEID) -> Kind {
        match element {
            ELEMENT_TYPE_STRING | ELEMENT_TYPE_CLASS | ELEMENT_TYPE_ARRAY | ELEMENT_TYPE_OBJECT | ELEMENT_TYPE_SZARRAY => Kind::Reference,
            ELEMENT_TYPE_VALUETYPE => Kind::Value,
            // ジェネリックの欄は、実際の型で決める。型の番号が来ない（まだ読み込まれていない型など）ときは参照として読む
            // （値型だった場合に読む値はオブジェクトの番地にならず、たどるときに `GetTypeID` が失敗して捨てられる）。
            ELEMENT_TYPE_GENERICINST | ELEMENT_TYPE_VAR | ELEMENT_TYPE_MVAR => {
                let mut layout: COR_TYPE_LAYOUT = unsafe { std::mem::zeroed() };
                if (id.token1 == 0 && id.token2 == 0) || !unsafe { self.process5.GetTypeLayout(id, &mut layout) }.is_ok() {
                    return Kind::Reference;
                }
                match layout.type_ {
                    ELEMENT_TYPE_VALUETYPE => Kind::Value,
                    ELEMENT_TYPE_STRING | ELEMENT_TYPE_CLASS | ELEMENT_TYPE_ARRAY | ELEMENT_TYPE_OBJECT | ELEMENT_TYPE_SZARRAY | ELEMENT_TYPE_GENERICINST => Kind::Reference,
                    _ => Kind::Other,
                }
            }
            // 型を引けない欄（.NET 10 の共有のジェネリック `List<参照型>` の `T[] _items` は、型が 0 で来る）も参照として読む。
            0 if id.token1 == 0 && id.token2 == 0 => Kind::Reference,
            _ => Kind::Other,
        }
    }

    /// 型が宣言したフィールドの名前（メタデータから。引けなければトークン）。
    fn field_names(&self, id: COR_TYPEID, fields: &[COR_FIELD]) -> Vec<String> {
        let metadata = (|| unsafe {
            let ty = take::<ICorDebugType>("ICorDebugProcess5::GetTypeForTypeID", |out| self.process5.GetTypeForTypeID(id, out)).ok()?;
            let class = take::<ICorDebugClass>("ICorDebugType::GetClass", |out| ty.GetClass(out)).ok()?;
            let module = take::<ICorDebugModule>("ICorDebugClass::GetModule", |out| class.GetModule(out)).ok()?;
            value::Modules::metadata(&self.session.modules, &module)
        })();
        fields
            .iter()
            .map(|field| metadata.and_then(|m| m.field_declaration(field.token & 0x00FF_FFFF)).map_or_else(|| format!("0x{:08X}", field.token), |(_, declared)| declared.name.to_string()))
            .collect()
    }
}

enum Kind {
    Reference,
    Value,
    Other,
}

const POINTER: usize = std::mem::size_of::<usize>();

/// バイト列の `at` から、ポインターの大きさの番地（エンジンと対象のビット数は同じ）。
fn pointer_at(bytes: &[u8], at: usize) -> u64 {
    bytes.get(at..at + POINTER).map_or(0, |b| {
        let mut word = [0u8; POINTER];
        word.copy_from_slice(b);
        usize::from_le_bytes(word) as u64
    })
}

/// 表の中のオブジェクトから親をたどって、ルートの番号。
fn root_of(visited: &HashMap<u64, Parent>, mut at: u64) -> usize {
    loop {
        match visited.get(&at) {
            Some(Parent::Root(index)) => return *index,
            Some(Parent::Object(parent, _)) => at = *parent,
            None => return 0,
        }
    }
}

/// GC のルート（弱い参照は除く）。参照しているオブジェクトの番地と種類。
unsafe fn gc_roots(process5: &ComPtr<ICorDebugProcess5>) -> Result<Vec<Root>, String> {
    let references = take::<ICorDebugGCReferenceEnum>("ICorDebugProcess5::EnumerateGCReferences", |out| process5.EnumerateGCReferences(0, out))?;
    let mut roots = Vec::new();
    let mut batch: Vec<COR_GC_REFERENCE> = vec![std::mem::zeroed(); 256];
    loop {
        let mut fetched = 0u32;
        if !references.Next(batch.len() as u32, batch.as_mut_ptr(), &mut fetched).is_ok() || fetched == 0 {
            break;
        }
        for reference in &batch[..fetched as usize] {
            // 受け取った COM のポインターは、こちらで放す。
            let _domain = ComPtr::<IUnknown>::from_raw(reference.Domain);
            let address = ComPtr::<ICorDebugValue>::from_raw(reference.Location).map_or(0, |location| object_address(&location));
            roots.push(Root { address, kind: reference.Type });
        }
    }
    Ok(roots)
}

fn root_kind(kind: u32) -> &'static str {
    const KINDS: [(u32, &str); 8] = [
        (CorReferenceStack, "stack"),
        (CorReferenceFinalizer, "finalizer queue"),
        (CorHandleStrong, "strong handle (static field, GCHandle, ...)"),
        (CorHandleStrongPinning, "pinned handle"),
        (CorHandleStrongRefCount, "ref-counted handle (COM)"),
        (CorHandleStrongDependent, "dependent handle"),
        (CorHandleStrongAsyncPinned, "async pinned handle"),
        (CorHandleStrongSizedByref, "sized-ref handle"),
    ];
    KINDS.iter().find(|(k, _)| *k == kind).map_or("other root", |(_, name)| name)
}
fn hex(address: u64) -> String {
    format!("0x{address:X}")
}
