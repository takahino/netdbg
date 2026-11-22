# Translation glossary

The screen and the messages of netdbg are written in English (the source) and translated with the tables in this folder
(`<language>.json`: English text → translation). The Chinese and Korean tables were translated by AI; corrections are
welcome (issue or pull request).

Terms follow the ones Visual Studio uses in each language, so that they read like the debugger users already know.
Keep `{name}` placeholders as they are; `{{` and `}}` are literal braces. The unit test
`i18n::tests::every_text_has_a_translation` checks that every text has a translation with the same placeholders.

| English | 简体中文 (zh-Hans) | 繁體中文 (zh-Hant) | 한국어 (ko) | 日本語 (ja) |
|---|---|---|---|---|
| breakpoint | 断点 | 中斷點 | 중단점 | ブレークポイント |
| data breakpoint (watch for writes) | 数据断点 | 資料中斷點 | 데이터 중단점 | 書き込みの見張り |
| tracepoint / log point | 跟踪点 | 追蹤點 | 추적점 | トレースポイント |
| Continue | 继续 | 繼續 | 계속 | 続行 |
| Pause | 暂停 | 暫停 | 일시 중지 | 一時停止 |
| Step Over | 逐过程 | 不進入函式 | 프로시저 단위 실행 | ステップ オーバー |
| Step Into | 逐语句 | 逐步執行 | 한 단계씩 코드 실행 | ステップ イン |
| Step Out | 跳出 | 跳離函式 | 프로시저 나가기 | ステップ アウト |
| Stop | 停止 | 停止 | 중지 | 終わらせる |
| Restart | 重新启动 | 重新啟動 | 다시 시작 | もう一度 |
| set next statement | 设置下一条语句 | 設定下一個陳述式 | 다음 문 설정 | 次の文の設定 |
| exception | 异常 | 例外狀況 | 예외 | 例外 |
| variable / local | 变量 / 局部变量 | 變數 / 區域變數 | 변수 / 지역 | 変数 / ローカル |
| Watch | 监视 | 監看式 | 조사식 | ウォッチ |
| Call Stack | 调用堆栈 | 呼叫堆疊 | 호출 스택 | コール スタック |
| thread | 线程 | 執行緒 | 스레드 | スレッド |
| module | 模块 | 模組 | 모듈 | モジュール |
| Output | 输出 | 輸出 | 출력 | 出力 |
| source (file) | 源文件 | 原始程式檔 | 소스 | ソース |
| expression / evaluate | 表达式 / 求值 | 運算式 / 評估 | 식 / 계산 | 式 / 評価 |
| symbols (PDB) | 符号 | 符號 | 기호 | シンボル |
| decompile | 反编译 | 反編譯 | 디컴파일 | 逆コンパイル |
| Edit and Continue | 编辑并继续 | 編輯後繼續 | 편집하며 계속하기 | Edit and Continue |
| launch / attach / detach | 启动 / 附加 / 分离 | 啟動 / 附加 / 中斷連結 | 시작 / 연결 / 분리 | 起動 / アタッチ / 切り離す |
| observe only | 仅观察 | 僅觀察 | 관찰 전용 | 観察専用 |
| process | 进程 | 處理序 | 프로세스 | プロセス |
| crash dump | 故障转储 | 損毀傾印 | 크래시 덤프 | クラッシュ ダンプ |
| heap | 堆 | 堆積 | 힙 | ヒープ |
| retention path | 保留路径 | 保留路徑 | 보존 경로 | 保持している経路 |
| coverage | 覆盖率 | 涵蓋範圍 | 커버리지 | カバレッジ |
| test report | 测试报告 | 測試報告 | 테스트 보고서 | 成績書 |
| record (noun) / log | 记录 | 記錄 | 기록 | 記録 |
| window (of the program) | 窗口 | 視窗 | 창 | 窓、画面 |
| control | 控件 | 控制項 | 컨트롤 | コントロール |
| screenshot / picture | 截图 | 截圖 | 스크린샷 | 写し、画面 |
| dialog | 对话框 | 對話方塊 | 대화 상자 | ダイアログ |
| UI freeze | 界面冻结 | UI 凍結 | UI 멈춤 | UI のフリーズ |
| bitness (32-bit / 64-bit) | 位数 | 位元數 | 비트 수 | ビット数 |
| engine | 引擎 | 引擎 | 엔진 | エンジン |
