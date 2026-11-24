//! スレッドとコールスタック。

use super::*;

impl Session {
    /// `pause`: 動いている対象を止める。
    pub fn pause(&mut self) -> Result<Vec<Event>, String> {
        match self.state {
            State::Running => {}
            // すでに止まっているなら、することは無い（クライアントには止まった通知が届いている）。
            State::Stopped => return Ok(Vec::new()),
            State::NotStarted | State::Exited => return Err(tr!("The target is not running.")),
        }
        let process = self.process.clone().ok_or_else(|| tr!("The target's runtime has not started yet, so it cannot be stopped. Wait a little and try again."))?;
        unsafe { process.Stop(0) }.ok().map_err(|hr| failed(tr_str!("Pausing"), hr))?;
        self.stopped_controller = Some(process.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugController"), hr))?);
        self.state = State::Stopped;
        // どのスレッドで止まったとも言えないので、最初のスレッドを選んで見せる。
        let thread = self.threads.first().copied().unwrap_or(0);
        Ok(vec![("stopped", json!({ "reason": "pause", "threadId": thread, "allThreadsStopped": true }))])
    }

    /// `threads`。名前は netcoredbg と同じ: `Thread.Name`、無ければ最初のスレッドは `Main Thread`、ほかは `<No name>`。
    pub fn threads(&self) -> Result<Value, String> {
        let mut threads = Vec::new();
        // 名前は対象のメモリにあるので、動いている間は一時的に止めて読む。
        self.while_synchronized(|| {
            // `Main Thread` を先頭に（netcoredbg と同じ）。ほかは知らされた順。
            let ordered = self.main_thread.filter(|main| self.threads.contains(main)).into_iter().chain(self.threads.iter().copied().filter(|&id| Some(id) != self.main_thread));
            for id in ordered {
                let name = self.thread_name(id).unwrap_or_else(|| if self.main_thread == Some(id) { "Main Thread".into() } else { "<No name>".into() });
                threads.push(json!({ "id": id, "name": name }));
            }
        });
        Ok(json!({ "threads": threads }))
    }

    /// スレッドに付けられた名前（`Thread.Name`）。プロパティは呼ばず、裏のフィールドを読む。
    pub(super) fn thread_name(&self, id: u32) -> Option<String> {
        let process = self.process.as_ref()?;
        unsafe {
            let mut raw = null_mut();
            process.GetThread(id, &mut raw);
            let thread = ComPtr::<ICorDebugThread>::from_raw(raw)?;
            let mut object = null_mut();
            thread.GetObject(&mut object);
            let object = ComPtr::<ICorDebugValue>::from_raw(object)?;
            // フィールドの名前は .NET では `_name`、.NET Framework では `m_Name`。
            value::string_field(&object, &self.modules, &["_name", "m_Name"]).filter(|name| !name.is_empty())
        }
    }

    /// `stackTrace`。
    pub fn stack_trace(&mut self, arguments: &Value) -> Result<Value, String> {
        let id = arguments.get("threadId").and_then(Value::as_u64).ok_or_else(|| tr!("stackTrace has no threadId."))? as u32;
        let start = arguments.get("startFrame").and_then(Value::as_u64).unwrap_or(0) as usize;
        let levels = arguments.get("levels").and_then(Value::as_u64).filter(|&l| l > 0).map_or(usize::MAX, |l| l as usize);
        if self.state != State::Stopped {
            return Err(tr!("The call stack can be read only while the target is stopped."));
        }
        let mut natives = Vec::new();
        let walked = self.walk(id, Some(&mut natives))?;
        // PDB の無いアプリのモジュールのフレームは、ここで逆コンパイルして、ソースの場所を見せる（D-060）。
        for frame in &walked {
            self.decompile_for_frame(frame);
        }
        let mut frames = Vec::new();
        for (depth, frame) in walked.iter().enumerate() {
            let frame_id = self.frame_id(id, depth);
            frames.push(unsafe { self.describe_frame(frame_id, frame) });
        }
        // await のあとで再開した async メソッドなら、await している呼び出し元を挟む（async_stack.rs）。
        let frames = self.with_async_callers(id, &walked, frames);
        // マネージドのフレームのあいだに、渡った先のネイティブのモジュールを 1 行ずつ挟む（D-090）。
        let frames = self.with_native_boundaries(id, &walked, frames, &natives);
        let total = frames.len();
        let page: Vec<Value> = frames.into_iter().skip(start).take(levels).collect();
        Ok(json!({ "stackFrames": page, "totalFrames": total }))
    }

    /// スレッドのマネージドのフレームを、いま実行中のものから呼び出し元へ順に並べる。
    ///
    /// `GetActiveFrame` と `GetCaller` は、ネイティブのコードとの境界（`Thread.Sleep` の中など）で途切れる。
    /// スタックウォークなら境界を越えて全部たどれる。ネイティブのフレームと、ランタイム内部のフレームは飛ばす。
    pub(super) fn walk_frames(&self, thread: u32) -> Result<Vec<ComPtr<ICorDebugFrame>>, String> {
        self.walk(thread, None)
    }

    /// `walk_frames` と同じ。`natives` があれば、ネイティブのフレームの命令の番地を、そのすぐ下にあるマネージドの
    /// フレームの添字とともに積む（添字が数と同じなら、いちばん下）。
    fn walk(&self, thread: u32, mut natives: Option<&mut Vec<(usize, usize)>>) -> Result<Vec<ComPtr<ICorDebugFrame>>, String> {
        let process = self.process.as_ref().ok_or_else(|| tr!("There is no target process."))?;
        let mut frames = Vec::new();
        unsafe {
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread, out))?;
            let thread3: ComPtr<ICorDebugThread3> = thread.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugThread3"), hr))?;
            // まだマネージドのコードを一度も実行していないスレッドは、ウォークを作れない。フレーム無しとして扱う。
            let mut raw = null_mut();
            if !thread3.CreateStackWalk(&mut raw).is_ok() {
                return Ok(frames);
            }
            let Some(walk) = ComPtr::<ICorDebugStackWalk>::from_raw(raw) else { return Ok(frames) };
            // 壊れたスタックで回り続けないように、数を限る。
            for _ in 0..10_000 {
                let mut raw = null_mut();
                let hr = walk.GetFrame(&mut raw);
                if !hr.is_ok() {
                    break;
                }
                // ネイティブのフレームでは S_FALSE と null が返る。
                match ComPtr::<ICorDebugFrame>::from_raw(raw) {
                    Some(frame) => {
                        if frame.cast::<ICorDebugILFrame>().is_ok() {
                            frames.push(frame);
                        }
                    }
                    None => {
                        if let Some(natives) = natives.as_deref_mut() {
                            let mut context = vec![0u8; crate::win32::CONTEXT_SIZE];
                            let mut size = 0u32;
                            if walk.GetContext(crate::win32::CONTEXT_CONTROL, context.len() as u32, &mut size, context.as_mut_ptr()).is_ok() {
                                if let Some(ip) = crate::win32::context_ip(&context).filter(|&ip| ip != 0) {
                                    natives.push((frames.len(), ip));
                                }
                            }
                        }
                    }
                }
                // 終わりは成功コードの CORDBG_S_AT_END_OF_STACK で知らされる。S_OK 以外はすべて「ここまで」。
                if walk.Next().0 != 0 {
                    break;
                }
            }
            // 例外で止まったとき、新しいランタイム（.NET 11 で確認）では、スタックの先頭にランタイム自身の例外処理
            // （System.Runtime.EH.DispatchEx など）が載っている。利用者にとっての「止まった場所」は、その下の、例外を
            // 投げたフレーム。先頭に並んだ分だけを除く。
            let machinery = frames.iter().take_while(|frame| self.is_exception_machinery(frame)).count();
            frames.drain(..machinery);
            if let Some(natives) = natives {
                // 除いたフレームより上のネイティブのフレームは、例外処理の中のもの。
                natives.retain(|(position, _)| *position >= machinery);
                natives.iter_mut().for_each(|(position, _)| *position -= machinery);
            }
        }
        Ok(frames)
    }

    /// フレームが、ランタイムの例外処理の内部のものか。
    pub(super) unsafe fn is_exception_machinery(&self, frame: &ComPtr<ICorDebugFrame>) -> bool {
        // シンボルのあるモジュール（利用者のコード）は、名前を調べるまでもない。
        let Some(module) = self.frame_module(frame).filter(|m| m.pdb.is_none()) else { return false };
        let mut token = 0u32;
        frame.GetFunctionToken(&mut token);
        module.metadata().and_then(|m| m.method_name(token)).is_some_and(|name| name.starts_with("System.Runtime.EH."))
    }

    /// フレームのメソッドがあるモジュール。
    pub(super) unsafe fn frame_module(&self, frame: &ComPtr<ICorDebugFrame>) -> Option<&Module> {
        let mut function = null_mut();
        frame.GetFunction(&mut function);
        let function = ComPtr::<ICorDebugFunction>::from_raw(function)?;
        let mut module = null_mut();
        function.GetModule(&mut module);
        let path = module_path(&ComPtr::<ICorDebugModule>::from_raw(module)?);
        self.modules.iter().find(|m| m.path == path)
    }

    /// クライアントが送ってきた frameId の「スレッド、深さ」。
    pub(super) fn frame_of(&self, arguments: &Value) -> Result<(u32, usize), String> {
        let id = arguments.get("frameId").and_then(Value::as_u64).ok_or_else(|| tr!("There is no frameId."))?;
        if self.state != State::Stopped {
            return Err(tr!("Variables can be read only while the target is stopped."));
        }
        // 番号は usize にする前に比べる（32bit では上の桁が落ちて、ふつうのフレームの番号に見えてしまう）。
        if id >= async_stack::LOGICAL_FRAME_BASE as u64 {
            return Err(tr!("This is an awaiting caller (not a running frame), so its variables and expressions cannot be read."));
        }
        let id = usize::try_from(id).map_err(|_| tr!("That frameId is not from the current stop. Get the stackTrace again."))?;
        id.checked_sub(1).and_then(|index| self.frames.get(index)).copied().ok_or_else(|| tr!("That frameId is not from the current stop. Get the stackTrace again."))
    }

    /// 「スレッド、深さ」に当たるフレームの番号。同じ停止の間は同じ番号を返す。
    pub(super) fn frame_id(&mut self, thread: u32, depth: usize) -> i64 {
        let index = self.frames.iter().position(|f| *f == (thread, depth)).unwrap_or_else(|| {
            self.frames.push((thread, depth));
            self.frames.len() - 1
        });
        index as i64 + 1
    }

    /// スレッドの上から `limit` 個のフレームの説明（番号は付けない）。記録の呼び出し元を探すのに使う。
    pub(super) fn describe_top_frames(&self, thread: u32, limit: usize) -> Vec<Value> {
        let Ok(frames) = self.walk_frames(thread) else { return Vec::new() };
        frames.iter().take(limit).map(|frame| unsafe { self.describe_frame(0, frame) }).collect()
    }

    pub(super) unsafe fn describe_frame(&self, id: i64, frame: &ComPtr<ICorDebugFrame>) -> Value {
        let mut token = 0u32;
        frame.GetFunctionToken(&mut token);
        // 名前が分からないとき（メタデータを読めないモジュール、マネージドでないフレーム）はトークンを見せる。
        let mut described = json!({ "id": id, "name": format!("[0x{token:08X}]"), "line": 0, "column": 0 });

        let Some(module) = self.frame_module(frame) else { return described };
        // 型引数は、実際の型で書く（netcoredbg と同じ。`Task<int>.InnerInvoke()`）。
        let arguments: Vec<String> = frame
            .cast::<ICorDebugILFrame2>()
            .ok()
            .and_then(|frame2| take::<ICorDebugTypeEnum>("ICorDebugILFrame2::EnumerateTypeParameters", |out| frame2.EnumerateTypeParameters(out)).ok())
            .map(|types| {
                let mut names = Vec::new();
                loop {
                    let (mut raw, mut fetched) = (null_mut(), 0u32);
                    if !types.Next(1, &mut raw, &mut fetched).is_ok() || fetched == 0 {
                        break;
                    }
                    match ComPtr::<ICorDebugType>::from_raw(raw) {
                        Some(ty) => names.push(value::type_name(&ty, &self.modules)),
                        None => break,
                    }
                }
                names
            })
            .unwrap_or_default();
        if let Some(name) = module.metadata().and_then(|m| m.method_name_with(token, &arguments)) {
            // 引数は書かず、空の括弧を付ける（netcoredbg と同じ形。拡張は `Program.Work(` で探す）。
            described["name"] = json!(format!("{name}()"));
        }

        let Ok(il_frame) = frame.cast::<ICorDebugILFrame>() else { return described };
        let (mut offset, mut mapping) = (0u32, 0);
        il_frame.GetIP(&mut offset, &mut mapping);
        let Some(pdb) = module.pdb.as_ref() else { return described };
        if let Some(point) = pdb.find_visible_point(token, offset, frame_version(frame)) {
            if let Some(document) = pdb.document(point.document) {
                let name = Path::new(document).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                described["source"] = json!({ "name": name, "path": document });
                described["line"] = json!(point.start_line);
                described["column"] = json!(point.start_column);
                described["endLine"] = json!(point.end_line);
                described["endColumn"] = json!(point.end_column);
                // メソッドの行の範囲（専用の画面が、通った行に変数の値を出す範囲。VS Code の拡張は使わない）。
                if let Some((first, last)) = pdb.method_lines(token, frame_version(frame), point.document) {
                    described["netdbgMethodLines"] = json!({ "first": first, "last": last });
                }
            }
        }
        described
    }
}

/// 説明したフレームのうち、`skip` 段より下で最初の、ソースのあるもの（利用者のコードの呼び出し元）の、名前、行、ソース。
pub(super) fn user_caller(described: &[Value], skip: usize) -> Option<Value> {
    described.iter().skip(skip).find(|f| f.pointer("/source/path").is_some()).map(|f| json!({ "name": f["name"], "line": f["line"], "source": f["source"] }))
}
