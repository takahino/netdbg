//! 画面の文言（1 か所に集める。D-084）。元の文は英語で、ほかの言語は訳の表（`i18n/<言語>.json`）で引く。
//! 言語は、エンジンが頁の `<html lang>` に書いたもの（`crate::i18n`）。文言を足したら、訳の表にも足す
//! （`tools/i18n-add.ps1`。足し忘れはエンジンの単体試験 `i18n::tests::every_text_has_a_translation` が見つける）。

use crate::i18n::tr;

pub fn open() -> &'static str {
    tr("Open")
}
pub fn restart() -> &'static str {
    tr("Restart")
}
pub fn resume() -> &'static str {
    tr("Continue (F5)")
}
pub fn pause() -> &'static str {
    tr("Pause")
}
pub fn step_over() -> &'static str {
    tr("Step Over (F10)")
}
pub fn step_in() -> &'static str {
    tr("Step Into (F11)")
}
pub fn step_out() -> &'static str {
    tr("Step Out (Shift+F11)")
}
pub fn terminate() -> &'static str {
    tr("Stop (Shift+F5)")
}
pub fn terminate_title() -> &'static str {
    tr("End the launched program and stop debugging.")
}
pub fn detach() -> &'static str {
    tr("Detach (Shift+F5)")
}
pub fn detach_title() -> &'static str {
    tr("Stop debugging. The attached program keeps running.")
}
pub fn close_dump() -> &'static str {
    tr("Close (Shift+F5)")
}
pub fn detached() -> &'static str {
    tr("Detached (the program keeps running)")
}
pub fn quit() -> &'static str {
    tr("Quit")
}
pub fn quit_title() -> &'static str {
    tr("Stop the target and quit the engine")
}
pub fn idle() -> &'static str {
    tr("Nothing is open")
}
pub fn starting() -> &'static str {
    tr("Starting…")
}
pub fn running() -> &'static str {
    tr("Running")
}
pub fn stopped() -> &'static str {
    tr("Stopped")
}
pub fn ended() -> &'static str {
    tr("Ended")
}
pub fn dump() -> &'static str {
    tr("Dump (read only)")
}
pub fn sources() -> &'static str {
    tr("Sources")
}
pub fn variables() -> &'static str {
    tr("Variables")
}
pub fn watch() -> &'static str {
    tr("Watch")
}
pub fn stack() -> &'static str {
    tr("Call Stack")
}
pub fn threads() -> &'static str {
    tr("Threads")
}
pub fn output() -> &'static str {
    tr("Output")
}
pub fn agent() -> &'static str {
    tr("AI Actions")
}
pub fn modules() -> &'static str {
    tr("Modules")
}
pub fn windows() -> &'static str {
    tr("Windows")
}
pub fn no_source() -> &'static str {
    tr("No source. When the program stops, its source opens here (decompiled C# for assemblies without a PDB).")
}
pub fn not_stopped() -> &'static str {
    tr("Shown while stopped.")
}
pub fn add_watch() -> &'static str {
    tr("Add an expression (Enter)")
}
pub fn evaluate() -> &'static str {
    tr("Evaluate an expression (Enter)")
}
pub fn no_actions() -> &'static str {
    tr("No AI (MCP) actions yet.")
}
pub fn load_tree() -> &'static str {
    tr("Read the windows")
}
pub fn tree_stopped() -> &'static str {
    tr("While stopped, the names and text of controls cannot be read (continue, then read again).")
}
pub fn open_title() -> &'static str {
    tr("Open a Program")
}
pub fn program_tab() -> &'static str {
    tr("Launch a Program")
}
pub fn attach_tab() -> &'static str {
    tr("Attach to a Process")
}
pub fn program() -> &'static str {
    tr("A program (.exe / .dll) or a dump (.dmp). Click a source file (.cs / .vb / .fs) to open it")
}
pub fn arguments() -> &'static str {
    tr("Arguments")
}
pub fn launch() -> &'static str {
    tr("Launch")
}
pub fn attach() -> &'static str {
    tr("Attach")
}
pub fn cancel() -> &'static str {
    tr("Close")
}
pub fn up() -> &'static str {
    tr("↑ Up")
}
pub fn drives() -> &'static str {
    tr("Drives")
}
pub fn reload() -> &'static str {
    tr("Reload")
}
pub fn relaunch() -> &'static str {
    tr("Reopen with the engine of the matching bitness")
}
pub fn close() -> &'static str {
    tr("×")
}
pub fn decompiled() -> &'static str {
    tr("Decompiled")
}
pub fn decompile() -> &'static str {
    tr("Decompile and open")
}
pub fn filter() -> &'static str {
    tr("Filter by name")
}
pub fn open_file() -> &'static str {
    tr("From a file…")
}
pub fn open_file_title() -> &'static str {
    tr("Choose a source file (.cs / .vb / .fs) to open")
}
pub fn sources_before_start() -> &'static str {
    tr("Before launching, open a source with \"From a file…\" to set breakpoints (they are sent at launch). After launch, the sources known to the PDB are listed here.")
}
pub fn records() -> &'static str {
    tr("Records")
}
pub fn usage() -> &'static str {
    tr("Usage")
}
pub fn record_sql() -> &'static str {
    tr("SQL")
}
pub fn record_exception() -> &'static str {
    tr("Exceptions")
}
pub fn record_event() -> &'static str {
    tr("Events")
}
pub fn record_dialog() -> &'static str {
    tr("Dialogs")
}
pub fn record_transaction() -> &'static str {
    tr("Transactions")
}
pub fn record_binding() -> &'static str {
    tr("Bindings")
}
pub fn record_process() -> &'static str {
    tr("Child Processes")
}
pub fn record_freeze() -> &'static str {
    tr("UI Freezes")
}
pub fn record_environment() -> &'static str {
    tr("Environment")
}
pub fn clear() -> &'static str {
    tr("Clear")
}
pub fn no_records() -> &'static str {
    tr("No records yet. SQL, exceptions, dialogs, data binding failures, child processes and UI freezes are recorded without stopping.")
}
pub fn log_sql() -> &'static str {
    tr("Record SQL")
}
pub fn log_exceptions() -> &'static str {
    tr("Record exceptions")
}
pub fn log_hint() -> &'static str {
    tr("Turn SQL on before launching to be sure (providers that are already loaded may not be readable).")
}
pub fn diff() -> &'static str {
    tr("Diff")
}
pub fn diff_old_placeholder() -> &'static str {
    tr("The path of the previous exe, dll or folder (compared with the program being debugged)")
}
pub fn diff_run() -> &'static str {
    tr("Compare")
}
pub fn diff_retest_only() -> &'static str {
    tr("Retest scope only")
}
pub fn diff_hint() -> &'static str {
    tr("Shows the methods that changed in the replaced exe. With coverage, methods that changed but were not run by the tests (the retest scope) are marked ○.")
}
pub fn report() -> &'static str {
    tr("Test Report")
}
pub fn report_id_placeholder() -> &'static str {
    tr("Number (1-2-3)")
}
pub fn report_title_placeholder() -> &'static str {
    tr("Item (what it checks)")
}
pub fn report_note_placeholder() -> &'static str {
    tr("Note (written when ending)")
}
pub fn report_start() -> &'static str {
    tr("Start")
}
pub fn report_capture() -> &'static str {
    tr("Capture the screen")
}
pub fn report_ok() -> &'static str {
    tr("End as OK")
}
pub fn report_ng() -> &'static str {
    tr("End as NG")
}
pub fn report_write() -> &'static str {
    tr("Write the report")
}
pub fn report_written() -> &'static str {
    tr("Report:")
}
pub fn report_recording() -> &'static str {
    tr("Recording:")
}
pub fn report_hint() -> &'static str {
    tr("Enter a number and an item, then \"Start\": until you end it, the AI's actions, the buttons pressed and text typed by hand, SQL, messages and the time are recorded. Each action gets a picture of the screen with the operated spot painted, and a picture after it. When you end, the changed controls are listed and the Excel and HTML (one file) reports are rewritten.")
}
pub fn report_template_placeholder() -> &'static str {
    tr("Template folder (report.html / report.xlsx; empty for the default layout)")
}
pub fn report_template_hint() -> &'static str {
    tr("To match your site's layout, enter a folder here, press \"Export the default templates\", and edit the exported report.html / report.xlsx. The names you can use are listed in the exported names.txt.")
}
pub fn report_template_export() -> &'static str {
    tr("Export the default templates")
}
pub fn report_template_exported() -> &'static str {
    tr("Exported the default templates:")
}
pub fn report_template_needed() -> &'static str {
    tr("Enter the folder to export to in the template field (for example D:\\tests\\layout).")
}
pub fn output_folder_placeholder() -> &'static str {
    tr("Save folder (for example D:\\tests\\results; empty for the default place)")
}
pub fn output_folder_hint() -> &'static str {
    tr("The test report (a <program>-<time> folder) and the coverage XML (<program>-<time>.cobertura.xml) are written under this folder. When empty, under %LOCALAPPDATA%\\netdbg.")
}
pub fn coverage_option() -> &'static str {
    tr("Collect coverage")
}
pub fn coverage_option_hint() -> &'static str {
    tr("Counts the lines run and the ways each branch (if, while, ?:, &&, ||, switch) went, and colors the source. Writes Cobertura XML at the end. It pauses briefly the first time a place runs, so it is a little slower.")
}
pub fn coverage_hint() -> &'static str {
    tr("Coverage: ● run ○ not run ◐ some branches (hover over a mark to see each branch).")
}
pub fn coverage_write() -> &'static str {
    tr("Write XML")
}
pub fn coverage_written() -> &'static str {
    tr("Wrote the coverage:")
}
pub fn coverage_rate_title() -> &'static str {
    tr("The share of lines run in this file")
}
pub fn break_on_sql() -> &'static str {
    tr("Break on SQL (text it contains, Enter)")
}
pub fn break_on_dialog() -> &'static str {
    tr("Break on dialogs (text it contains, Enter)")
}
pub fn field_time() -> &'static str {
    tr("Time")
}
pub fn field_thread() -> &'static str {
    tr("Thread")
}
pub fn field_duration() -> &'static str {
    tr("Duration")
}
pub fn field_failed() -> &'static str {
    tr("Failed")
}
pub fn field_data_source() -> &'static str {
    tr("Data source")
}
pub fn field_command_type() -> &'static str {
    tr("Command type")
}
pub fn field_transaction() -> &'static str {
    tr("Transaction")
}
pub fn field_repeat() -> &'static str {
    tr("Same statement count")
}
pub fn field_method() -> &'static str {
    tr("Method")
}
pub fn field_parameters() -> &'static str {
    tr("Parameters")
}
pub fn field_type() -> &'static str {
    tr("Type")
}
pub fn field_message() -> &'static str {
    tr("Message")
}
pub fn field_stopped() -> &'static str {
    tr("Stopped there")
}
pub fn field_context() -> &'static str {
    tr("Leading up to the exception")
}
pub fn field_snapshot() -> &'static str {
    tr("Stack when thrown")
}
pub fn field_caption() -> &'static str {
    tr("Caption")
}
pub fn field_text() -> &'static str {
    tr("Text")
}
pub fn field_form() -> &'static str {
    tr("Form")
}
pub fn field_event() -> &'static str {
    tr("Event")
}
pub fn field_note() -> &'static str {
    tr("Note")
}
pub fn field_statements() -> &'static str {
    tr("SQL run inside")
}
pub fn field_direction() -> &'static str {
    tr("Direction")
}
pub fn field_control() -> &'static str {
    tr("Control")
}
pub fn field_property() -> &'static str {
    tr("Property")
}
pub fn field_member() -> &'static str {
    tr("Member")
}
pub fn field_handler() -> &'static str {
    tr("Handler")
}
pub fn field_sender() -> &'static str {
    tr("Sender")
}
pub fn field_file() -> &'static str {
    tr("File")
}
pub fn field_arguments() -> &'static str {
    tr("Arguments")
}
pub fn field_kind() -> &'static str {
    tr("Kind")
}
pub fn field_bitness() -> &'static str {
    tr("Bitness")
}
pub fn field_process_id() -> &'static str {
    tr("Process")
}
pub fn field_followed() -> &'static str {
    tr("Followed")
}
pub fn field_caller() -> &'static str {
    tr("Caller")
}
pub fn field_frames() -> &'static str {
    tr("Call sequence")
}
pub fn env_user() -> &'static str {
    tr("User")
}
pub fn env_machine() -> &'static str {
    tr("Machine")
}
pub fn env_os() -> &'static str {
    tr("OS")
}
pub fn env_culture() -> &'static str {
    tr("Culture")
}
pub fn env_time_zone() -> &'static str {
    tr("Time zone")
}
pub fn env_runtime() -> &'static str {
    tr("Runtime")
}
pub fn env_program() -> &'static str {
    tr("Program")
}
pub fn env_command_line() -> &'static str {
    tr("Command line")
}
pub fn env_directory() -> &'static str {
    tr("Working folder")
}
pub fn env_connection_strings() -> &'static str {
    tr("Connection strings")
}
pub fn usage_running_only() -> &'static str {
    tr("While running, usage is read every 2 seconds.")
}
pub fn usage_private() -> &'static str {
    tr("Private Bytes")
}
pub fn usage_working_set() -> &'static str {
    tr("Working Set")
}
pub fn usage_cpu() -> &'static str {
    tr("CPU time")
}
pub fn usage_gdi() -> &'static str {
    tr("GDI objects")
}
pub fn usage_user() -> &'static str {
    tr("USER objects")
}
pub fn usage_handles() -> &'static str {
    tr("Handles")
}
pub fn usage_address() -> &'static str {
    tr("Address space (32-bit)")
}
pub fn usage_largest_free() -> &'static str {
    tr("Largest free block")
}
pub fn heap_stats() -> &'static str {
    tr("Heap statistics")
}
pub fn heap_stopped_only() -> &'static str {
    tr("Can be read while stopped (count and size per type).")
}
pub fn watch_writes() -> &'static str {
    tr("Break on write")
}
pub fn watch_writes_title() -> &'static str {
    tr("Stop when this field is written, by any code (.NET only, one field at a time)")
}
pub fn retention() -> &'static str {
    tr("Path")
}
pub fn retention_title() -> &'static str {
    tr("The path that keeps this object alive (from a GC root)")
}
pub fn retention_not_found() -> &'static str {
    tr("No path from a root was found")
}
pub fn exceptions_button() -> &'static str {
    tr("Exceptions")
}
pub fn exceptions_button_title() -> &'static str {
    tr("Exception settings: which exceptions to stop on when they are thrown")
}
pub fn exceptions_title() -> &'static str {
    tr("Exception Settings")
}
pub fn exceptions_hint() -> &'static str {
    tr("An exception nobody catches always stops. Changes apply at once, also while running.")
}
pub fn exceptions_user_unhandled() -> &'static str {
    tr("Stop on exceptions thrown in user code and not caught in user code")
}
pub fn exceptions_all() -> &'static str {
    tr("Stop when any exception is thrown")
}
pub fn exceptions_types() -> &'static str {
    tr("Stop when these are thrown")
}
pub fn exceptions_added() -> &'static str {
    tr("Added types")
}
pub fn exceptions_add_placeholder() -> &'static str {
    tr("Type with namespace (for example MyApp.OrderException)")
}
pub fn exceptions_add() -> &'static str {
    tr("Add")
}
pub fn exceptions_remove() -> &'static str {
    tr("Remove")
}
pub fn exceptions_invalid() -> &'static str {
    tr("Write a type name with its namespace (letters, digits, dots).")
}
pub fn replay() -> &'static str {
    tr("Replay")
}
pub fn replay_hint() -> &'static str {
    tr("Replay recorded steps (the AI's and hand operations, written next to the report as .steps.json) on the running program, for example on a newly delivered exe.")
}
pub fn replay_file_placeholder() -> &'static str {
    tr("Steps file to replay (.steps.json; filled in when the report is written)")
}
pub fn replay_item_placeholder() -> &'static str {
    tr("Item (empty: all)")
}
pub fn replay_file_needed() -> &'static str {
    tr("Enter the steps file to replay (.steps.json, written next to the report).")
}
pub fn replaying() -> &'static str {
    tr("Replaying the steps…")
}
pub fn replayable_title() -> &'static str {
    tr("This action is in the steps file and can be replayed")
}
pub fn tree_changes_first() -> &'static str {
    tr("Stopped before the windows were read, so there is nothing to compare yet.")
}
pub fn tree_changes_none() -> &'static str {
    tr("No control changed since the previous stop.")
}
pub fn observe_only() -> &'static str {
    tr("Observe only")
}
pub fn observe_only_hint() -> &'static str {
    tr("Attach only to observe (for a shared test environment): changing values, Edit and Continue, running the program's functions and input to its windows are refused, so that the debugger changes no data. Fields, stacks, records and screenshots work.")
}
pub fn observe_only_badge() -> &'static str {
    tr("observe only")
}