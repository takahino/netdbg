//! ポインター（unsafe のコード）: `*p`、`p->m`、`p[i]`、`p + n`、`p - n`、`p - q`、比較、`&x`、ポインターの型へのキャスト、
//! `*p = v`。C# の意味（D-044）。
//!
//! ポインターの変数の値は、ICorDebug ではポインターの型（`ELEMENT_TYPE_PTR`）の参照の値（`ICorDebugReferenceValue`）で、
//! `GetValue` が番地、`Dereference` が指す先の値（対象のメモリの上の値。書き換えると対象に書かれる）。
//! ところが、計算した番地のポインターは作れない（`ICorDebugEval2::CreateValueForType` がポインターの型を受け付けない）。
//! そこで、計算したポインター（`p + 1`、`&x`）は、番地を入れた `ulong` の値で持ち、ポインターの型を `pointer_typed` に
//! 覚えておく（`dynamic_typed` と同じやり方。式を一つ評価する間だけ）。
//!
//! 指す先を読むとき、番地がポインターの変数の番地そのものなら、`Dereference` で対象の上の値を使う。ほかの番地なら、
//! 対象のメモリを読んで写しを作る（`pointer_copies` に、もとの番地と一緒に覚える）。書き込み（`*p = v`、`p[i] = v`）は、
//! 番地に直に書く。

use super::*;

const ELEMENT_TYPE_PTR: u32 = 0x0F;
const ELEMENT_TYPE_VOID: u32 = 0x01;
const ELEMENT_TYPE_VALUETYPE: u32 = 0x11;
const ELEMENT_TYPE_U8: u32 = 0x0B;

/// ポインターとして扱える値。
pub(super) struct Pointer {
    pub address: u64,
    /// ポインターの型（`int*`）。
    pub ty: ComPtr<ICorDebugType>,
    /// ポインターの変数の値そのもの（計算したポインターなら `None`）。
    real: Option<ComPtr<ICorDebugValue>>,
}

/// ポインターの幅（対象＝エンジンのビット数）で、番地を切り詰める。
fn wrap(address: u128) -> u64 {
    if std::mem::size_of::<usize>() == 4 {
        address as u64 & 0xFFFF_FFFF
    } else {
        address as u64
    }
}

/// ポインターの型の、指す先の型。
unsafe fn pointee_type(ty: &ComPtr<ICorDebugType>) -> Option<ComPtr<ICorDebugType>> {
    let mut raw = null_mut();
    ty.GetFirstTypeParameter(&mut raw);
    ComPtr::from_raw(raw)
}

unsafe fn element_of(ty: &ComPtr<ICorDebugType>) -> u32 {
    let mut element = 0;
    ty.GetType(&mut element);
    element
}

impl Session {
    /// 値がポインター（変数の値か、計算したもの）なら、その中身。
    pub(super) fn as_pointer(&self, value: &ComPtr<ICorDebugValue>) -> Option<Pointer> {
        if let Some((address, ty)) = unsafe { value::pointer_parts(value) } {
            return Some(Pointer { address, ty, real: Some(value.clone()) });
        }
        let (_, ty) = self.pointer_typed.iter().find(|(known, _)| known.as_raw() == value.as_raw())?;
        let Operand::Int { value: address, .. } = self.operand(value) else { return None };
        Some(Pointer { address: address as u64, ty: ty.clone(), real: None })
    }

    /// 番地 `address` の、型 `ty` のポインター（計算したポインター）を作る。
    pub(super) fn make_pointer(&mut self, thread: u32, address: u64, ty: &ComPtr<ICorDebugType>) -> Result<ComPtr<ICorDebugValue>, String> {
        let value = self.new_primitive(thread, ELEMENT_TYPE_U8, &address.to_le_bytes())?;
        self.pointer_typed.push((value.clone(), ty.clone()));
        Ok(value)
    }

    /// 型 `pointee` を指すポインターの型。
    fn pointer_type_to(&self, thread: u32, pointee: &ComPtr<ICorDebugType>) -> Result<ComPtr<ICorDebugType>, String> {
        let domain = self.app_domain(thread)?;
        unsafe { take::<ICorDebugType>("ICorDebugAppDomain2::GetArrayOrPointerType", |out| domain.GetArrayOrPointerType(ELEMENT_TYPE_PTR, 0, pointee.as_raw(), out)) }
    }

    /// 型の値の大きさ（ポインターの足し算で進む幅）。
    fn size_of_type(&self, thread: u32, ty: &ComPtr<ICorDebugType>) -> Result<u64, String> {
        let element = unsafe { element_of(ty) };
        match element {
            ELEMENT_TYPE_VOID => Err(tr!("void* points at something with no size, so it cannot be added to, subtracted from or indexed (as in C#).")),
            ELEMENT_TYPE_PTR => Ok(std::mem::size_of::<usize>() as u64),
            ELEMENT_TYPE_VALUETYPE => {
                // 構造体の大きさは、その型の値を一つ作って聞く（対象のコードは動かさない）。
                let process = self.process.as_ref().ok_or_else(|| tr!("There is no target process."))?;
                unsafe {
                    let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread, out))?;
                    let eval = take::<ICorDebugEval>("ICorDebugThread::CreateEval", |out| thread.CreateEval(out))?;
                    let eval2: ComPtr<ICorDebugEval2> = eval.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugEval2"), hr))?;
                    let sample = take::<ICorDebugValue>("ICorDebugEval2::CreateValueForType", |out| eval2.CreateValueForType(ty.as_raw(), out))?;
                    let mut size = 0u32;
                    sample.GetSize(&mut size).ok().map_err(|hr| failed("ICorDebugValue::GetSize", hr))?;
                    Ok(u64::from(size))
                }
            }
            element => match u8::try_from(element).ok().filter(|&e| (0x02..=0x0D).contains(&e) || matches!(e, 0x18 | 0x19)) {
                Some(element) => Ok(primitive_size(element) as u64),
                None => Err(tr!("A pointer to this type cannot be handled.")),
            },
        }
    }

    /// 対象のメモリを読む。
    fn read_memory(&self, address: u64, size: usize) -> Result<Vec<u8>, String> {
        let process = self.process.as_ref().ok_or_else(|| tr!("There is no target process."))?;
        let mut bytes = vec![0u8; size];
        let mut read = 0usize;
        let hr = unsafe { process.ReadMemory(address, size as u32, bytes.as_mut_ptr(), &mut read) };
        if !hr.is_ok() || read != size {
            return Err(tr!("Cannot read the address {address} ({code}). The pointer may not point at a valid place.", address = format!("0x{address:X}"), code = hr));
        }
        Ok(bytes)
    }

    /// 対象のメモリに書く。
    fn write_memory(&mut self, address: u64, bytes: &[u8]) -> Result<(), String> {
        let process = self.process.clone().ok_or_else(|| tr!("There is no target process."))?;
        let mut buffer = bytes.to_vec();
        let mut written = 0usize;
        let hr = unsafe { process.WriteMemory(address, buffer.len() as u32, buffer.as_mut_ptr(), &mut written) };
        if !hr.is_ok() || written != buffer.len() {
            return Err(tr!("Cannot write the address {address} ({code}).", address = format!("0x{address:X}"), code = hr));
        }
        self.touched_target = true;
        Ok(())
    }

    /// ポインターの指す先の値。
    fn read_through(&mut self, thread: u32, pointer: &Pointer) -> Result<ComPtr<ICorDebugValue>, String> {
        if pointer.address == 0 {
            return Err(tr!("What a null pointer points at cannot be read."));
        }
        let pointee = unsafe { pointee_type(&pointer.ty) }.ok_or_else(|| tr!("The type the pointer points at is not known."))?;
        // ポインターの変数そのものなら、対象の上の値（書き換えると、対象に書かれる）。
        if let Some(target) = pointer.real.as_ref().and_then(|real| unsafe { value::pointee(real) }) {
            return Ok(target);
        }
        let element = unsafe { element_of(&pointee) };
        let size = self.size_of_type(thread, &pointee)? as usize;
        let bytes = self.read_memory(pointer.address, size)?;
        let copy = match element {
            // ポインターを指すポインター（`int**`）: 読んだ番地の、指す先の型のポインター。
            ELEMENT_TYPE_PTR => {
                let mut wide = [0u8; 8];
                wide[..bytes.len()].copy_from_slice(&bytes);
                return self.make_pointer(thread, u64::from_le_bytes(wide), &pointee);
            }
            // 構造体: その型の空の値（ボックス）を作って、中身に読んだバイトを書き込む。
            ELEMENT_TYPE_VALUETYPE => {
                let boxed = self.new_object_of_type(thread, &pointee).map_err(|failure| failure.describe(self))?;
                let inside = unsafe { value::inside_box(&boxed) }.ok_or_else(|| tr!("Cannot create a copy of the struct."))?;
                let mut address = 0u64;
                unsafe { inside.GetAddress(&mut address) }.ok().map_err(|hr| failed("ICorDebugValue::GetAddress", hr))?;
                self.write_memory(address, &bytes)?;
                boxed
            }
            element => self.new_number(thread, element as u8, &bytes)?,
        };
        self.pointer_copies.push((copy.clone(), pointer.address));
        Ok(copy)
    }

    /// `*p`
    pub(super) fn dereference(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let pointer = self.as_pointer(value).ok_or_else(|| tr!("Only pointers can take *."))?;
        if unsafe { pointee_type(&pointer.ty) }.is_some_and(|ty| unsafe { element_of(&ty) } == ELEMENT_TYPE_VOID) {
            return Err(tr!("What void* points at cannot be read (as in C#, cast it to another pointer type)."));
        }
        self.read_through(thread, &pointer)
    }

    /// `p + n`（`n` は要素の数）。
    fn offset_pointer(&mut self, thread: u32, pointer: &Pointer, count: i128) -> Result<Pointer, String> {
        let pointee = unsafe { pointee_type(&pointer.ty) }.ok_or_else(|| tr!("The type the pointer points at is not known."))?;
        let size = self.size_of_type(thread, &pointee)?;
        let address = wrap((pointer.address as i128 + count * size as i128) as u128);
        // 足したあとも、もとの変数と同じ番地なら、もとの値を使い続ける（`p + 0`、`p[0]`）。
        let real = pointer.real.clone().filter(|_| address == pointer.address);
        Ok(Pointer { address, ty: pointer.ty.clone(), real })
    }

    /// 計算したポインターを、式の値にする。
    fn pointer_value(&mut self, thread: u32, pointer: Pointer) -> Result<ComPtr<ICorDebugValue>, String> {
        match pointer.real {
            Some(real) => Ok(real),
            None => self.make_pointer(thread, pointer.address, &pointer.ty),
        }
    }

    /// 整数の値（ポインターの足し算の相手、添字）。
    fn integer_of(&self, value: &ComPtr<ICorDebugValue>) -> Option<i128> {
        match self.operand(value) {
            Operand::Int { value, .. } => Some(value),
            _ => None,
        }
    }

    /// `p[i]`
    pub(super) fn pointer_index(&mut self, thread: u32, pointer: &Pointer, index: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let index = self.integer_of(index).ok_or_else(|| tr!("Write a pointer index as an integer."))?;
        let moved = self.offset_pointer(thread, pointer, index)?;
        self.read_through(thread, &moved)
    }

    /// 二項演算子の片方か両方がポインターのとき（`p + n`、`n + p`、`p - n`、`p - q`、比較）。どちらもポインターでなければ `None`。
    /// `null` は、相手がポインターのときだけ、番地 0 のポインター。
    pub(super) fn pointer_binary(&mut self, thread: u32, op: BinaryOp, left: &ComPtr<ICorDebugValue>, left_null: bool, right: &ComPtr<ICorDebugValue>, right_null: bool) -> Result<Option<Current>, String> {
        let (left_pointer, right_pointer) = (self.as_pointer(left), self.as_pointer(right));
        if left_pointer.is_none() && right_pointer.is_none() {
            return Ok(None);
        }
        let address = |pointer: &Option<Pointer>, null: bool| pointer.as_ref().map(|p| p.address).or(null.then_some(0));
        let compared = |result: bool, session: &mut Self| session.new_bool(thread, result).map(|value| Some(Current::Value(value)));
        match (op, &left_pointer, &right_pointer) {
            (BinaryOp::Add, Some(pointer), None) | (BinaryOp::Subtract, Some(pointer), None) => {
                let count = self.integer_of(right).ok_or_else(|| tr!("Only integers can be added to or subtracted from a pointer."))?;
                let count = if op == BinaryOp::Subtract { -count } else { count };
                let moved = self.offset_pointer(thread, pointer, count)?;
                Ok(Some(Current::Value(self.pointer_value(thread, moved)?)))
            }
            (BinaryOp::Add, None, Some(pointer)) => {
                let count = self.integer_of(left).ok_or_else(|| tr!("Only integers can be added to a pointer."))?;
                let moved = self.offset_pointer(thread, pointer, count)?;
                Ok(Some(Current::Value(self.pointer_value(thread, moved)?)))
            }
            // `p - q`: 間の要素の数（long）。C# と同じく、同じ型のポインターどうしだけ。
            (BinaryOp::Subtract, Some(left), Some(right)) => {
                let (left_name, right_name) = unsafe { (value::type_name(&left.ty, &self.modules), value::type_name(&right.ty, &self.modules)) };
                if left_name != right_name {
                    return Err(tr!("Pointers can be subtracted only from pointers of the same type ({left} and {right}).", left = left_name, right = right_name));
                }
                let pointee = unsafe { pointee_type(&left.ty) }.ok_or_else(|| tr!("The type the pointer points at is not known."))?;
                let size = self.size_of_type(thread, &pointee)? as i128;
                // 番地の差は、ポインターの幅の符号つきの数として計算する。
                let difference = if std::mem::size_of::<usize>() == 4 { (left.address as u32).wrapping_sub(right.address as u32) as i32 as i128 } else { left.address.wrapping_sub(right.address) as i64 as i128 };
                Ok(Some(Current::Value(self.operand_value(thread, Operand::Int { value: difference / size, kind: IntKind::I64 })?)))
            }
            (BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessOrEqual | BinaryOp::Greater | BinaryOp::GreaterOrEqual, _, _) => {
                let (Some(a), Some(b)) = (address(&left_pointer, left_null), address(&right_pointer, right_null)) else {
                    return Err(tr!("Operator {operator} can compare only pointers with each other (or null).", operator = operators::symbol(op)));
                };
                // 番地は符号なしの数として比べる（C# と同じ）。
                let result = match op {
                    BinaryOp::Equal => a == b,
                    BinaryOp::NotEqual => a != b,
                    BinaryOp::Less => a < b,
                    BinaryOp::LessOrEqual => a <= b,
                    BinaryOp::Greater => a > b,
                    _ => a >= b,
                };
                compared(result, self)
            }
            _ => Err(tr!("Operator {operator} cannot be used on pointers.", operator = operators::symbol(op))),
        }
    }

    /// `&式`: 変数、フィールド、配列の要素、ポインターの指す先の番地。
    pub(super) fn address_of(&mut self, thread: u32, depth: usize, operand: &Expr) -> Result<ComPtr<ICorDebugValue>, String> {
        // `&*p` は p、`&p[i]` は p + i（指す先を読まずに、番地だけを計算する）。
        if let Expr::Dereference(inner) = operand {
            let current = self.evaluate_node(thread, depth, inner)?;
            let value = self.value_of(current)?;
            self.as_pointer(&value).ok_or_else(|| tr!("Only pointers can take *."))?;
            return Ok(value);
        }
        if let Expr::Index(target, indices) = operand {
            let current = self.evaluate_node(thread, depth, target)?;
            let target = self.value_of(current)?;
            if let Some(pointer) = self.as_pointer(&target) {
                let [index] = indices.as_slice() else { return Err(tr!("A pointer takes only one index.")) };
                let index = self.evaluate_node(thread, depth, index)?;
                let index = self.value_of(index)?;
                let count = self.integer_of(&index).ok_or_else(|| tr!("Write a pointer index as an integer."))?;
                let moved = self.offset_pointer(thread, &pointer, count)?;
                return self.make_pointer(thread, moved.address, &moved.ty);
            }
        }
        // フィールド: 持ち主が、ポインターの指す先の写しなら、写しの中の位置から、もとの番地を割り出す。
        let (value, address) = match operand {
            Expr::Member(owner, name) => {
                let current = self.evaluate_node(thread, depth, owner)?;
                let owner = self.value_of(current)?;
                let field = unsafe { value::field(&owner, &self.modules, name) }.ok_or_else(|| tr!("No field named '{name}' was found (& can be put on variables, fields and elements).", name = name))?;
                let address = self.home_of(&field)?;
                let address = match self.copy_source(&owner) {
                    Some(source) => {
                        let inside = unsafe { value::inside_box(&owner) }.unwrap_or_else(|| owner.clone());
                        wrap((source as u128).wrapping_add(address.wrapping_sub(self.home_of(&inside)?) as u128))
                    }
                    None => address,
                };
                (field, address)
            }
            _ => {
                let current = self.evaluate_node(thread, depth, operand)?;
                let value = self.value_of(current)?;
                let address = match self.copy_source(&value) {
                    Some(source) => source,
                    None => self.home_of(&value)?,
                };
                (value, address)
            }
        };
        let ty = unsafe { value::exact_type_of(&value) }.ok_or_else(|| tr!("The value's type is not known."))?;
        let pointer_type = self.pointer_type_to(thread, &ty)?;
        self.make_pointer(thread, address, &pointer_type)
    }

    /// 値の、対象のメモリの上の番地。
    fn home_of(&self, value: &ComPtr<ICorDebugValue>) -> Result<u64, String> {
        let mut address = 0u64;
        unsafe { value.GetAddress(&mut address) }.ok().map_err(|hr| failed("ICorDebugValue::GetAddress", hr))?;
        if address == 0 {
            return Err(tr!("This value has no address (it is in a register, or the result of a calculation, for example). & can be put on variables, fields and elements."));
        }
        Ok(address)
    }

    /// 値が、ポインターの指す先を読んで作った写しなら、もとの番地。
    fn copy_source(&self, value: &ComPtr<ICorDebugValue>) -> Option<u64> {
        self.pointer_copies.iter().find(|(copy, _)| copy.as_raw() == value.as_raw()).map(|(_, address)| *address)
    }

    /// 値が、ポインターの指す先を読んで作った写しか（書いても対象に届かない）。
    pub(super) fn is_pointer_copy(&self, value: &ComPtr<ICorDebugValue>) -> bool {
        self.copy_source(value).is_some()
    }

    /// `(型*)式` と、ポインターから整数へのキャスト（`(long)p`）。ポインターに関係しなければ `None`。
    pub(super) fn pointer_cast(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, value: &ComPtr<ICorDebugValue>, null_literal: bool) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let pointer = self.as_pointer(value);
        if ty.pointers > 0 {
            let address = match (&pointer, null_literal) {
                (Some(pointer), _) => pointer.address,
                (None, true) => 0,
                (None, false) => wrap(self.integer_of(value).ok_or_else(|| tr!("Only pointers and integers can be cast to a pointer type."))? as u128),
            };
            let target = self.runtime_type(thread, depth, ty)?;
            return Ok(Some(self.make_pointer(thread, address, &target)?));
        }
        let Some(pointer) = pointer else { return Ok(None) };
        // ポインターから整数へ: 番地を、その型の幅に切り詰める（unchecked の変換と同じ）。
        let element = primitive_element(&ty.name).filter(|_| !ty.nullable && !ty.array).filter(|e| IntKind::from_element(*e).is_some()).ok_or_else(|| tr!("A pointer cannot be cast to {type} (only to integer types or pointer types).", type = ty.name))?;
        let kind = if std::mem::size_of::<usize>() == 4 { IntKind::U32 } else { IntKind::U64 };
        let bytes = operators::convert(&Operand::Int { value: pointer.address as i128, kind }, element).ok_or_else(|| tr!("Cannot make the address an integer."))?;
        Ok(Some(self.new_number(thread, element, &bytes)?))
    }

    /// `*p = v`、`p[i] = v` の代入先の番地と、指す先の型。ポインターの指す先でなければ `None`。
    pub(super) fn pointer_place(&mut self, thread: u32, depth: usize, target: &Expr) -> Result<Option<(u64, ComPtr<ICorDebugType>)>, String> {
        let (pointer, count) = match target {
            Expr::Dereference(inner) => {
                let current = self.evaluate_node(thread, depth, inner)?;
                let value = self.value_of(current)?;
                (self.as_pointer(&value).ok_or_else(|| tr!("Only pointers can take *."))?, 0)
            }
            Expr::Index(owner, indices) => {
                let current = self.evaluate_node(thread, depth, owner)?;
                let value = self.value_of(current)?;
                let Some(pointer) = self.as_pointer(&value) else { return Ok(None) };
                let [index] = indices.as_slice() else { return Err(tr!("A pointer takes only one index.")) };
                let index = self.evaluate_node(thread, depth, index)?;
                let index = self.value_of(index)?;
                (pointer, self.integer_of(&index).ok_or_else(|| tr!("Write a pointer index as an integer."))?)
            }
            _ => return Ok(None),
        };
        let moved = self.offset_pointer(thread, &pointer, count)?;
        if moved.address == 0 {
            return Err(tr!("What a null pointer points at cannot be written."));
        }
        let pointee = unsafe { pointee_type(&moved.ty) }.ok_or_else(|| tr!("The type the pointer points at is not known."))?;
        Ok(Some((moved.address, pointee)))
    }

    /// ポインターの指す先（番地 `address`、型 `pointee`）に値を書く。数とポインターだけ（構造体は、メンバーごとに書く）。
    pub(super) fn write_through(&mut self, address: u64, pointee: &ComPtr<ICorDebugType>, value: &ComPtr<ICorDebugValue>) -> Result<(), String> {
        self.refuse_write_while_observing()?;
        let element = unsafe { element_of(pointee) };
        let bytes = match element {
            ELEMENT_TYPE_PTR => {
                let pointer = self.as_pointer(value).ok_or_else(|| tr!("Write a pointer as the target of the pointer."))?;
                pointer.address.to_le_bytes()[..std::mem::size_of::<usize>()].to_vec()
            }
            ELEMENT_TYPE_VALUETYPE | ELEMENT_TYPE_VOID => return Err(tr!("Only numbers and pointers can be written where a pointer points (for a struct, write each member, like p->X = 1).")),
            element => operators::convert(&self.operand(value), element as u8).ok_or_else(|| tr!("This value cannot be made the type the pointer points at."))?,
        };
        self.write_memory(address, &bytes)
    }

    /// 式の結果がポインターなら、表示のための型の名前と指す先（null と void* は `None`）。
    pub(super) fn pointer_result(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Option<(String, Option<ComPtr<ICorDebugValue>>, u64)> {
        let pointer = self.as_pointer(value)?;
        let type_name = unsafe { value::type_name(&pointer.ty, &self.modules) };
        let is_void = unsafe { pointee_type(&pointer.ty) }.is_some_and(|ty| unsafe { element_of(&ty) } == ELEMENT_TYPE_VOID);
        let target = if pointer.address == 0 || is_void { None } else { self.read_through(thread, &pointer).ok() };
        Some((type_name, target, pointer.address))
    }
}
